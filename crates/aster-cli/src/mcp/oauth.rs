//! OAuth for remote MCP servers following the MCP authorization spec:
//! protected-resource metadata, dynamic client registration, authorization-code +
//! PKCE through the browser, and a token store under `~/.aster/mcp-auth/`.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use base64::Engine;
use rand::RngCore;
use serde::{Deserialize, Serialize};

/// How long any one OAuth round trip may take. A browser login is human-paced,
/// so the callback wait is its own, much longer budget.
const HTTP_TIMEOUT: Duration = Duration::from_secs(15);
pub(crate) const CALLBACK_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct StoredTokens {
    pub(crate) access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) refresh_token: Option<String>,
    /// Unix seconds after which `access_token` should be refreshed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) expires_at: Option<u64>,
    /// The client id this server issued us, reused on re-login.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) client_id: Option<String>,
}

fn auth_dir() -> Result<std::path::PathBuf> {
    let dir = crate::credentials::aster_dir()?.join("mcp-auth");
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    Ok(dir)
}

fn token_path(name: &str) -> Result<std::path::PathBuf> {
    // The name is a config key, but it still must not escape the directory.
    let safe: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    Ok(auth_dir()?.join(format!("{safe}.json")))
}

fn load_tokens(name: &str) -> Option<StoredTokens> {
    let bytes = std::fs::read(token_path(name).ok()?).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn store_tokens(name: &str, tokens: &StoredTokens) -> Result<()> {
    let path = token_path(name)?;
    let bytes = serde_json::to_vec_pretty(tokens)?;
    // Created 0o600 from the start so the token is never briefly world-readable.
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&path)
            .with_context(|| format!("writing {}", path.display()))?;
        file.write_all(&bytes)?;
    }
    #[cfg(not(unix))]
    std::fs::write(&path, bytes).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

pub fn has_stored_login(name: &str) -> bool {
    load_tokens(name).is_some()
}

/// A valid bearer token for `name`, refreshing once if it has expired.
/// `None` when no login is stored; connecting then proceeds unauthenticated.
pub async fn bearer_header(name: &str) -> Option<String> {
    let tokens = load_tokens(name)?;
    let fresh = match tokens.expires_at {
        Some(at) => now_secs() + 60 < at,
        None => true,
    };
    if fresh {
        return Some(format!("Bearer {}", tokens.access_token));
    }
    let refresh = tokens.refresh_token.clone()?;
    let metadata = discover(&server_url(name)?).await.ok()?;
    let renewed = refresh_token(&metadata.token_endpoint, &refresh).await?;
    let updated = StoredTokens {
        access_token: renewed.access_token,
        refresh_token: renewed.refresh_token.or(Some(refresh)),
        expires_at: expires_at(renewed.expires_in),
        client_id: tokens.client_id,
    };
    store_tokens(name, &updated).ok()?;
    Some(format!("Bearer {}", updated.access_token))
}

/// The configured url of a named server, read back from the settings.
fn server_url(name: &str) -> Option<String> {
    let settings = crate::settings::Settings::load(None).ok()?;
    settings
        .mcp
        .servers
        .get(name)
        .map(|config| config.url.trim().to_string())
        .filter(|url| !url.is_empty())
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn expires_at(expires_in: Option<u64>) -> Option<u64> {
    expires_in.map(|secs| now_secs() + secs)
}

// --- Metadata discovery -------------------------------------------------

#[derive(Debug, Deserialize)]
struct ProtectedResource {
    #[serde(default)]
    authorization_servers: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct AuthMetadata {
    authorization_endpoint: String,
    token_endpoint: String,
    #[serde(default)]
    registration_endpoint: Option<String>,
    #[serde(default)]
    scopes_supported: Vec<String>,
}

/// Find the authorization server for `url`: the resource-metadata route first,
/// then the well-known document on the origin itself.
async fn discover(url: &str) -> Result<AuthMetadata> {
    let http = http_client()?;
    let base = reqwest::Url::parse(url).context("parsing the MCP server url")?;

    let resource_url = well_known(&base, "oauth-protected-resource");
    if let Ok(response) = http.get(resource_url).send().await
        && response.status().is_success()
        && let Ok(resource) = response.json::<ProtectedResource>().await
        && let Some(server) = resource.authorization_servers.first()
    {
        let server = reqwest::Url::parse(server)
            .with_context(|| format!("parsing authorization server {server:?}"))?;
        return fetch_metadata(&http, &well_known(&server, "oauth-authorization-server")).await;
    }
    fetch_metadata(&http, &well_known(&base, "oauth-authorization-server")).await
}

/// RFC 8414 inserts the well-known segment before the path, not after it.
pub(crate) fn well_known(base: &reqwest::Url, segment: &str) -> reqwest::Url {
    let mut url = base.clone();
    let path = url.path().trim_end_matches('/').to_string();
    url.set_path(&format!("/.well-known/{segment}{path}"));
    url
}

async fn fetch_metadata(http: &reqwest::Client, url: &reqwest::Url) -> Result<AuthMetadata> {
    let response = http
        .get(url.clone())
        .send()
        .await
        .with_context(|| format!("reaching {url}"))?
        .error_for_status()
        .with_context(|| format!("no OAuth metadata at {url}"))?;
    response
        .json()
        .await
        .with_context(|| format!("parsing OAuth metadata from {url}"))
}

// --- Dynamic client registration ----------------------------------------

#[derive(Debug, Deserialize)]
struct Registration {
    client_id: String,
}

async fn register_client(
    http: &reqwest::Client,
    endpoint: &str,
    redirect_uri: &str,
    scope: &str,
) -> Result<Registration> {
    let body = serde_json::json!({
        "client_name": "aster",
        "redirect_uris": [redirect_uri],
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
        "token_endpoint_auth_method": "none",
        "scope": scope,
    });
    let response = http
        .post(endpoint)
        .json(&body)
        .send()
        .await
        .with_context(|| format!("registering with {endpoint}"))?
        .error_for_status()
        .with_context(|| {
            format!(
                "the server does not accept dynamic client registration ({endpoint}); \
                 register a client manually and set mcp.servers.<name>.headers instead"
            )
        })?;
    response
        .json()
        .await
        .context("parsing the registration response")
}

// --- Login flow ----------------------------------------------------------

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
}

pub(crate) use aster_ai::pkce::pkce;

/// Run the whole browser login for one configured remote server and store what
/// it hands back. Returns the account-facing summary printed by the CLI.
pub async fn login(name: &str) -> Result<String> {
    let url = server_url(name)
        .with_context(|| format!("no remote MCP server named {name:?} with a url"))?;
    let metadata = discover(&url).await?;
    let scope = metadata.scopes_supported.join(" ");

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .context("binding the loopback callback port")?;
    let port = listener.local_addr()?.port();
    let redirect_uri = format!("http://127.0.0.1:{port}/callback");

    let http = http_client()?;
    let registration = match &metadata.registration_endpoint {
        Some(endpoint) => register_client(&http, endpoint, &redirect_uri, &scope).await?,
        None => bail!(
            "the server publishes no dynamic client registration endpoint; \
             register a client manually and set mcp.servers.{name}.headers instead"
        ),
    };

    let code_verifier = pkce();
    let state = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode({
        let mut bytes = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut bytes);
        bytes
    });
    let authorize_url = build_authorize_url(
        &metadata.authorization_endpoint,
        &registration.client_id,
        &redirect_uri,
        &scope,
        &state,
        &code_verifier.challenge,
    )?;

    println!("Opening your browser to log in to {name}…");
    println!("If nothing opens, visit:\n  {authorize_url}");
    let _ = open::that(&authorize_url);

    let callback = tokio::time::timeout(CALLBACK_TIMEOUT, await_callback(listener)).await??;
    let code = callback.code;
    if callback.state.as_deref() != Some(state.as_str()) {
        bail!("the login callback carried a mismatched state; refusing it");
    }

    let tokens = exchange_code(
        &http,
        &metadata.token_endpoint,
        &registration.client_id,
        &code,
        &redirect_uri,
        &code_verifier.verifier,
    )
    .await?;
    let stored = StoredTokens {
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token,
        expires_at: expires_at(tokens.expires_in),
        client_id: Some(registration.client_id),
    };
    store_tokens(name, &stored)?;
    let path = token_path(name)?;
    Ok(format!(
        "logged in to {name}; token stored in {}",
        path.display()
    ))
}

pub(crate) fn build_authorize_url(
    endpoint: &str,
    client_id: &str,
    redirect_uri: &str,
    scope: &str,
    state: &str,
    challenge: &str,
) -> Result<String> {
    let mut url = reqwest::Url::parse(endpoint)
        .with_context(|| format!("parsing authorization endpoint {endpoint:?}"))?;
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("scope", scope)
        .append_pair("state", state)
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256");
    Ok(url.to_string())
}

#[derive(Debug)]
pub(crate) struct Callback {
    pub code: String,
    pub state: Option<String>,
}

/// Accept browser connections until one carries the sign-in redirect, and return
/// its decoded parameters. Speculative sockets and favicon probes are answered and
/// skipped rather than ending the wait.
pub(crate) async fn await_callback(listener: tokio::net::TcpListener) -> Result<Callback> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    loop {
        let (mut socket, _) = listener.accept().await?;
        // Read to the end of the headers: the redirect can straddle segments.
        let mut buffer: Vec<u8> = Vec::new();
        let mut chunk = [0u8; 2048];
        while !buffer.windows(4).any(|w| w == b"\r\n\r\n") && buffer.len() < 16 * 1024 {
            match socket.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(n) => buffer.extend_from_slice(&chunk[..n]),
            }
        }
        if buffer.is_empty() {
            continue;
        }
        let request = String::from_utf8_lossy(&buffer);
        let target = request
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .unwrap_or_default();
        let query = target.split_once('?').map(|(_, query)| query).unwrap_or("");
        let mut code = None;
        let mut state = None;
        let mut denied = None;
        for pair in query.split('&') {
            match pair.split_once('=') {
                Some(("code", value)) => code = Some(percent_decode(value)),
                Some(("state", value)) => state = Some(percent_decode(value)),
                Some(("error", value)) => denied = Some(percent_decode(value)),
                _ => {}
            }
        }
        if code.is_none() && denied.is_none() {
            let _ = socket
                .write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")
                .await;
            continue;
        }
        let page = match code.is_some() {
            true => "<html><body><p>Sign-in complete. You can close this tab.</p></body></html>",
            false => {
                "<html><body><p>Sign-in was cancelled. You can close this tab.</p></body></html>"
            }
        };
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
            page.len()
        );
        let _ = socket.write_all(response.as_bytes()).await;

        return match (code, denied) {
            (Some(code), _) => Ok(Callback { code, state }),
            (None, denied) => bail!(
                "the sign-in was declined in the browser ({})",
                denied.unwrap_or_default()
            ),
        };
    }
}

/// Percent-decode a query value: `%XX` sequences and `+` as space.
fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            b'%' if index + 3 <= bytes.len() => {
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok();
                match hex.and_then(|hex| u8::from_str_radix(hex, 16).ok()) {
                    Some(byte) => {
                        out.push(byte);
                        index += 3;
                    }
                    None => {
                        out.push(b'%');
                        index += 1;
                    }
                }
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

async fn exchange_code(
    http: &reqwest::Client,
    endpoint: &str,
    client_id: &str,
    code: &str,
    redirect_uri: &str,
    verifier: &str,
) -> Result<TokenResponse> {
    let response = http
        .post(endpoint)
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", client_id),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("code_verifier", verifier),
        ])
        .send()
        .await
        .with_context(|| format!("exchanging the code at {endpoint}"))?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        bail!(
            "the token endpoint at {endpoint} refused the exchange ({status}): {}",
            body.trim()
        );
    }
    let tokens: TokenResponse =
        serde_json::from_str(&body).context("parsing the token response")?;
    Ok(tokens)
}

async fn refresh_token(endpoint: &str, refresh_token: &str) -> Option<TokenResponse> {
    let http = http_client().ok()?;
    let response = http
        .post(endpoint)
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
        ])
        .send()
        .await
        .ok()?;
    let tokens: TokenResponse = response.error_for_status().ok()?.json().await.ok()?;
    Some(tokens)
}

fn http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .connect_timeout(HTTP_TIMEOUT)
        .timeout(HTTP_TIMEOUT)
        .user_agent(concat!("aster/", env!("CARGO_PKG_VERSION")))
        .build()
        .context("building the OAuth HTTP client")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_decode_decodes_escapes_and_plus() {
        assert_eq!(percent_decode("a%2Fb%20c+d"), "a/b c d");
        assert_eq!(percent_decode("plain"), "plain");
        assert_eq!(percent_decode("bad%2"), "bad%2");
        assert_eq!(percent_decode("trailing%"), "trailing%");
    }
}
