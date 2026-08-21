//! `open_preview`: hand finished visual work to the user's browser instead of
//! only describing it. Resolves a URL or a repo file to something a browser can
//! load, refuses schemes that are not a page, and asks before opening anything
//! that is not this machine's own repo or loopback.

use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};

use crate::chat::{Answer, SessionCtx, UiSender, request_approval};

/// How long a local dev server has to accept a connection before the preview
/// is refused. Long enough for a listening socket, short enough that a dead
/// port does not stall the turn.
const PROBE_TIMEOUT: Duration = Duration::from_millis(700);

/// Set it to keep Aster out of the browser: over SSH, in a container, or on a
/// box where launching one is not wanted. The URL still comes back, so the
/// agent reports it instead of opening it.
const NO_BROWSER: &str = "ASTER_NO_BROWSER";

/// A target resolved to something a browser can load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Preview {
    /// The URL handed to the browser.
    pub url: String,
    /// True when the target is loopback or a file inside the repository, which
    /// opens without asking. Anything else is a page the agent chose to send
    /// the user to, so the user confirms it.
    pub local: bool,
    /// Loopback port to probe before opening, so a dead dev server is reported
    /// as such instead of showing the user a connection error.
    pub port: Option<u16>,
}

/// Hands a resolved URL to the browser. Swapped in tests, which have every
/// reason to exercise the tool and none to open a window.
type Launch = fn(&str) -> Result<()>;

fn browser(url: &str) -> Result<()> {
    open::that_detached(url).with_context(|| format!("opening {url}"))
}

/// Open `target` in the user's browser. `description` is what the model says
/// the user is about to see; it rides the approval prompt and the result.
pub(crate) async fn open_preview(
    repo_root: &Path,
    approver: Option<&UiSender>,
    ctx: &SessionCtx,
    target: &str,
    description: Option<&str>,
) -> Result<String> {
    open_with(repo_root, approver, ctx, target, description, browser).await
}

async fn open_with(
    repo_root: &Path,
    approver: Option<&UiSender>,
    ctx: &SessionCtx,
    target: &str,
    description: Option<&str>,
    launch: Launch,
) -> Result<String> {
    let preview = resolve(repo_root, target)?;
    if let Some(port) = preview.port {
        probe(port).await?;
    }
    if already_open(ctx, &preview.url) {
        return Ok(format!(
            "{} is already open in the browser from earlier this session; the \
             tab is still there, so tell the user to reload it rather than \
             opening a second one",
            preview.url
        ));
    }
    if !preview.local {
        let what = description.unwrap_or("a page");
        let preview_text = format!("Open {} in your browser\n\n{what}", preview.url);
        if request_approval(approver, preview_text, None).await == Answer::No {
            bail!(
                "the user declined to open {}; give them the link in your reply instead",
                preview.url
            );
        }
    }
    if suppressed() {
        return Ok(format!(
            "{} is ready but {NO_BROWSER} is set, so nothing was opened; put \
             the URL in your reply for the user to click",
            preview.url
        ));
    }
    launch(&preview.url)?;
    remember_open(ctx, &preview.url);
    Ok(format!(
        "opened {} in the user's browser; say so in your reply and describe \
         what they are looking at, without opening it again",
        preview.url
    ))
}

/// Turn what the model passed into a URL a browser can load.
pub(crate) fn resolve(repo_root: &Path, target: &str) -> Result<Preview> {
    let target = target.trim();
    if target.is_empty() {
        bail!("open_preview needs a `target`: a URL, or a path to a file in the repo");
    }
    match scheme(target) {
        Some("http") | Some("https") => http_preview(target),
        Some("file") => {
            let path = target.strip_prefix("file://").unwrap_or(target);
            path_preview(repo_root, path)
        }
        Some(other) => bail!(
            "open_preview opens pages, not `{other}:` URLs. Pass an http(s) URL \
             or a path to a file in the repo."
        ),
        None => match host_port(target) {
            Some(url) => http_preview(&url),
            None => path_preview(repo_root, target),
        },
    }
}

/// The scheme of `target`, when it has one. A single letter is a Windows drive
/// and a digit after the colon is a port, so neither counts.
fn scheme(target: &str) -> Option<&str> {
    let (head, rest) = target.split_once(':')?;
    if head.len() < 2
        || !head
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-')
    {
        return None;
    }
    if rest.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    Some(head)
}

/// `localhost:5173`, `127.0.0.1:8080/about`, and a bare `:3000` as the http
/// URL they meant. Anything else is left for the path branch.
fn host_port(target: &str) -> Option<String> {
    let (host, rest) = target.split_once(':')?;
    let port: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if port.is_empty() {
        return None;
    }
    let host = if host.is_empty() { "localhost" } else { host };
    is_loopback(host).then(|| format!("http://{host}:{rest}"))
}

fn http_preview(url: &str) -> Result<Preview> {
    let authority = authority(url).context("that URL has no host")?;
    if authority.is_empty() {
        bail!("that URL has no host");
    }
    let host = host(authority);
    let local = is_loopback(host);
    Ok(Preview {
        url: url.to_string(),
        local,
        port: local.then(|| port(authority, url)).flatten(),
    })
}

fn path_preview(repo_root: &Path, target: &str) -> Result<Preview> {
    let expanded = crate::edits::expand_home(target);
    let joined = if expanded.is_absolute() {
        expanded
    } else {
        repo_root.join(expanded)
    };
    let resolved = joined.canonicalize().with_context(|| {
        format!(
            "no such file: {target}. Build the page first, or pass the URL of a running server."
        )
    })?;
    let resolved = entry_point(resolved)?;
    let inside = repo_root
        .canonicalize()
        .is_ok_and(|root| resolved.starts_with(root));
    Ok(Preview {
        url: file_url(&resolved),
        local: inside,
        port: None,
    })
}

/// A directory is not a page: open its `index.html` when it has one, and say
/// what is missing when it does not.
fn entry_point(path: PathBuf) -> Result<PathBuf> {
    if !path.is_dir() {
        return Ok(path);
    }
    let index = path.join("index.html");
    if index.is_file() {
        return Ok(index);
    }
    bail!(
        "{} is a directory with no index.html; name the file to open",
        path.display()
    )
}

/// A `file://` URL. Percent-encodes the bytes a browser would otherwise read
/// as a query, a fragment, or an escape.
fn file_url(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let mut url = String::from("file://");
    if !text.starts_with('/') {
        url.push('/');
    }
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' | b':' => {
                url.push(byte as char);
            }
            _ => url.push_str(&format!("%{byte:02X}")),
        }
    }
    url
}

/// Everything between `://` and the path, which is where the host lives.
fn authority(url: &str) -> Option<&str> {
    let rest = url.split_once("://")?.1;
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    Some(&rest[..end])
}

/// The host alone. Userinfo is dropped from the front, since
/// `http://localhost@example.com/` is example.com wearing a local name.
fn host(authority: &str) -> &str {
    let bare = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    if let Some(end) = bare.strip_prefix('[').and_then(|rest| rest.find(']')) {
        return &bare[..end + 2];
    }
    bare.split(':').next().unwrap_or(bare)
}

/// The port to probe: the one in the URL, or the scheme's default.
fn port(authority: &str, url: &str) -> Option<u16> {
    let after_host = authority
        .rsplit_once(']')
        .map_or(authority, |(_, rest)| rest);
    match after_host.rsplit_once(':') {
        Some((_, digits)) => digits.parse().ok(),
        None if url.starts_with("https://") => Some(443),
        None => Some(80),
    }
}

fn is_loopback(host: &str) -> bool {
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    let lowered = bare.to_ascii_lowercase();
    if lowered == "localhost" || lowered.ends_with(".localhost") {
        return true;
    }
    match lowered.parse::<IpAddr>() {
        Ok(IpAddr::V4(v4)) => v4.is_loopback() || v4.is_unspecified(),
        Ok(IpAddr::V6(v6)) => v6.is_loopback() || v6.is_unspecified(),
        Err(_) => false,
    }
}

/// Refuse a loopback URL nothing is serving. The browser would show a
/// connection error the user reads as Aster being broken.
async fn probe(port: u16) -> Result<()> {
    let addr = format!("127.0.0.1:{port}");
    let connect = tokio::net::TcpStream::connect(&addr);
    match tokio::time::timeout(PROBE_TIMEOUT, connect).await {
        Ok(Ok(_)) => Ok(()),
        _ => bail!(
            "nothing is listening on port {port}. Start the dev server first, \
             then open the preview."
        ),
    }
}

fn suppressed() -> bool {
    std::env::var_os(NO_BROWSER).is_some_and(|v| !v.is_empty())
}

fn already_open(ctx: &SessionCtx, url: &str) -> bool {
    ctx.previews.lock().is_ok_and(|opened| opened.contains(url))
}

fn remember_open(ctx: &SessionCtx, url: &str) {
    if let Ok(mut opened) = ctx.previews.lock() {
        opened.insert(url.to_string());
    }
}

#[cfg(test)]
#[path = "tests/preview_test.rs"]
mod tests;
