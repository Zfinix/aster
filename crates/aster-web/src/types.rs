use serde::{Deserialize, Serialize};

/// One page extracted from a crawl or single-page fetch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedPage {
    pub markdown: String,
    pub metadata: PageMetadata,
}

/// Metadata for a single crawled page.
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

/// Parameters that control a crawl.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlOptions {
    /// Maximum pages to crawl (1-500).
    #[serde(default = "default_max_pages")]
    pub max_pages: u32,
    /// Link depth from the start URL. None = unbounded.
    #[serde(default)]
    pub max_depth: Option<u32>,
    /// Soft time budget for the entire crawl, in milliseconds.
    #[serde(default = "default_stop_after_ms")]
    pub stop_after_ms: u32,
    /// Follow subdomain links of the start domain.
    #[serde(default)]
    pub follow_subdomains: bool,
    /// Whether to parse PDFs encountered during the crawl.
    #[serde(default = "default_true")]
    pub parse_pdfs: bool,
    /// Regex filter for URLs to follow and scrape.
    #[serde(default)]
    pub url_regex: Option<String>,
    /// Strip headers, footers, nav, sidebars.
    #[serde(default)]
    pub use_main_content_only: bool,
    /// Preserve hyperlinks in the Markdown output.
    #[serde(default = "default_true")]
    pub include_links: bool,
    /// Request timeout in milliseconds.
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

/// The result of a full crawl.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlResult {
    pub pages: Vec<ExtractedPage>,
    pub num_urls: u32,
    pub num_succeeded: u32,
    pub num_failed: u32,
    pub max_crawl_depth: u32,
    pub credits_consumed: Option<u32>,
}
