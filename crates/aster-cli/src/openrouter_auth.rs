//! "Sign in with OpenRouter": an authorization-code + PKCE login through the
//! browser with no client id or secret, exchanging the returned code for a
//! real API key stored as `OPEN_ROUTER_API_KEY` in `~/.aster/.env`.

use anyhow::{Context, Result};

use crate::mcp::oauth;

const AUTH_URL: &str = "https://openrouter.ai/auth";
const EXCHANGE_URL: &str = "https://openrouter.ai/api/v1/auth/keys";
pub(crate) const KEY_VAR: &str = "OPEN_ROUTER_API_KEY";

/// Run the whole browser sign-in and store the key. Returns the summary
/// printed by the CLI.
pub async fn login() -> Result<String> {
    let pkce = oauth::pkce();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .context("binding a local port for the login callback")?;
    let port = listener.local_addr()?.port();
    let callback_url = format!("http://127.0.0.1:{port}/callback");

    let mut url = reqwest::Url::parse(AUTH_URL).context("parsing the OpenRouter auth url")?;
    // Without a label OpenRouter names the key after the callback host, so the
    // user's key list would read "127.0.0.1:59115" instead of "Aster".
    url.query_pairs_mut()
        .append_pair("callback_url", &callback_url)
        .append_pair("code_challenge", &pkce.challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("key_label", "Aster");

    let json = crate::json_mode();
    let instructions = format!(
        "\nTo sign in with OpenRouter, finish in your browser:\n  {url}\n\nWaiting for authorization…"
    );
    if json {
        eprintln!("{instructions}");
    } else {
        println!("{instructions}");
    }
    let _ = open::that(url.as_str());

    let callback =
        tokio::time::timeout(oauth::CALLBACK_TIMEOUT, oauth::await_callback(listener)).await??;
    let key = exchange(&callback.code, &pkce.verifier).await?;
    let path = store_key(&key)?;
    // Reload so the key reaches this process too: without it the run that
    // offered the sign-in would still find nothing and fail anyway.
    let _ = dotenvy::from_path_override(&path);

    if json {
        println!(
            "{}",
            serde_json::json!({ "ok": true, "provider": "openrouter", "key_var": KEY_VAR })
        );
        return Ok(String::new());
    }
    Ok(format!(
        "Signed in. The key is stored as {KEY_VAR} in {}.",
        path.display()
    ))
}

/// Sign in and print the summary; the shared tail of every entry point.
pub async fn login_and_report() -> Result<()> {
    let summary = login().await?;
    if !summary.is_empty() {
        println!("{summary}");
    }
    Ok(())
}

#[derive(serde::Deserialize)]
struct ExchangeResponse {
    key: String,
}

async fn exchange(code: &str, verifier: &str) -> Result<String> {
    let http = reqwest::Client::builder()
        .user_agent("aster-cli")
        .build()
        .context("building the http client")?;
    let resp: ExchangeResponse = http
        .post(EXCHANGE_URL)
        .json(&serde_json::json!({
            "code": code,
            "code_verifier": verifier,
            "code_challenge_method": "S256",
        }))
        .send()
        .await
        .context("exchanging the sign-in code for a key")?
        .error_for_status()
        .context("OpenRouter refused the sign-in exchange")?
        .json()
        .await
        .context("decoding the exchange response")?;
    Ok(resp.key)
}

fn store_key(key: &str) -> Result<std::path::PathBuf> {
    let path = crate::persist::global_env_path().context("no home directory")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    crate::init::set_env_key(&path, KEY_VAR, key)?;
    Ok(path)
}

/// When a run dies for lack of an OpenRouter key on an attended terminal,
/// offer the browser sign-in once instead of failing outright. True when the
/// caller should retry resolution.
pub async fn offer_sign_in(error: &str) -> Result<bool> {
    if !error.contains("no API key") || !error.contains("OpenRouter") {
        return Ok(false);
    }
    if !console::Term::stdout().features().is_attended() {
        return Ok(false);
    }
    let yes = cliclack::confirm("No OpenRouter key found. Sign in with OpenRouter now?")
        .initial_value(true)
        .interact()?;
    if !yes {
        return Ok(false);
    }
    login_and_report().await?;
    Ok(true)
}

#[cfg(test)]
#[path = "openrouter_auth_tests.rs"]
mod tests;
