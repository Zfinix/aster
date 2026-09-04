//! "Sign in with Z.ai": the browser authorization the ZCode app uses, trading
//! the returned code for a GLM Coding Plan token stored as `ZAI_API_KEY` in
//! `~/.aster/.env`.

use anyhow::{Context, Result, bail};
use base64::Engine as _;
use rand::RngCore;
use serde::Deserialize;

const AUTHORIZE_URL: &str = "https://chat.z.ai/api/oauth/authorize";
const TOKEN_URL: &str = "https://zcode.z.ai/api/v1/oauth/token";
const BUSINESS_LOGIN_URL: &str = "https://api.z.ai/api/auth/z/login";
const CLIENT_ID: &str = "client_P8X5CMWmlaRO9gyO-KSqtg";
fn redirect_uri() -> String {
    format!(
        "https://zcode.z.ai/app/oauth/login?redirect=aster%3A%2F%2Foauth%2Fcallback&app_version={}",
        env!("CARGO_PKG_VERSION")
    )
}
const APP_RETURN_TO: &str = "https://zcode.z.ai/en";
pub(crate) const KEY_VAR: &str = "ZAI_API_KEY";
pub(crate) const CODING_BASE_URL: &str = "https://api.z.ai/api/coding/paas/v4";

const REFRESH_SKEW: u64 = 60;

pub async fn refresh() -> Result<std::path::PathBuf> {
    let session = crate::credentials::load()
        .zai
        .filter(|session| !session.is_empty())
        .context("no stored Z.ai sign-in; run `aster login zai`")?;
    let http = reqwest::Client::builder()
        .user_agent("aster-cli")
        .build()
        .context("building the http client")?;
    let candidates = sign_in_tokens(session.zcode_token, session.access_token);
    let token = mint(&http, &candidates).await?;
    let path = store_key(&token)?;
    let _ = dotenvy::from_path_override(&path);
    Ok(path)
}

async fn mint(http: &reqwest::Client, candidates: &[String]) -> Result<String> {
    if candidates.is_empty() {
        bail!("the sign-in carried no token");
    }
    let mut refusal = None;
    for candidate in candidates {
        match business_login(http, candidate).await {
            Ok(token) => return Ok(token),
            Err(err) => refusal = Some(err),
        }
    }
    for candidate in candidates {
        if serves_models(http, candidate).await {
            return Ok(candidate.clone());
        }
    }
    Err(refusal
        .expect("a candidate was tried")
        .context(NO_KEY_FROM_SIGN_IN))
}

const NO_KEY_FROM_SIGN_IN: &str = "signing in to Z.ai did not yield a usable key. \
Create one at https://z.ai/manage-apikey/apikey-list, then set it as ZAI_API_KEY \
(or run `aster init` and pick Z.ai)";

async fn serves_models(http: &reqwest::Client, token: &str) -> bool {
    let url = format!("{CODING_BASE_URL}/models");
    matches!(http.get(&url).bearer_auth(token).send().await, Ok(resp) if resp.status().is_success())
}

fn token_fields(raw: &serde_json::Value) -> String {
    let mut groups = Vec::new();
    for (label, value) in [("top level", raw), ("data", &raw["data"])] {
        if let Some(names) = field_names(value) {
            groups.push(format!("{label}: {names}"));
        }
    }
    if let Some(map) = raw["data"].as_object() {
        for (key, value) in map {
            if let Some(names) = field_names(value) {
                groups.push(format!("data.{key}: {names}"));
            }
        }
    }
    if groups.is_empty() {
        return "nothing".to_string();
    }
    groups.join("; ")
}

fn field_names(value: &serde_json::Value) -> Option<String> {
    let map = value.as_object()?;
    let mut names: Vec<&str> = map.keys().map(String::as_str).collect();
    names.sort_unstable();
    (!names.is_empty()).then(|| names.join(", "))
}

pub async fn refresh_if_stale() {
    let Some(expires_at) = crate::credentials::load().zai.and_then(|s| s.expires_at) else {
        return;
    };
    let Ok(now) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) else {
        return;
    };
    if !is_spent(expires_at, now.as_secs()) {
        return;
    }
    if let Err(err) = refresh().await {
        tracing::debug!("the stored Z.ai sign-in could not be refreshed: {err:#}");
    }
}

fn is_spent(expires_at: u64, now: u64) -> bool {
    expires_at <= now.saturating_add(REFRESH_SKEW)
}

pub async fn login() -> Result<String> {
    if let Ok(path) = refresh().await {
        return Ok(finish(None, &path, true));
    }
    let nonce = nonce();
    let state = encode_state(&nonce);
    let url = authorize_url(&state)?;

    let instructions = format!(
        "\nTo sign in with Z.ai, finish in your browser:\n  {url}\n\n\
         When the Aster app is installed, your browser hands the code back to it \
         and this command finishes on its own. Otherwise copy the final address \
         (it starts with aster://oauth/callback) and paste it below."
    );
    if crate::json_mode() {
        eprintln!("{instructions}");
    } else {
        println!("{instructions}");
    }
    let _ = open::that(url.as_str());

    let pasted = read_callback().await?;
    let code = callback_code(&pasted, &nonce)?;
    let session = exchange(&code, &state).await?;
    let path = store_key(&session.token)?;
    if !session.upstream.is_empty() {
        crate::credentials::store_zai_session(session.upstream)?;
    }
    let _ = dotenvy::from_path_override(&path);

    Ok(finish(session.account, &path, false))
}

fn finish(account: Option<String>, path: &std::path::Path, reused: bool) -> String {
    if crate::json_mode() {
        println!(
            "{}",
            serde_json::json!({
                "ok": true,
                "provider": "zai",
                "key_var": KEY_VAR,
                "base_url": CODING_BASE_URL,
                "account": account,
                "reused": reused,
            })
        );
        return String::new();
    }
    let mut summary = match (&account, reused) {
        (_, true) => "Reused your stored Z.ai sign-in.".to_string(),
        (Some(account), false) => format!("Signed in as {account}."),
        (None, false) => "Signed in.".to_string(),
    };
    summary.push_str(&format!(
        " The token is stored as {KEY_VAR} in {}.",
        path.display()
    ));
    summary
}

pub async fn login_and_report() -> Result<()> {
    let summary = login().await?;
    if !summary.is_empty() {
        println!("{summary}");
    }
    adopt_coding_endpoint()
}

fn authorize_url(state: &str) -> Result<reqwest::Url> {
    let mut url = reqwest::Url::parse(AUTHORIZE_URL).context("parsing the Z.ai auth url")?;
    url.query_pairs_mut()
        .append_pair("redirect_uri", &redirect_uri())
        .append_pair("response_type", "code")
        .append_pair("client_id", CLIENT_ID)
        .append_pair("state", state);
    Ok(url)
}

fn nonce() -> String {
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn encode_state(nonce: &str) -> String {
    let payload = serde_json::json!({
        "nonce": nonce,
        "app_return_to": APP_RETURN_TO,
        "redirect_uri": redirect_uri(),
    });
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload.to_string())
}

fn state_nonce(state: &str) -> Option<String> {
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(state)
        .ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    value
        .get("nonce")?
        .as_str()
        .map(std::string::ToString::to_string)
}

fn callback_file() -> Result<std::path::PathBuf> {
    let dir = crate::persist::global_env_path()
        .context("no home directory")?
        .parent()
        .context("no config directory")?
        .to_path_buf();
    Ok(dir.join("login-callback"))
}

async fn read_callback() -> Result<String> {
    let file = callback_file()?;
    let _ = std::fs::remove_file(&file);
    let prompt = "Paste the address you were redirected to";
    let stdin = tokio::task::spawn_blocking(move || read_pasted(prompt));
    loop {
        if let Ok(text) = std::fs::read_to_string(&file)
            && !text.trim().is_empty()
        {
            return Ok(text);
        }
        if stdin.is_finished() {
            return stdin
                .await
                .context("reading the pasted address")?
                .context("reading the pasted address");
        }
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    }
}

fn read_pasted(prompt: &str) -> Result<String> {
    if console::Term::stdout().features().is_attended() {
        let pasted: String = cliclack::input(prompt).required(true).interact()?;
        return Ok(pasted);
    }
    eprintln!("{prompt}:");
    let mut line = String::new();
    std::io::BufRead::read_line(&mut std::io::stdin().lock(), &mut line)
        .context("reading the pasted address")?;
    if line.trim().is_empty() {
        bail!("no address pasted; run `aster login zai` again");
    }
    Ok(line)
}

fn callback_code(pasted: &str, nonce: &str) -> Result<String> {
    let pasted = pasted.trim();
    let Ok(url) = reqwest::Url::parse(pasted) else {
        if pasted.contains(char::is_whitespace) {
            bail!("that does not look like the redirect address or a code");
        }
        return Ok(pasted.to_string());
    };
    let mut code = None;
    let mut state = None;
    let mut error = None;
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "code" | "authCode" if code.is_none() => code = Some(value.trim().to_string()),
            "state" => state = Some(value.trim().to_string()),
            "error" => error = Some(value.trim().to_string()),
            _ => {}
        }
    }
    if let Some(error) = error.filter(|e| !e.is_empty()) {
        bail!("Z.ai refused the sign-in: {error}");
    }
    match state.as_deref().and_then(state_nonce) {
        Some(found) if found == nonce => {}
        Some(_) => bail!("that address is from a different sign-in; run `aster login zai` again"),
        None => bail!("that address carries no usable state; paste the whole redirect address"),
    }
    code.filter(|c| !c.is_empty())
        .context("that address carries no code; paste the whole redirect address")
}

struct Session {
    token: String,
    account: Option<String>,
    upstream: crate::credentials::ZaiSession,
}

#[derive(Deserialize)]
struct Envelope {
    code: i64,
    #[serde(default)]
    msg: Option<String>,
    #[serde(default)]
    data: Option<TokenData>,
}

#[derive(Deserialize)]
struct TokenData {
    #[serde(default)]
    token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    zai: Option<ZaiTokens>,
    #[serde(default)]
    user: Option<User>,
}

#[derive(Deserialize)]
struct ZaiTokens {
    #[serde(default)]
    access_token: String,
}

#[derive(Deserialize)]
struct User {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    email: Option<String>,
}

async fn exchange(code: &str, state: &str) -> Result<Session> {
    let http = reqwest::Client::builder()
        .user_agent("aster-cli")
        .build()
        .context("building the http client")?;
    let raw: serde_json::Value = http
        .post(TOKEN_URL)
        .json(&serde_json::json!({
            "code": code,
            "redirect_uri": redirect_uri(),
            "state": state,
        }))
        .send()
        .await
        .context("exchanging the sign-in code for a token")?
        .error_for_status()
        .context("Z.ai refused the sign-in exchange")?
        .json()
        .await
        .context("decoding the exchange response")?;
    let envelope: Envelope =
        serde_json::from_value(raw.clone()).context("decoding the exchange response")?;
    if envelope.code != 0 {
        let msg = envelope
            .msg
            .filter(|m| !m.trim().is_empty())
            .unwrap_or_else(|| format!("code {}", envelope.code));
        bail!("Z.ai refused the sign-in exchange: {msg}");
    }
    let data = envelope.data.context("the exchange returned no token")?;
    let account = data.user.and_then(|user| {
        user.name
            .or(user.email)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    });
    let zcode_token = clean(data.token);
    let access_token = clean(data.zai.map(|zai| zai.access_token));
    let candidates = sign_in_tokens(zcode_token.clone(), access_token.clone());
    let token = mint(&http, &candidates)
        .await
        .with_context(|| format!("the exchange carried {}", token_fields(&raw)))?;
    let upstream = crate::credentials::ZaiSession {
        zcode_token,
        access_token,
        expires_at: expires_at(data.expires_in),
    };
    Ok(Session {
        token,
        account,
        upstream,
    })
}

fn sign_in_tokens(zcode_token: Option<String>, access_token: Option<String>) -> Vec<String> {
    let mut tokens: Vec<String> = [zcode_token, access_token].into_iter().flatten().collect();
    tokens.dedup();
    tokens
}

fn clean(token: Option<String>) -> Option<String> {
    token
        .map(|token| token.trim().to_string())
        .filter(|token| !token.is_empty())
}

fn expires_at(expires_in: Option<u64>) -> Option<u64> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    expires_in.map(|secs| now.saturating_add(secs))
}

async fn business_login(http: &reqwest::Client, access_token: &str) -> Result<String> {
    let envelope: serde_json::Value = http
        .post(BUSINESS_LOGIN_URL)
        .json(&serde_json::json!({ "token": access_token }))
        .send()
        .await
        .context("signing in to the Z.ai API")?
        .error_for_status()
        .context("Z.ai refused the API sign-in")?
        .json()
        .await
        .context("decoding the API sign-in response")?;
    let code = envelope.get("code").and_then(serde_json::Value::as_i64);
    if code.is_some_and(|code| code != 0 && code != 200) {
        let msg = envelope
            .get("msg")
            .and_then(serde_json::Value::as_str)
            .filter(|msg| !msg.trim().is_empty())
            .map_or_else(
                || format!("code {}", code.unwrap_or_default()),
                str::to_string,
            );
        bail!("Z.ai refused the API sign-in: {msg}");
    }
    let data = envelope
        .get("data")
        .context("the API sign-in returned no data")?;
    if let Some(token) = data.as_str().map(str::trim).filter(|t| !t.is_empty()) {
        return Ok(token.to_string());
    }
    for field in ["token", "apiKey", "api_key", "access_token", "accessToken"] {
        if let Some(token) = data
            .get(field)
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|token| !token.is_empty())
        {
            return Ok(token.to_string());
        }
    }
    let fields: Vec<&str> = data
        .as_object()
        .map(|map| map.keys().map(String::as_str).collect())
        .unwrap_or_default();
    bail!(
        "the API sign-in returned no recognisable key; it carried: {}",
        fields.join(", ")
    );
}

fn store_key(token: &str) -> Result<std::path::PathBuf> {
    let path = crate::persist::global_env_path().context("no home directory")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    crate::init::set_env_key(&path, KEY_VAR, token)?;
    Ok(path)
}

fn adopt_coding_endpoint() -> Result<()> {
    let repo_root = std::env::current_dir().context("could not determine the current directory")?;
    let settings = crate::settings::Settings::load(Some(&repo_root))?;
    let (base_url, _) = crate::config::provider::resolve_endpoint(&settings.review, None);
    if base_url.trim_end_matches('/') == CODING_BASE_URL {
        return Ok(());
    }

    let (_, base_url, model) = crate::init::find_provider(CODING_BASE_URL)?;
    if !console::Term::stdout().features().is_attended() {
        println!("To use the plan, run `aster provider use zai_coding`.");
        return Ok(());
    }
    let switch = cliclack::confirm("Point Aster at the Z.ai coding endpoint now?")
        .initial_value(true)
        .interact()?;
    if !switch {
        println!("Left as is. Run `aster provider use zai_coding` when you want the plan.");
        return Ok(());
    }
    let saved = crate::settings::persist_user_review(
        Some(&repo_root),
        &[("base_url", &base_url), ("model", &model)],
    )?;
    crate::config::provider::report(&repo_root, &saved, &["ASTER_BASE_URL", "ASTER_MODEL"])
}

#[cfg(test)]
#[path = "zai_auth_tests.rs"]
mod tests;
