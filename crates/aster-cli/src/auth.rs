use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::config;

const DEVICE_CODE_URL: &str = "https://github.com/login/device/code";
const ACCESS_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
const SCOPE: &str = "repo";

#[derive(Deserialize)]
struct DeviceCode {
    device_code: String,
    user_code: String,
    verification_uri: String,
    interval: u64,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    error: Option<String>,
}

pub async fn login() -> Result<()> {
    if config::APP_CLIENT_ID.starts_with("REPLACE_WITH") {
        bail!(
            "This build has no GitHub App client id wired in yet.\n\
             Use a token instead for now: `export GITHUB_TOKEN=…` or `aster review --pr N --token …`."
        );
    }

    let http = reqwest::Client::builder().user_agent("aster-cli").build()?;

    let device: DeviceCode = http
        .post(DEVICE_CODE_URL)
        .header("Accept", "application/json")
        .form(&[("client_id", config::APP_CLIENT_ID), ("scope", SCOPE)])
        .send()
        .await
        .context("requesting device code")?
        .error_for_status()?
        .json()
        .await
        .context("parsing device code response")?;

    // Under --json stdout is the caller's channel, so the human instructions
    // go to stderr and stdout carries one object at the end.
    let json = crate::json_mode();
    let instructions = format!(
        "\nTo link your GitHub account, open:\n  {}\nand enter the code:\n\n    {}\n\nWaiting for authorization…",
        device.verification_uri, device.user_code
    );
    if json {
        eprintln!("{instructions}");
    } else {
        println!("{instructions}");
    }
    let _ = open::that(&device.verification_uri);

    let token = poll_for_token(&http, &device).await?;
    config::store_token(&token)?;

    if json {
        println!(
            "{}",
            serde_json::json!({
                "ok": true,
                "linked": true,
                "install_url": config::APP_INSTALL_URL,
            })
        );
    } else {
        println!("\n✓ Linked. Credentials stored.");
        println!(
            "If you haven't yet, install the Aster app on your repos:\n  {}",
            config::APP_INSTALL_URL
        );
    }
    Ok(())
}

async fn poll_for_token(http: &reqwest::Client, device: &DeviceCode) -> Result<String> {
    let mut interval = device.interval.max(1);
    loop {
        tokio::time::sleep(Duration::from_secs(interval)).await;

        let resp: TokenResponse = http
            .post(ACCESS_TOKEN_URL)
            .header("Accept", "application/json")
            .form(&[
                ("client_id", config::APP_CLIENT_ID),
                ("device_code", device.device_code.as_str()),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ])
            .send()
            .await
            .context("polling for access token")?
            .json()
            .await
            .context("parsing token response")?;

        if let Some(token) = resp.access_token {
            return Ok(token);
        }
        match resp.error.as_deref() {
            Some("authorization_pending") => continue,
            Some("slow_down") => interval += 5,
            Some("expired_token") => {
                bail!("the code expired before you authorized; run `aster login` again")
            }
            Some("access_denied") => bail!("authorization was denied"),
            Some(other) => bail!("device flow failed: {other}"),
            None => bail!("device flow returned no token and no error"),
        }
    }
}
