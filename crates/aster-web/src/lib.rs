#![forbid(unsafe_code)]
//! Web tools for Aster: crawl, extract, and search across pluggable providers.
//! [`WebBackend::from_env`] holds every provider whose key is set and dispatches
//! to the best one; [`register_tools`] is the catalog agents discover.

mod browserbase;
mod cloudflare_br;
mod config;
mod context_dev;
mod firecrawl;
mod jina;
mod plain_http;
mod types;

use aster_mcp::McpTool;
use async_trait::async_trait;

pub use config::WebConfig;
pub use types::{CrawlOptions, CrawlResult, ExtractedPage, PageMetadata};

/// Extract a single page as Markdown.
#[async_trait]
pub trait WebExtract: Send + Sync {
    async fn extract(&self, url: &str) -> anyhow::Result<ExtractedPage>;
}

/// Crawl a site starting from `url`, returning Markdown for every page.
#[async_trait]
pub trait WebCrawl: Send + Sync {
    async fn crawl(&self, url: &str, opts: &CrawlOptions) -> anyhow::Result<CrawlResult>;
}

/// Composite backend that holds every configured web provider. Methods dispatch
/// to the best available backend in priority order.
#[derive(Clone)]
pub struct WebBackend {
    context_dev: Option<context_dev::ContextDevClient>,
    firecrawl: Option<firecrawl::FirecrawlClient>,
    browserbase: Option<browserbase::BrowserbaseClient>,
    cloudflare: Option<cloudflare_br::CloudflareBrClient>,
    jina: jina::JinaClient,
    plain_http: plain_http::PlainHttpClient,
}

impl WebBackend {
    pub fn from_env(config: &WebConfig) -> Self {
        let timeout_ms = config.defaults.timeout_ms;
        Self {
            context_dev: config
                .resolve_context_dev_key()
                .map(|k| context_dev::ContextDevClient::new(k, timeout_ms)),
            firecrawl: config
                .resolve_firecrawl_key()
                .map(|k| firecrawl::FirecrawlClient::new(k, timeout_ms)),
            browserbase: config
                .resolve_browserbase_key()
                .map(|k| browserbase::BrowserbaseClient::new(k, timeout_ms)),
            cloudflare: config.resolve_cloudflare_br_keys().map(|(account, token)| {
                cloudflare_br::CloudflareBrClient::new(account, token, timeout_ms)
            }),
            jina: jina::JinaClient::new(config.resolve_jina_key(), timeout_ms),
            plain_http: plain_http::PlainHttpClient::new(timeout_ms),
        }
    }

    /// True when at least one API-backed provider is configured.
    pub fn is_api_backed(&self) -> bool {
        self.context_dev.is_some()
            || self.firecrawl.is_some()
            || self.browserbase.is_some()
            || self.cloudflare.is_some()
    }

    pub async fn extract(&self, url: &str) -> anyhow::Result<ExtractedPage> {
        if let Some(ref c) = self.context_dev {
            return c.extract(url).await;
        }
        if let Some(ref c) = self.firecrawl {
            return c.extract(url).await;
        }
        if let Some(ref c) = self.cloudflare {
            return c.extract(url).await;
        }
        if let Some(ref c) = self.browserbase {
            return c.fetch(url).await;
        }
        if let Ok(page) = self.jina.extract(url).await {
            return Ok(page);
        }
        self.plain_http.extract(url).await
    }

    pub async fn crawl(&self, url: &str, opts: &CrawlOptions) -> anyhow::Result<CrawlResult> {
        if let Some(ref c) = self.context_dev {
            return c.crawl(url, opts).await;
        }
        if let Some(ref c) = self.firecrawl {
            return c.crawl(url, opts).await;
        }
        if let Some(ref c) = self.cloudflare {
            return c.crawl(url, opts).await;
        }
        anyhow::bail!(
            "crawl requires a configured provider (set CONTEXT_DEV_API_KEY, FIRECRAWL_API_KEY, or CLOUDFLARE_BR_API_TOKEN)"
        )
    }

    pub async fn search(&self, query: &str, limit: u32) -> anyhow::Result<Vec<ExtractedPage>> {
        if let Some(ref c) = self.firecrawl {
            return c.search(query, limit).await;
        }
        if let Some(ref c) = self.browserbase {
            return c.search(query, limit).await;
        }
        anyhow::bail!(
            "search requires Firecrawl or Browserbase (set FIRECRAWL_API_KEY or BROWSERBASE_API_KEY)"
        )
    }
}

#[cfg(test)]
#[path = "tests/lib_test.rs"]
mod tests;

/// One tool per name, described by the provider that will serve it. Providers
/// overlap (five can offer `extract`), so candidates are gathered in dispatch
/// priority order and the first claim on each name wins.
pub fn register_tools(backend: &WebBackend) -> Vec<McpTool> {
    let mut candidates = Vec::new();
    if backend.context_dev.is_some() {
        candidates.push(context_dev::extract_tool());
        candidates.push(context_dev::crawl_tool());
    }
    if backend.firecrawl.is_some() {
        candidates.push(firecrawl::scrape_tool());
        candidates.push(firecrawl::crawl_tool());
        candidates.push(firecrawl::search_tool());
    }
    if backend.cloudflare.is_some() {
        candidates.push(cloudflare_br::extract_tool());
        candidates.push(cloudflare_br::crawl_tool());
    }
    if backend.browserbase.is_some() {
        candidates.push(browserbase::fetch_tool());
        candidates.push(browserbase::search_tool());
    }
    // Jina needs no key; always available as the default extract provider.
    candidates.push(jina::extract_tool());
    // Plain HTTP needs no key, so `extract` is always on offer.
    candidates.push(plain_http::extract_tool());

    let mut seen = Vec::new();
    candidates.retain(|tool| match seen.contains(&tool.name) {
        true => false,
        false => {
            seen.push(tool.name.clone());
            true
        }
    });
    candidates
}
