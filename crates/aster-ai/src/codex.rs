//! ChatGPT subscription auth for the Codex backend: OAuth PKCE against
//! auth.openai.com, tokens kept where the Codex CLI keeps them so one login serves
//! both. Wire translation lives in [`crate::codex_api`].

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use base64::Engine as _;
use serde::{Deserialize, Serialize};

/// Codex CLI's public OAuth client. It is not a secret; every Codex build ships it.
pub const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub const ISSUER: &str = "https://auth.openai.com";
pub const TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
pub const AUTHORIZE_URL: &str = "https://auth.openai.com/oauth/authorize";
pub const CODEX_BASE_URL: &str = "https://chatgpt.com/backend-api/codex";
const CALLBACK_PORT: u16 = 1455;
const SCOPES: &str = "openid profile email offline_access";
const REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
const NOT_LOGGED_IN: &str =
    "no ChatGPT/Codex login found. Run `aster login codex`, or sign in once with the Codex CLI";
pub const LOGIN_EXPIRED: &str =
    "your ChatGPT login has expired; run `aster login codex` to sign in again";

/// Same shape as `~/.codex/auth.json`, so a file written by either tool reads
/// back in the other.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct CodexAuth {
    #[serde(rename = "OPENAI_API_KEY", skip_serializing_if = "Option::is_none")]
    pub openai_api_key: Option<String>,
    pub tokens: Option<TokenSet>,
    #[serde(default)]
    pub last_refresh: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct TokenSet {
    pub id_token: String,
    pub access_token: String,
    pub refresh_token: String,
    #[serde(default)]
    pub account_id: Option<String>,
}

/// Where Aster keeps its own copy, in the XDG data dir alongside its other
/// credentials. A separate path from the Codex CLI's file so a Codex upgrade
/// never races a write from this side.
pub fn store_path(home: &Path) -> PathBuf {
    data_dir(home).join("codex.json")
}

/// `$XDG_DATA_HOME/aster` or `~/.local/share/aster`, the CLI's own data root.
fn data_dir(home: &Path) -> PathBuf {
    let root = match std::env::var_os("XDG_DATA_HOME").filter(|d| !d.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => home.join(".local").join("share"),
    };
    root.join("aster")
}

/// Where older builds put the store: `~/.aster`, and a nested `.aster` under
/// the data dir from a login that was handed the data dir as home.
fn legacy_store_paths(home: &Path) -> [PathBuf; 2] {
    [
        data_dir(home).join(".aster").join("codex.json"),
        home.join(".aster").join("codex.json"),
    ]
}

/// The Codex CLI's own store, imported read-only when Aster has none of its own.
pub fn codex_cli_path(home: &Path) -> PathBuf {
    home.join(".codex").join("auth.json")
}

/// Load Aster's store, falling back to the Codex CLI's. Returns None when
/// neither exists; the caller prompts a login. A store found at a legacy path
/// moves to the current one so every later read and refresh agree on it.
pub fn load(home: &Path) -> Option<CodexAuth> {
    if let Some(auth) = read_store(&store_path(home)) {
        return Some(auth);
    }
    for legacy in legacy_store_paths(home) {
        let Some(auth) = read_store(&legacy) else {
            continue;
        };
        if save(home, &auth).is_ok() {
            let _ = fs::remove_file(&legacy);
        }
        return Some(auth);
    }
    read_store(&codex_cli_path(home))
}

fn read_store(path: &Path) -> Option<CodexAuth> {
    let bytes = fs::read(path).ok()?;
    let auth = serde_json::from_slice::<CodexAuth>(&bytes).ok()?;
    auth.tokens.is_some().then_some(auth)
}

/// Persist to Aster's own store, created 0o600 so the tokens are never briefly
/// world-readable.
pub fn save(home: &Path, auth: &CodexAuth) -> Result<()> {
    let path = store_path(home);
    fs::create_dir_all(path.parent().expect("store path has a parent"))
        .with_context(|| format!("creating {}", path.parent().unwrap().display()))?;
    let bytes = serde_json::to_vec_pretty(auth)?;
    write_private(&path, &bytes)
}

pub fn clear(home: &Path) -> bool {
    let mut removed = fs::remove_file(store_path(home)).is_ok();
    for legacy in legacy_store_paths(home) {
        removed |= fs::remove_file(legacy).is_ok();
    }
    removed
}

#[cfg(unix)]
fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("writing {}", path.display()))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("securing {}", path.display()))?;
    f.write_all(bytes)
        .with_context(|| format!("writing {}", path.display()))
}

#[cfg(not(unix))]
fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))
}

/// The bearer token to send now, refreshing first when the access token is
/// expired or close to it. `home` is passed rather than read so callers that
/// already resolved it do not pay for it twice.
pub async fn ensure_access_token(home: &Path) -> Result<String> {
    let Some(mut auth) = load(home) else {
        bail!(NOT_LOGGED_IN);
    };
    let tokens = auth.tokens.as_ref().expect("load only returns some tokens");
    if !expired(&tokens.access_token) {
        return Ok(tokens.access_token.clone());
    }
    refresh_with(TOKEN_URL, home, &mut auth).await?;
    current_access_token(&auth)
}

/// Refresh unconditionally and return the new bearer. For the 401 the backend
/// sends when it has expired a token ahead of the JWT's own `exp` claim, which
/// [`ensure_access_token`] cannot see coming.
pub async fn refresh_access_token(home: &Path) -> Result<String> {
    let Some(mut auth) = load(home) else {
        bail!(NOT_LOGGED_IN);
    };
    refresh_with(TOKEN_URL, home, &mut auth).await?;
    current_access_token(&auth)
}

fn current_access_token(auth: &CodexAuth) -> Result<String> {
    auth.tokens
        .as_ref()
        .map(|t| t.access_token.clone())
        .context("refresh produced no tokens")
}

/// JWT expiry from the access token itself. Unparseable counts as expired so a
/// malformed token costs one refresh attempt instead of a 401 loop.
fn expired(access_token: &str) -> bool {
    jwt_claim(access_token, "exp")
        .and_then(|v| v.as_u64())
        .map(|exp| now_unix() + 60 > exp)
        .unwrap_or(true)
}

fn jwt_claim(token: &str, claim: &str) -> Option<serde_json::Value> {
    let payload = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    value.get(claim).cloned()
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// `token_url` is a parameter so tests can point it at a local server.
async fn refresh_with(token_url: &str, home: &Path, auth: &mut CodexAuth) -> Result<()> {
    let Some(tokens) = &auth.tokens else {
        bail!("no tokens to refresh");
    };
    let client = reqwest::Client::new();
    let resp = client
        .post(token_url)
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", tokens.refresh_token.as_str()),
            ("client_id", CLIENT_ID),
            ("scope", SCOPES),
        ])
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .context("token refresh request failed")?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        tracing::debug!(%status, body, "codex token refresh failed");
        bail!("{LOGIN_EXPIRED} ({status})");
    }
    let refreshed: TokenResponse = resp.json().await.context("decoding token response")?;
    auth.tokens = Some(TokenSet {
        id_token: refreshed
            .id_token
            .unwrap_or_else(|| tokens.id_token.clone()),
        access_token: refreshed.access_token,
        refresh_token: refreshed
            .refresh_token
            .unwrap_or_else(|| tokens.refresh_token.clone()),
        account_id: tokens.account_id.clone(),
    });
    auth.last_refresh = Some(rfc3339_now());
    save(home, auth)?;
    Ok(())
}

fn rfc3339_now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Run the browser PKCE flow and store the result. Blocks until the local
/// callback server sees a code or the user gives up.
pub async fn login(home: &Path) -> Result<CodexAuth> {
    let crate::pkce::Pkce {
        verifier,
        challenge,
    } = crate::pkce::pkce();
    // Fresh entropy per attempt: the provider rejects short state, and the
    // callback check refuses a redirect this attempt did not start.
    let state = crate::pkce::random_urlsafe();
    let url = authorize_url(&challenge, &state)?;
    println!(
        "Opening {} in your browser; approve access to continue.",
        home_display(url.as_str())
    );
    open_browser(url.as_str());

    let code = wait_for_code(&state)?;

    let client = reqwest::Client::new();
    let resp = client
        .post(TOKEN_URL)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("client_id", CLIENT_ID),
            ("code_verifier", verifier.as_str()),
            ("redirect_uri", REDIRECT_URI),
        ])
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .context("token exchange failed")?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        bail!("token exchange failed ({status}). {body}");
    }
    let exchanged = resp
        .json::<TokenResponse>()
        .await
        .context("decoding token response")?;

    let account_id = exchanged
        .id_token
        .as_deref()
        .and_then(|id| jwt_claim(id, "https://api.openai.com/auth"))
        .and_then(|v| v.get("chatgpt_account_id").cloned())
        .and_then(|v| v.as_str().map(str::to_string));
    let auth = CodexAuth {
        openai_api_key: None,
        tokens: Some(TokenSet {
            id_token: exchanged.id_token.unwrap_or_default(),
            access_token: exchanged.access_token,
            refresh_token: exchanged.refresh_token.unwrap_or_default(),
            account_id,
        }),
        last_refresh: Some(rfc3339_now()),
    };
    save(home, &auth)?;
    Ok(auth)
}

fn home_display(url: &str) -> &str {
    url.split('?').next().unwrap_or(url)
}

fn open_browser(url: &str) {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    #[cfg(windows)]
    let _ = std::process::Command::new("cmd")
        .args(["/c", "start", "", url])
        .spawn();
}

/// The authorize URL with every parameter percent-encoded; a raw `format!`
/// would put the scope list's literal spaces into the URL.
fn authorize_url(challenge: &str, state: &str) -> Result<reqwest::Url> {
    reqwest::Url::parse_with_params(
        AUTHORIZE_URL,
        [
            ("response_type", "code"),
            ("client_id", CLIENT_ID),
            ("redirect_uri", REDIRECT_URI),
            ("scope", SCOPES),
            ("code_challenge", challenge),
            ("code_challenge_method", "S256"),
            ("state", state),
        ],
    )
    .context("building the authorize URL")
}

/// What one request to the callback listener turned out to be.
enum Callback {
    Code(String),
    Refused(String),
    NotTheRedirect,
}

/// Classify one raw HTTP request against the state this attempt sent. Favicon
/// probes and empty speculative connections are NotTheRedirect, not failures.
fn parse_callback(req: &str, expected_state: &str) -> Callback {
    let target = req.split_whitespace().nth(1).unwrap_or_default();
    let Ok(url) = reqwest::Url::parse(&format!("http://localhost{target}")) else {
        return Callback::NotTheRedirect;
    };
    let param = |name: &str| {
        url.query_pairs()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.into_owned())
    };
    if let Some(error) = param("error") {
        let detail = param("error_description").unwrap_or_default();
        let reason = format!("the provider refused the login: {error} {detail}");
        return Callback::Refused(reason.trim_end().to_string());
    }
    let Some(code) = param("code") else {
        return Callback::NotTheRedirect;
    };
    if param("state").as_deref() != Some(expected_state) {
        return Callback::Refused(
            "the callback state did not match this login attempt; try again".into(),
        );
    }
    Callback::Code(code)
}

/// Localhost listener for the OAuth redirect. Blocking by design: the CLI has
/// nothing else to do until the browser comes back.
fn wait_for_code(expected_state: &str) -> Result<String> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", CALLBACK_PORT))
        .with_context(|| format!("binding port {CALLBACK_PORT}; is another login running?"))?;
    loop {
        let (stream, _) = listener
            .accept()
            .context("waiting for the browser callback")?;
        // A connection that never sends must not hang the login.
        let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
        let mut buf = [0u8; 4096];
        let n = std::io::Read::read(&mut (&stream), &mut buf).unwrap_or(0);
        let req = String::from_utf8_lossy(&buf[..n]);
        match parse_callback(&req, expected_state) {
            Callback::Code(code) => {
                respond(&stream, "200 OK", "Aster login complete.");
                return Ok(code);
            }
            Callback::Refused(reason) => {
                respond(
                    &stream,
                    "400 Bad Request",
                    "Aster login failed; see the terminal.",
                );
                bail!(reason);
            }
            Callback::NotTheRedirect => respond(&stream, "404 Not Found", ""),
        }
    }
}

fn respond(mut stream: &std::net::TcpStream, status: &str, body: &str) {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let _ = std::io::Write::write_all(&mut stream, response.as_bytes());
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    id_token: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
}

#[cfg(test)]
#[path = "codex_tests.rs"]
mod tests;
