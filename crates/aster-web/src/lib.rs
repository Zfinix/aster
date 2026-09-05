#![forbid(unsafe_code)]
//! Web tools for Aster: crawl, extract, search, sitemap, and screenshot across
//! pluggable providers. [`WebBackend::from_env`] holds every provider whose key is
//! set and dispatches to the best; [`register_tools`] is the catalog agents see.

mod browserbase;
mod cloudflare_br;
mod config;
mod context_dev;
mod duckduckgo;
mod exa;
mod fetch;
mod firecrawl;
mod html;
mod jina;
mod limit;
mod perplexity;
mod plain_http;
mod serve;
mod types;

use aster_mcp::McpTool;
use async_trait::async_trait;
use serde_json::Value;

pub use config::{KEY_VARS, WebConfig};
pub use serve::serve;
pub use types::{
    CrawlOptions, CrawlResult, ExtractedPage, PageMetadata, Screenshot, SearchOptions,
    SitemapResult,
};

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

type Attempt<'a, T> = (
    &'static str,
    std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<T>> + Send + 'a>>,
);

async fn first_success<T>(what: &str, attempts: Vec<Attempt<'_, T>>) -> anyhow::Result<T> {
    let mut failures = Vec::new();
    for (provider, attempt) in attempts {
        match attempt.await {
            Ok(value) => return Ok(value),
            Err(err) => {
                tracing::warn!(
                    provider,
                    "web {what} provider failed, trying the next: {err:#}"
                );
                failures.push(format!("{provider}: {err:#}"));
            }
        }
    }
    anyhow::bail!("every {what} provider failed:\n  {}", failures.join("\n  "))
}

/// Composite backend that holds every configured web provider. Methods try
/// providers in priority order and fall through to the next one on failure.
#[derive(Clone)]
pub struct WebBackend {
    exa: Option<exa::ExaClient>,
    perplexity: Option<perplexity::PerplexityClient>,
    context_dev: Option<context_dev::ContextDevClient>,
    firecrawl: firecrawl::FirecrawlClient,
    browserbase: Option<browserbase::BrowserbaseClient>,
    cloudflare: Option<cloudflare_br::CloudflareBrClient>,
    jina: jina::JinaClient,
    duckduckgo: std::sync::Arc<duckduckgo::DuckDuckGoClient>,
    plain_http: plain_http::PlainHttpClient,
}

impl WebBackend {
    pub fn from_env(config: &WebConfig) -> Self {
        let timeout_ms = config.defaults.timeout_ms;
        Self {
            exa: config
                .resolve_exa_key()
                .map(|k| exa::ExaClient::new(k, timeout_ms)),
            perplexity: config
                .resolve_perplexity_key()
                .map(|k| perplexity::PerplexityClient::new(k, timeout_ms)),
            context_dev: config
                .resolve_context_dev_key()
                .map(|k| context_dev::ContextDevClient::new(k, timeout_ms)),
            firecrawl: firecrawl::FirecrawlClient::new(config.resolve_firecrawl_key(), timeout_ms),
            browserbase: config
                .resolve_browserbase_key()
                .map(|k| browserbase::BrowserbaseClient::new(k, timeout_ms)),
            cloudflare: config.resolve_cloudflare_br_keys().map(|(account, token)| {
                cloudflare_br::CloudflareBrClient::new(account, token, timeout_ms)
            }),
            jina: jina::JinaClient::new(config.resolve_jina_key(), timeout_ms),
            duckduckgo: std::sync::Arc::new(duckduckgo::DuckDuckGoClient::new()),
            plain_http: plain_http::PlainHttpClient::new(timeout_ms),
        }
    }

    pub fn is_api_backed(&self) -> bool {
        self.exa.is_some()
            || self.perplexity.is_some()
            || self.context_dev.is_some()
            || self.firecrawl.is_keyed()
            || self.browserbase.is_some()
            || self.cloudflare.is_some()
    }

    pub async fn extract(&self, url: &str) -> anyhow::Result<ExtractedPage> {
        let mut attempts: Vec<Attempt<'_, ExtractedPage>> = Vec::new();
        if let Some(ref c) = self.context_dev {
            attempts.push(("Context.dev", Box::pin(c.extract(url))));
        }
        if self.firecrawl.is_keyed() {
            attempts.push(("Firecrawl", Box::pin(self.firecrawl.extract(url))));
        }
        if let Some(ref c) = self.cloudflare {
            attempts.push(("Cloudflare", Box::pin(c.extract(url))));
        }
        if let Some(ref c) = self.browserbase {
            attempts.push(("Browserbase", Box::pin(c.fetch(url))));
        }
        if let Some(ref c) = self.exa {
            attempts.push(("Exa", Box::pin(c.extract(url))));
        }
        if !self.firecrawl.is_keyed() {
            attempts.push(("Firecrawl (keyless)", Box::pin(self.firecrawl.extract(url))));
        }
        attempts.push(("Jina", Box::pin(self.jina.extract(url))));
        attempts.push(("DuckDuckGo", Box::pin(self.duckduckgo.extract(url))));
        attempts.push(("plain HTTP", Box::pin(self.plain_http.extract(url))));
        first_success("extract", attempts).await
    }

    pub async fn crawl(&self, url: &str, opts: &CrawlOptions) -> anyhow::Result<CrawlResult> {
        let mut attempts: Vec<Attempt<'_, CrawlResult>> = Vec::new();
        if let Some(ref c) = self.context_dev {
            attempts.push(("Context.dev", Box::pin(c.crawl(url, opts))));
        }
        if self.firecrawl.is_keyed() {
            attempts.push(("Firecrawl", Box::pin(self.firecrawl.crawl(url, opts))));
        }
        if let Some(ref c) = self.cloudflare {
            attempts.push(("Cloudflare", Box::pin(c.crawl(url, opts))));
        }
        if attempts.is_empty() {
            anyhow::bail!(
                "crawl requires a configured provider (set CONTEXT_DEV_API_KEY, FIRECRAWL_API_KEY, or both CLOUDFLARE_BR_ACCOUNT_ID and CLOUDFLARE_BR_API_TOKEN)"
            )
        }
        first_success("crawl", attempts).await
    }

    pub async fn search(
        &self,
        query: &str,
        opts: &SearchOptions,
    ) -> anyhow::Result<Vec<ExtractedPage>> {
        let mut attempts: Vec<Attempt<'_, Vec<ExtractedPage>>> = Vec::new();
        if let Some(ref c) = self.exa {
            attempts.push(("Exa", Box::pin(c.search(query, opts))));
        }
        if let Some(ref c) = self.perplexity {
            attempts.push(("Perplexity", Box::pin(c.search(query, opts))));
        }
        if let Some(ref c) = self.context_dev {
            attempts.push(("Context.dev", Box::pin(c.search(query, opts.limit))));
        }
        if self.firecrawl.is_keyed() {
            attempts.push((
                "Firecrawl",
                Box::pin(self.firecrawl.search(query, opts.limit)),
            ));
        }
        if let Some(ref c) = self.browserbase {
            attempts.push(("Browserbase", Box::pin(c.search(query, opts.limit))));
        }
        if !self.firecrawl.is_keyed() {
            attempts.push((
                "Firecrawl (keyless)",
                Box::pin(self.firecrawl.search(query, opts.limit)),
            ));
        }
        attempts.push(("DuckDuckGo", Box::pin(self.duckduckgo.search(query, opts))));
        first_success("search", attempts).await
    }

    pub async fn sitemap(
        &self,
        domain: &str,
        max_links: u32,
        url_regex: Option<&str>,
    ) -> anyhow::Result<SitemapResult> {
        if let Some(ref c) = self.context_dev {
            return c.sitemap(domain, max_links, url_regex).await;
        }
        anyhow::bail!("sitemap requires Context.dev (set CONTEXT_DEV_API_KEY)")
    }

    pub async fn screenshot(&self, url: &str, full_page: bool) -> anyhow::Result<Screenshot> {
        if let Some(ref c) = self.context_dev {
            return c.screenshot(url, full_page).await;
        }
        anyhow::bail!("screenshot requires Context.dev (set CONTEXT_DEV_API_KEY)")
    }

    /// A screenshot as MCP content parts: the image itself, so the model can
    /// look at it, beside the link. A provider link that cannot be fetched
    /// falls back to the link alone.
    async fn screenshot_content(&self, page: &str, shot: Screenshot) -> Value {
        use base64::Engine;
        let size = match (shot.width, shot.height) {
            (Some(w), Some(h)) => format!(" ({w}x{h})"),
            _ => String::new(),
        };
        let caption = format!("Screenshot of {page}{size}: {}", shot.url);
        match self.plain_http.fetch_bytes(&shot.url).await {
            Ok((bytes, mime)) if mime.starts_with("image/") => serde_json::json!({
                "content": [
                    { "type": "text", "text": caption },
                    {
                        "type": "image",
                        "data": base64::engine::general_purpose::STANDARD.encode(bytes),
                        "mimeType": mime,
                    }
                ]
            }),
            Ok((_, mime)) => {
                tracing::warn!(mime, "screenshot link did not return an image");
                serde_json::to_value(shot).unwrap_or(Value::Null)
            }
            Err(err) => {
                tracing::warn!("screenshot link could not be fetched: {err:#}");
                serde_json::to_value(shot).unwrap_or(Value::Null)
            }
        }
    }

    /// Run one tool by its bare name. The single dispatch table, shared by the
    /// in-process server the agent uses and the stdio one `aster mcp serve`
    /// exposes, so the two can never drift.
    pub async fn call(&self, tool: &str, arguments: &serde_json::Value) -> anyhow::Result<Value> {
        use anyhow::Context;
        match tool {
            "extract" => {
                let url = arguments["url"].as_str().context("missing url")?;
                Ok(serde_json::to_value(self.extract(url).await?)?)
            }
            "search" => {
                let query = arguments["query"].as_str().context("missing query")?;
                let opts = SearchOptions {
                    limit: arguments["limit"].as_u64().unwrap_or(10) as u32,
                    region: arguments["region"].as_str().map(str::to_string),
                    safesearch: arguments["safesearch"].as_str().map(str::to_string),
                };
                Ok(serde_json::to_value(self.search(query, &opts).await?)?)
            }
            "screenshot" => {
                let url = arguments["url"].as_str().context("missing url")?;
                let full_page = arguments["full_page"].as_bool().unwrap_or(false);
                let shot = self.screenshot(url, full_page).await?;
                Ok(self.screenshot_content(url, shot).await)
            }
            "sitemap" => {
                // The schema says `domain`; `url` is accepted so an older
                // caller keeps working.
                let domain = arguments["domain"]
                    .as_str()
                    .or_else(|| arguments["url"].as_str())
                    .context("missing domain")?;
                let max_links = arguments["max_links"].as_u64().unwrap_or(100) as u32;
                let url_regex = arguments["url_regex"].as_str();
                Ok(serde_json::to_value(
                    self.sitemap(domain, max_links, url_regex).await?,
                )?)
            }
            "crawl" => {
                let url = arguments["url"].as_str().context("missing url")?;
                let opts: CrawlOptions = serde_json::from_value(arguments.clone())
                    .context("crawl options did not match the schema")?;
                Ok(serde_json::to_value(self.crawl(url, &opts).await?)?)
            }
            other => anyhow::bail!("unknown web tool: {other}"),
        }
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
    // Exa leads `search` because that is what it is for, and its `extract` is
    // offered further down, where the extraction-first providers rank above it.
    // Perplexity is the next search-first provider behind Exa.
    if backend.exa.is_some() {
        candidates.push(exa::search_tool());
    }
    if backend.perplexity.is_some() {
        candidates.push(perplexity::search_tool());
    }
    if backend.context_dev.is_some() {
        candidates.push(context_dev::extract_tool());
        candidates.push(context_dev::crawl_tool());
        candidates.push(context_dev::search_tool());
        candidates.push(context_dev::sitemap_tool());
        candidates.push(context_dev::screenshot_tool());
    }
    if backend.firecrawl.is_keyed() {
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
    if backend.exa.is_some() {
        candidates.push(exa::extract_tool());
    }
    if !backend.firecrawl.is_keyed() {
        candidates.push(firecrawl::scrape_tool());
        candidates.push(firecrawl::search_tool());
    }
    // Jina needs no key; always available as the default extract provider.
    candidates.push(jina::extract_tool());
    // DuckDuckGo needs no key either, and backs `search` when keyless Firecrawl
    // is rate-limited.
    candidates.push(duckduckgo::search_tool());
    candidates.push(duckduckgo::extract_tool());
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
