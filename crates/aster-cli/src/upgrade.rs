//! `aster upgrade`: download a released binary from a deterministic
//! `github.com/.../releases/download/` URL (no GitHub API, so no rate limit),
//! verify its sha256, and swap it in place with `self-replace`.

use std::time::Instant;

use anyhow::{Context, Result};
use clap::Args;
use kiln::install::{self, Bar, Change, Live};
use ratatui::text::Line;

const REPO_OWNER: &str = "zfinix";
const REPO_NAME: &str = "aster";

#[derive(Args)]
pub struct UpgradeArgs {
    /// Upgrade to a specific version (e.g. 0.4.0) instead of the latest.
    #[arg(long, value_name = "VERSION")]
    version: Option<String>,
    /// Reinstall even when the running binary is already current.
    #[arg(long)]
    force: bool,
    /// Set by the automatic update: install only a newer release, silently.
    #[arg(long, hide = true)]
    background: bool,
}

pub async fn run(args: UpgradeArgs) -> Result<()> {
    let started = Instant::now();
    let live = Live::start("Resolving latest release...");
    let tag = match &args.version {
        Some(version) => normalize_tag(version),
        None => tokio::task::spawn_blocking(latest_cli_tag)
            .await?
            .context("latest release lookup failed")?,
    };
    let version = tag
        .strip_prefix("cli-v")
        .with_context(|| format!("unexpected release tag: {tag}"))?
        .to_string();
    live.println(&install::summary(
        "Resolved",
        &format!("aster {version}"),
        started.elapsed(),
    ));
    let current = env!("CARGO_PKG_VERSION");
    if args.background {
        let probe = std::env::current_exe()?.with_file_name(".aster-update-probe");
        if !crate::update::is_newer(&version, current) || std::fs::write(&probe, b"").is_err() {
            return Ok(());
        }
        let _ = std::fs::remove_file(probe);
    }
    if !args.force && version == current {
        live.finish();
        install::emit(&Line::from(format!(
            "aster {current} is already the latest version"
        )));
        return Ok(());
    }

    // Blocking HTTP and file I/O, so keep it off the async executor.
    let install_version = version.clone();
    let (live, installed) = tokio::task::spawn_blocking(move || {
        let installed = install(&tag, &install_version, &live);
        (live, installed)
    })
    .await?;
    live.finish();
    installed.context("install failed")?;

    install::emit(&install::summary(
        "Installed",
        &format!("aster {version}"),
        started.elapsed(),
    ));
    match version == current {
        true => install::emit(&install::change(Change::Reinstalled, "aster", &version)),
        false => {
            install::emit(&install::change(Change::Removed, "aster", current));
            install::emit(&install::change(Change::Added, "aster", &version));
        }
    }
    Ok(())
}

fn install(tag: &str, version: &str, live: &Live) -> Result<()> {
    let target = target_triple();
    let base = format!(
        "https://github.com/{REPO_OWNER}/{REPO_NAME}/releases/download/{tag}/aster-{version}-{target}"
    );
    let (asset_url, zipped) = if cfg!(windows) {
        (format!("{base}.zip"), true)
    } else {
        (format!("{base}.tar.gz"), false)
    };

    let started = Instant::now();
    let label = format!("aster {version}");
    live.set_message(&format!("Downloading {label}..."));
    let archive_path =
        download(&asset_url, "archive", Some((live, &label))).context("asset download failed")?;
    live.println(&install::summary("Downloaded", &label, started.elapsed()));

    let checked = Instant::now();
    let expected_path = download(&format!("{asset_url}.sha256"), "checksum", None).ok();
    let expected = expected_path
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|s| s.trim().to_string());
    match expected {
        Some(expected) => {
            let archive = std::fs::read(&archive_path).context("temp archive read failed")?;
            verify_sha256(&archive, &expected).context("checksum verification failed")?;
            live.println(&install::summary("Verified", "checksum", checked.elapsed()));
        }
        None => live.println(&install::warning(
            "No checksum was published for this release, so the download was not verified.",
        )),
    }

    live.set_message(&format!("Installing {label}..."));
    let binary = unpack_from_file(&archive_path, &format!("aster-{version}-{target}"), zipped)?;
    self_replace::self_replace(&binary).context("binary replacement failed")?;
    Ok(())
}

/// `shown` puts a bar for the download in the live rows, labelled; small
/// files (a 65 B checksum) never get one.
fn download(url: &str, name: &str, shown: Option<(&Live, &str)>) -> Result<std::path::PathBuf> {
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(30))
        .timeout(std::time::Duration::from_secs(30 * 60))
        .build()
        .context("http client build failed")?;
    let response = client.get(url).send()?.error_for_status()?;

    let total = response.content_length();
    let path = std::env::temp_dir().join(format!("aster-download-{}-{name}", std::process::id()));
    let mut file = std::fs::File::create(&path).context("temp file creation failed")?;

    let bar = shown
        .zip(total.filter(|&t| t > 1 << 20))
        .map(|((live, label), total)| live.bar(label, total));

    const CHUNKS: u64 = 16;
    let parallel = total.is_some_and(|t| t > 1 << 20)
        && response
            .headers()
            .get(reqwest::header::ACCEPT_RANGES)
            .is_some_and(|v| v.to_str().is_ok_and(|v| v.contains("bytes")));

    let result = if parallel {
        drop(response);
        let total = total.unwrap_or_default();
        file.set_len(total).context("temp file sizing failed")?;
        let chunk_size = total.div_ceil(CHUNKS);
        let mut handles = Vec::new();
        for chunk in 0..CHUNKS {
            let start = chunk * chunk_size;
            let end = (start + chunk_size).min(total);
            if start >= end {
                break;
            }
            let client = client.clone();
            let url = url.to_string();
            let path = path.clone();
            let bar = bar.clone();
            handles.push(
                std::thread::Builder::new()
                    .name(format!("aster-dl-{chunk}"))
                    .spawn(move || -> Result<()> {
                        let mut resp = client
                            .get(&url)
                            .header(reqwest::header::RANGE, format!("bytes={start}-{}", end - 1))
                            .send()?
                            .error_for_status()?;
                        let mut file = std::fs::OpenOptions::new()
                            .write(true)
                            .open(&path)
                            .context("temp file open failed")?;
                        use std::io::{Read, Seek};
                        file.seek(std::io::SeekFrom::Start(start))
                            .context("temp file seek failed")?;
                        let mut reader = ProgressReader {
                            inner: (&mut resp).take(end - start),
                            bar: bar.as_ref(),
                        };
                        std::io::copy(&mut reader, &mut file).context("download interrupted")?;
                        Ok(())
                    })
                    .context("download thread spawn failed")?,
            );
        }
        let joined: Result<Vec<_>> = handles
            .into_iter()
            .map(|h| {
                h.join()
                    .map_err(|_| anyhow::anyhow!("download thread panicked"))?
            })
            .collect();
        joined.map(|_| ())
    } else {
        let mut reader = ProgressReader {
            inner: response,
            bar: bar.as_ref(),
        };
        std::io::copy(&mut reader, &mut file)
            .map(|_| ())
            .context("download interrupted")
    };
    if let Some(bar) = &bar {
        bar.finish();
    }
    result?;
    Ok(path)
}

struct ProgressReader<'a, R: std::io::Read> {
    inner: R,
    bar: Option<&'a Bar>,
}

impl<R: std::io::Read> std::io::Read for ProgressReader<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        if let Some(bar) = self.bar {
            bar.inc(n as u64);
        }
        Ok(n)
    }
}

fn verify_sha256(bytes: &[u8], expected: &str) -> Result<()> {
    use sha2::{Digest, Sha256};
    let actual = format!("{:x}", Sha256::digest(bytes));
    if !expected.eq_ignore_ascii_case(&actual) {
        anyhow::bail!("checksum mismatch: expected {expected}, got {actual}");
    }
    Ok(())
}

fn unpack_from_file(
    archive_path: &std::path::Path,
    dir_name: &str,
    zipped: bool,
) -> Result<std::path::PathBuf> {
    let dir = std::env::temp_dir().join(dir_name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).context("temp dir creation failed")?;

    if zipped {
        let archive = std::fs::read(archive_path).context("temp archive read failed")?;
        unpack_zip(&archive, &dir)?;
    } else {
        let file = std::fs::File::open(archive_path).context("temp archive open failed")?;
        let reader = std::io::BufReader::new(file);
        unpack_tar_gz_stream(reader, &dir)?;
    }
    find_binary(&dir)
}

fn find_binary(dir: &std::path::Path) -> Result<std::path::PathBuf> {
    let name = bin_name();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let mut entries =
            std::fs::read_dir(&current).with_context(|| format!("read {}", current.display()))?;
        while let Some(entry) = entries.next().transpose().context("temp dir read failed")? {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.file_name().is_some_and(|n| n == name) {
                return Ok(path);
            }
        }
    }
    anyhow::bail!("archive did not contain {name}")
}

fn unpack_tar_gz_stream<R: std::io::Read>(reader: R, dir: &std::path::Path) -> Result<()> {
    let gz = flate2::read::GzDecoder::new(reader);
    let mut tar = tar::Archive::new(gz);
    tar.unpack(dir).context("tar extraction failed")?;
    Ok(())
}

#[cfg(windows)]
fn unpack_zip(archive: &[u8], dir: &std::path::Path) -> Result<()> {
    let zip_path = dir.join("aster.zip");
    std::fs::write(&zip_path, archive).context("temp archive write failed")?;
    let status = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "Expand-Archive",
            "-Force",
            &format!("'{}'", zip_path.display()),
            &format!("'{}'", dir.display()),
        ])
        .status()
        .context("Expand-Archive failed")?;
    if !status.success() {
        anyhow::bail!("Expand-Archive exited with {status}");
    }
    let _ = std::fs::remove_file(&zip_path);
    Ok(())
}

#[cfg(unix)]
fn unpack_zip(_: &[u8], _: &std::path::Path) -> Result<()> {
    anyhow::bail!("zip archives are only published for Windows releases")
}

fn bin_name() -> &'static str {
    if cfg!(windows) { "aster.exe" } else { "aster" }
}

fn target_triple() -> &'static str {
    match (std::env::consts::ARCH, std::env::consts::OS) {
        ("aarch64", "macos") => "aarch64-apple-darwin",
        ("x86_64", "macos") => "x86_64-apple-darwin",
        ("x86_64", "linux") => "x86_64-unknown-linux-gnu",
        ("aarch64", "linux") => "aarch64-unknown-linux-gnu",
        ("x86_64", "windows") => "x86_64-pc-windows-msvc",
        _ => "unknown",
    }
}

fn normalize_tag(version: &str) -> String {
    match version {
        v if v.starts_with("cli-v") => v.to_string(),
        v if v.starts_with('v') => format!("cli-{v}"),
        v => format!("cli-v{v}"),
    }
}

/// `releases/latest` points at whatever release was published last, which is
/// often a `vscode-v*` one, so read the release feed (static, so still no API
/// rate limit) and take the newest CLI tag from it.
fn latest_cli_tag() -> Result<String> {
    let feed = format!("https://github.com/{REPO_OWNER}/{REPO_NAME}/releases.atom");
    let body = reqwest::blocking::get(feed)?.error_for_status()?.text()?;
    let tag = first_cli_tag(&body).context(
        "no aster release found in GitHub's release feed; \
         pass --version to install a specific release",
    )?;
    Ok(tag.to_string())
}

fn first_cli_tag(feed: &str) -> Option<&str> {
    feed.split("Repository/")
        .skip(1)
        .filter_map(|entry| entry.split_once("</id>"))
        .filter_map(|(id, _)| id.trim().rsplit('/').next())
        .find(|tag| tag.starts_with("cli-v"))
}

#[cfg(test)]
#[path = "tests/upgrade_test.rs"]
mod tests;
