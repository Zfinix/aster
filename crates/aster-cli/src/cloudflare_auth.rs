//! "Sign in with Cloudflare": PKCE through the dashboard to read the account id
//! the Workers AI endpoint needs. Cloudflare grants no scope that mints an API
//! token and none that renews one, so the key itself is still pasted.

use anyhow::{Context, Result, bail};
use aster_ai::keys::env_non_empty;
use serde::Deserialize;

use crate::mcp::oauth;

const AUTH_URL: &str = "https://dash.cloudflare.com/oauth2/auth";
const TOKEN_URL: &str = "https://dash.cloudflare.com/oauth2/token";
const API: &str = "https://api.cloudflare.com/client/v4";

/// Cloudflare matches the redirect exactly, so this must be registered verbatim.
const REDIRECT_URI: &str = "http://127.0.0.1:8976/oauth/callback";

/// Workers AI reads and writes. The dashboard shows permission names; these
/// are the ids the authorize endpoint accepts.
const SCOPES: &str = "ai.read ai.write account-settings.read";

const CLIENT_ID: &str = "5f4eb1922ca08bed0a1a238cbd13872f";
const CLIENT_ID_VAR: &str = "ASTER_CLOUDFLARE_CLIENT_ID";

pub(crate) const KEY_VAR: &str = "CLOUDFLARE_API_TOKEN";
pub(crate) const ACCOUNT_VAR: &str = "CLOUDFLARE_ACCOUNT_ID";

const TOKEN_PAGE: &str = "https://dash.cloudflare.com/profile/api-tokens";

fn client_id() -> String {
    env_non_empty(CLIENT_ID_VAR).unwrap_or_else(|| CLIENT_ID.to_string())
}

/// Sign in, store the key and account id, and return the summary plus the
/// endpoint the account resolves to.
pub async fn login() -> Result<(String, String)> {
    let client_id = client_id();
    let pkce = oauth::pkce();
    let state = oauth::pkce().verifier;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:8976")
        .await
        .context("binding 127.0.0.1:8976 for the login callback; is another sign-in running?")?;

    let url = oauth::build_authorize_url(
        AUTH_URL,
        &client_id,
        REDIRECT_URI,
        SCOPES,
        &state,
        &pkce.challenge,
    )?;

    let json = crate::json_mode();
    let instructions = format!(
        "\nTo sign in with Cloudflare, finish in your browser:\n  {url}\n\nWaiting for authorization…"
    );
    if json {
        eprintln!("{instructions}");
    } else {
        println!("{instructions}");
    }
    let _ = open::that(url.as_str());

    let callback = tokio::time::timeout(oauth::CALLBACK_TIMEOUT, oauth::await_callback(listener))
        .await
        .map_err(|_| anyhow::anyhow!("the sign-in was not finished in time; run it again"))??;
    if callback.state.as_deref() != Some(state.as_str()) {
        bail!("the sign-in came back with the wrong state; start it again");
    }

    let http = http_client()?;
    let access = exchange(&http, &client_id, &callback.code, &pkce.verifier).await?;
    let account = account(&http, &access).await?;
    let base_url = format!("{API}/accounts/{}/ai/v1", account.id);

    let key = ask_for_key(&account.name)?;
    let path = store(key.as_deref(), &account.id)?;
    // Reload so the key reaches this process too.
    let _ = dotenvy::from_path_override(&path);

    if json {
        println!(
            "{}",
            serde_json::json!({
                "ok": true,
                "provider": "cloudflare",
                "key_var": KEY_VAR,
                "account_id": account.id,
                "key_stored": key.is_some(),
            })
        );
        return Ok((String::new(), base_url));
    }
    let summary = match key.is_some() {
        true => format!(
            "Signed in to Cloudflare on {}. The token is stored as {KEY_VAR} in {}.",
            account.name,
            path.display()
        ),
        false => format!(
            "Found your account, stored as {ACCOUNT_VAR} in {}. Add the token \
             when you have it: `export {KEY_VAR}=…`.",
            path.display()
        ),
    };
    Ok((summary, base_url))
}

/// The sign-in proves who you are but cannot mint a key, so the key is pasted.
/// Skipping is fine: the account id alone already makes the endpoint usable.
fn ask_for_key(account: &str) -> Result<Option<String>> {
    println!(
        "\nFound {account}.\n\nCloudflare only issues API tokens by hand, so create one \
         with Workers AI permissions:\n  {TOKEN_PAGE}\n"
    );
    let _ = open::that(TOKEN_PAGE);
    let pasted = crate::util::or_cancel(
        cliclack::password(format!(
            "Paste the token (enter to skip and set {KEY_VAR} later)"
        ))
        .mask('•')
        .allow_empty()
        .interact(),
    )?;
    Ok(pasted.filter(|key| !key.trim().is_empty()))
}

pub async fn login_and_report() -> Result<()> {
    let (summary, _) = login().await?;
    if !summary.is_empty() {
        println!("{summary}");
    }
    Ok(())
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
}

async fn exchange(
    http: &reqwest::Client,
    client_id: &str,
    code: &str,
    verifier: &str,
) -> Result<String> {
    let response = http
        .post(TOKEN_URL)
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", client_id),
            ("code", code),
            ("redirect_uri", REDIRECT_URI),
            ("code_verifier", verifier),
        ])
        .send()
        .await
        .context("exchanging the sign-in code for a token")?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        bail!(
            "Cloudflare refused the sign-in exchange ({status}): {}",
            body.trim()
        );
    }
    let tokens: TokenResponse =
        serde_json::from_str(&body).context("parsing the token response")?;
    Ok(tokens.access_token)
}

#[derive(Deserialize)]
struct Envelope<T> {
    #[serde(default)]
    success: bool,
    #[serde(default)]
    errors: Vec<ApiError>,
    result: Option<T>,
}

#[derive(Deserialize)]
struct ApiError {
    message: String,
}

impl<T> Envelope<T> {
    fn into_result(self, what: &str) -> Result<T> {
        if !self.success {
            let why = self
                .errors
                .into_iter()
                .map(|e| e.message)
                .collect::<Vec<_>>()
                .join("; ");
            bail!("Cloudflare refused {what}: {why}");
        }
        self.result
            .with_context(|| format!("Cloudflare returned no {what}"))
    }
}

#[derive(Deserialize)]
struct Account {
    id: String,
    #[serde(default)]
    name: String,
}

/// The account the endpoint points at; one picks itself, several ask.
async fn account(http: &reqwest::Client, access: &str) -> Result<Account> {
    let envelope: Envelope<Vec<Account>> = http
        .get(format!("{API}/accounts"))
        .bearer_auth(access)
        .send()
        .await
        .context("listing your Cloudflare accounts")?
        .json()
        .await
        .context("decoding the account list")?;
    let mut accounts = envelope.into_result("the account list")?;
    match accounts.len() {
        0 => bail!(
            "that sign-in came back with no accounts, so there is no account id \
             to point Workers AI at. Set {ACCOUNT_VAR} by hand, or sign in again \
             once the OAuth client asks for account read."
        ),
        1 => Ok(accounts.remove(0)),
        _ => {
            let mut menu = cliclack::select::<usize>("Which account?");
            for (i, account) in accounts.iter().enumerate() {
                menu = menu.item(i, &account.name, &account.id);
            }
            let Some(i) = crate::util::or_cancel(menu.interact())? else {
                bail!("no account chosen");
            };
            Ok(accounts.remove(i))
        }
    }
}

fn store(key: Option<&str>, account_id: &str) -> Result<std::path::PathBuf> {
    let path = crate::persist::global_env_path().context("no home directory")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    if let Some(key) = key {
        crate::init::set_env_key(&path, KEY_VAR, key)?;
    }
    crate::init::set_env_key(&path, ACCOUNT_VAR, account_id)?;
    Ok(path)
}

fn http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent("aster-cli")
        .build()
        .context("building the http client")
}
