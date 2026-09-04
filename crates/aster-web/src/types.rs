use serde::{Deserialize, Serialize};

/// One page extracted from a crawl or single-page fetch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedPage {
    pub markdown: String,
    pub metadata: PageMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageMetadata {
    pub url: String,
    pub title: Option<String>,
    pub crawl_depth: Option<u32>,
    pub status_code: Option<u16>,
    pub success: bool,
    pub description: Option<String>,
    pub language: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlOptions {
    #[serde(default = "default_max_pages")]
    pub max_pages: u32,
    #[serde(default)]
    pub max_depth: Option<u32>,
    #[serde(default = "default_stop_after_ms")]
    pub stop_after_ms: u32,
    #[serde(default)]
    pub follow_subdomains: bool,
    #[serde(default = "default_true")]
    pub parse_pdfs: bool,
    #[serde(default)]
    pub url_regex: Option<String>,
    #[serde(default)]
    pub use_main_content_only: bool,
    #[serde(default = "default_true")]
    pub include_links: bool,
    #[serde(default)]
    pub timeout_ms: u64,
}

fn default_max_pages() -> u32 {
    100
}
fn default_stop_after_ms() -> u32 {
    80000
}
fn default_true() -> bool {
    true
}

impl Default for CrawlOptions {
    fn default() -> Self {
        Self {
            max_pages: default_max_pages(),
            max_depth: None,
            stop_after_ms: default_stop_after_ms(),
            follow_subdomains: false,
            parse_pdfs: true,
            url_regex: None,
            use_main_content_only: false,
            include_links: true,
            timeout_ms: 120_000,
        }
    }
}

/// Parameters that control a search. Only the keyless DuckDuckGo provider
/// honours `region` and `safesearch`; the API-backed ones take `limit` alone.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchOptions {
    #[serde(default = "default_limit")]
    pub limit: u32,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub safesearch: Option<String>,
}

fn default_limit() -> u32 {
    10
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            limit: default_limit(),
            region: None,
            safesearch: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlResult {
    pub pages: Vec<ExtractedPage>,
    pub num_urls: u32,
    pub num_succeeded: u32,
    pub num_failed: u32,
    pub max_crawl_depth: u32,
    pub credits_consumed: Option<u32>,
}

/// URLs discovered from a domain's sitemap.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SitemapResult {
    pub domain: String,
    pub urls: Vec<String>,
}

/// A rendered-page screenshot hosted on the provider's CDN.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Screenshot {
    pub url: String,
    pub screenshot_type: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}
