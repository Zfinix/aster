use std::time::Duration;

use anyhow::{Context, Result};
use aster_ai::retry::RetryWithBackoff;
use aster_models::Finding;
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use serde_json::json;

const API: &str = "https://api.github.com";
const API_VERSION: &str = "2022-11-28";
const TIMEOUT: Duration = Duration::from_secs(30);
const MAX_RETRIES: u32 = 3;
const RETRY_DEADLINE: Duration = Duration::from_secs(120);

fn client(token: &str) -> Result<ClientWithMiddleware> {
    use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue, USER_AGENT};

    let mut headers = HeaderMap::new();

    headers.insert(USER_AGENT, HeaderValue::from_static("aster-cli"));
    headers.insert(
        "X-GitHub-Api-Version",
        HeaderValue::from_static(API_VERSION),
    );

    let mut auth = HeaderValue::from_str(&format!("Bearer {token}"))?;

    auth.set_sensitive(true);
    headers.insert(AUTHORIZATION, auth);

    let inner = reqwest::Client::builder()
        .default_headers(headers)
        .timeout(TIMEOUT)
        .build()?;

    Ok(ClientBuilder::new(inner)
        .with(RetryWithBackoff::new(MAX_RETRIES, RETRY_DEADLINE))
        .build())
}

pub async fn fetch_pr_diff(owner: &str, repo: &str, pr: u64, token: &str) -> Result<String> {
    let client = client(token)?;
    let url = format!("{API}/repos/{owner}/{repo}/pulls/{pr}");
    let resp = client
        .get(&url)
        .header("Accept", "application/vnd.github.v3.diff")
        .send()
        .await
        .context("sending GitHub request")?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("GitHub returned {status} fetching PR #{pr}: {body}");
    }
    resp.text().await.context("reading PR diff body")
}

pub async fn post_review(
    owner: &str,
    repo: &str,
    pr: u64,
    token: &str,
    findings: &[Finding],
) -> Result<()> {
    let client = client(token)?;
    let url = format!("{API}/repos/{owner}/{repo}/pulls/{pr}/reviews");

    let comments: Vec<_> = findings.iter().map(inline_comment).collect();
    let body = json!({
        "event": "COMMENT",
        "body": review_summary(findings),
        "comments": comments,
    });

    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .context("posting review")?;
    let status = resp.status();
    if status.is_success() {
        return Ok(());
    }

    if status == reqwest::StatusCode::UNPROCESSABLE_ENTITY {
        let fallback = json!({
            "event": "COMMENT",
            "body": findings_markdown(findings),
        });
        let resp = client
            .post(&url)
            .json(&fallback)
            .send()
            .await
            .context("posting fallback review")?;
        if !resp.status().is_success() {
            let s = resp.status();
            let msg = resp.text().await.unwrap_or_default();
            anyhow::bail!("GitHub returned {s} posting fallback summary review: {msg}");
        }
        return Ok(());
    }

    let msg = resp.text().await.unwrap_or_default();
    anyhow::bail!("GitHub returned {status} posting review: {msg}");
}

fn inline_comment(f: &Finding) -> serde_json::Value {
    let side = f.side.as_deref().unwrap_or("right").to_uppercase();
    match f.start_line {
        Some(start) if start < f.line => json!({
            "path": f.file_path,
            "start_line": start,
            "start_side": side,
            "line": f.line,
            "side": side,
            "body": comment_body(f),
        }),
        _ => json!({
            "path": f.file_path,
            "line": f.line,
            "side": side,
            "body": comment_body(f),
        }),
    }
}

fn comment_body(f: &Finding) -> String {
    let conf = f
        .confidence
        .map(|c| format!(" · {:.0}% confident", c * 100.0))
        .unwrap_or_default();
    format!(
        "**{}** — {}/{}{}\n\n{}\n\n**Fix:** {}",
        f.title, f.severity, f.category, conf, f.description, f.suggestion
    )
}

fn review_summary(findings: &[Finding]) -> String {
    if findings.is_empty() {
        return "Aster reviewed this PR and found no issues that survived verification.".into();
    }
    format!(
        "Aster reviewed this PR: {} finding(s) survived adversarial verification.",
        findings.len()
    )
}

fn findings_markdown(findings: &[Finding]) -> String {
    let mut out = review_summary(findings);
    out.push_str(
        "\n\n_Some findings referenced lines outside the diff, so they are listed here instead of as inline comments._\n",
    );
    for f in findings {
        let conf = f
            .confidence
            .map(|c| format!(" · {:.0}% confident", c * 100.0))
            .unwrap_or_default();
        out.push_str(&format!(
            "\n---\n\n### {} — `{}:{}`\n\n**Severity:** {} / {}{}\n\n{}\n\n**Fix:** {}\n",
            f.title, f.file_path, f.line, f.severity, f.category, conf, f.description, f.suggestion
        ));
    }
    out
}
