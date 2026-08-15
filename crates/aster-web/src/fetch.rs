//! Page fetching for `fetch_content`. Refuses non-public hosts so a tool the
//! model can point anywhere cannot be turned into a probe of the loopback
//! interface or the local network.

use std::net::IpAddr;
use std::time::Duration;

use anyhow::{Context, Result, bail};

use crate::html;

/// Longest page text returned. Past this a page is reference material the model
/// should search rather than read whole.
pub const MAX_CHARS: usize = 8_000;

pub async fn fetch(client: &reqwest::Client, url: &str, allow_private: bool) -> Result<String> {
    let parsed = reqwest::Url::parse(url).context("parsing url")?;
    if !matches!(parsed.scheme(), "http" | "https") {
        bail!(
            "only http and https urls can be fetched, not `{}`",
            parsed.scheme()
        );
    }
    if !allow_private {
        reject_private_host(&parsed).await?;
    }

    let res = client
        .get(parsed.clone())
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .with_context(|| format!("fetching {parsed}"))?;

    let status = res.status();
    if !status.is_success() {
        bail!("{parsed} returned HTTP {status}");
    }

    let body = res.text().await.context("reading page body")?;
    let text = html::to_document(&body);
    if text.is_empty() {
        bail!("{parsed} has no readable text");
    }
    Ok(truncate(text))
}

fn truncate(mut text: String) -> String {
    if text.chars().count() <= MAX_CHARS {
        return text;
    }
    let cut = text
        .char_indices()
        .nth(MAX_CHARS)
        .map(|(i, _)| i)
        .unwrap_or(text.len());
    text.truncate(cut);
    text.push_str("\n\n[truncated]");
    text
}

/// Resolve the host and refuse loopback, private, and link-local addresses.
/// Resolution happens again inside the request, so this is a guard rather than
/// a guarantee; it stops the cases that matter without a custom DNS resolver.
async fn reject_private_host(url: &reqwest::Url) -> Result<()> {
    let host = url.host_str().context("url has no host")?;
    let port = url.port_or_known_default().unwrap_or(443);
    let addrs = tokio::net::lookup_host((host, port))
        .await
        .with_context(|| format!("resolving {host}"))?;

    let mut any = false;
    for addr in addrs {
        any = true;
        if !is_public(&addr.ip()) {
            bail!(
                "refusing to fetch {host}: it resolves to the non-public address {}",
                addr.ip()
            );
        }
    }
    if !any {
        bail!("{host} resolved to no addresses");
    }
    Ok(())
}

fn is_public(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_unspecified()
                // Carrier-grade NAT and the cloud metadata range live here.
                || v4.octets()[0] == 100 && (64..128).contains(&v4.octets()[1]))
        }
        IpAddr::V6(v6) => {
            !(v6.is_loopback()
                || v6.is_unspecified()
                // Unique-local (fc00::/7) and link-local (fe80::/10).
                || (v6.segments()[0] & 0xfe00) == 0xfc00
                || (v6.segments()[0] & 0xffc0) == 0xfe80)
        }
    }
}

#[cfg(test)]
#[path = "tests/fetch_test.rs"]
mod tests;
