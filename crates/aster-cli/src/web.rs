//! `aster web` subcommands: one-shot crawl, extract, search, sitemap, and
//! screenshot tools using the configured provider.

use std::time::Instant;

use anyhow::Result;
use aster_web::{CrawlOptions, WebBackend, WebConfig};
use clap::Subcommand;

#[derive(clap::Args)]
pub struct WebArgs {
    #[command(subcommand)]
    pub action: WebAction,
}

#[derive(Subcommand)]
pub enum WebAction {
    /// Crawl a site starting from a URL and print Markdown for every page.
    Crawl(CrawlArgs),
    /// Extract a single page as Markdown.
    Extract(ExtractArgs),
    /// Search the web and print Markdown for every result.
    Search(SearchArgs),
    /// List a domain's sitemap URLs without scraping any pages.
    Sitemap(SitemapArgs),
    /// Capture a screenshot of a page and print the image URL.
    Screenshot(ScreenshotArgs),
}

#[derive(clap::Args)]
pub struct CrawlArgs {
    /// Starting URL, including https://.
    pub url: String,

    /// Maximum pages to crawl (1-500, default 100).
    #[arg(long, default_value = "100")]
    pub max_pages: u32,

    /// Link depth from the start URL. 0 = only the start page.
    #[arg(long)]
    pub max_depth: Option<u32>,

    /// Soft time budget in milliseconds (default 80000).
    #[arg(long, default_value = "80000")]
    pub stop_after_ms: u32,

    /// Follow subdomain links.
    #[arg(long)]
    pub follow_subdomains: bool,

    /// Skip PDF parsing.
    #[arg(long)]
    pub no_pdfs: bool,

    /// Only scrape URLs matching this regex.
    #[arg(long)]
    pub url_regex: Option<String>,

    /// Strip headers, footers, nav, and sidebars.
    #[arg(long)]
    pub main_content_only: bool,
}

#[derive(clap::Args)]
pub struct ExtractArgs {
    /// URL of the page, including https://.
    pub url: String,
}

#[derive(clap::Args)]
pub struct SearchArgs {
    /// Search query.
    pub query: String,

    /// Maximum number of results (default 5).
    #[arg(long, default_value = "5")]
    pub limit: u32,

    /// Region/language code such as us-en or de-de. Keyless search only.
    #[arg(long)]
    pub region: Option<String>,

    /// Adult-content filter: strict, moderate, or off. Keyless search only.
    #[arg(long)]
    pub safesearch: Option<String>,
}

#[derive(clap::Args)]
pub struct SitemapArgs {
    /// Domain without protocol, e.g. docs.rs.
    pub domain: String,

    /// Maximum URLs to return (default 500).
    #[arg(long, default_value = "500")]
    pub max_links: u32,

    /// Only return URLs matching this regex.
    #[arg(long)]
    pub url_regex: Option<String>,
}

#[derive(clap::Args)]
pub struct ScreenshotArgs {
    /// URL of the page, including https://.
    pub url: String,

    /// Capture the full scrollable height instead of one viewport.
    #[arg(long)]
    pub full_page: bool,
}

struct Status {
    spinner: Option<cliclack::ProgressBar>,
    started: Instant,
}

impl Status {
    fn begin(message: String) -> Self {
        let spinner = crate::picker::is_tty().then(|| {
            let s = cliclack::spinner();
            s.start(&message);
            s
        });
        if spinner.is_none() && !crate::json_mode() {
            eprintln!("{message}…");
        }
        Self {
            spinner,
            started: Instant::now(),
        }
    }

    fn end<T>(self, result: Result<T>, summary: impl FnOnce(&T) -> String) -> Result<T> {
        let took = self.started.elapsed();
        // Tenths below a minute, where a search's speed is the interesting part;
        // minutes past it, so a slow run does not read as a part number.
        let elapsed = match took.as_secs() >= 60 {
            true => crate::util::elapsed(took.as_secs()),
            false => format!("{:.1}s", took.as_secs_f32()),
        };
        match (&result, self.spinner) {
            (Ok(v), Some(s)) => s.stop(format!("{} in {elapsed}", summary(v))),
            (Ok(v), None) if !crate::json_mode() => {
                eprintln!("{} in {elapsed}", summary(v));
            }
            (Err(_), Some(s)) => s.error("failed"),
            _ => {}
        }
        result
    }
}

pub async fn run(args: WebArgs) -> Result<()> {
    let config = WebConfig::from_env();
    let backend = WebBackend::from_env(&config);

    match args.action {
        WebAction::Crawl(a) => {
            let opts = CrawlOptions {
                max_pages: a.max_pages,
                max_depth: a.max_depth,
                stop_after_ms: a.stop_after_ms,
                follow_subdomains: a.follow_subdomains,
                parse_pdfs: !a.no_pdfs,
                url_regex: a.url_regex,
                use_main_content_only: a.main_content_only,
                ..Default::default()
            };

            let status = Status::begin(format!(
                "Crawling {} (up to {} pages, {}s budget)",
                a.url,
                a.max_pages,
                a.stop_after_ms / 1000
            ));
            let result = status.end(backend.crawl(&a.url, &opts).await, |r| {
                format!("Crawled {} pages", r.num_urls)
            })?;
            if crate::json_mode() {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!(
                    "Crawled {} pages ({} succeeded, {} failed, depth {})",
                    result.num_urls,
                    result.num_succeeded,
                    result.num_failed,
                    result.max_crawl_depth
                );
                if let Some(c) = result.credits_consumed {
                    println!("Credits used: {c}");
                }
                println!();
                for page in &result.pages {
                    println!("═══ {} ═══", page.metadata.url);
                    if let Some(ref title) = page.metadata.title {
                        println!("  Title: {title}");
                    }
                    println!();
                    println!("{}", page.markdown);
                    println!();
                }
            }
        }
        WebAction::Extract(a) => {
            let status = Status::begin(format!("Reading {}", a.url));
            let page = status.end(backend.extract(&a.url).await, |p| {
                format!("Read {}", p.metadata.url)
            })?;
            if crate::json_mode() {
                println!("{}", serde_json::to_string_pretty(&page)?);
            } else {
                println!("{}", page.markdown);
            }
        }
        WebAction::Search(a) => {
            let status = Status::begin(format!("Searching \"{}\"", a.query));
            let opts = aster_web::SearchOptions {
                limit: a.limit,
                region: a.region.clone(),
                safesearch: a.safesearch.clone(),
            };
            let results = status.end(backend.search(&a.query, &opts).await, |r| {
                format!("{} results", r.len())
            })?;
            if crate::json_mode() {
                println!("{}", serde_json::to_string_pretty(&results)?);
            } else {
                for page in &results {
                    println!("═══ {} ═══", page.metadata.url);
                    if let Some(ref title) = page.metadata.title {
                        println!("  Title: {title}");
                    }
                    println!();
                    println!("{}", page.markdown);
                    println!();
                }
            }
        }
        WebAction::Sitemap(a) => {
            let status = Status::begin(format!("Listing sitemap for {}", a.domain));
            let result = status.end(
                backend
                    .sitemap(&a.domain, a.max_links, a.url_regex.as_deref())
                    .await,
                |r| format!("{} URLs", r.urls.len()),
            )?;
            if crate::json_mode() {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("{} URLs on {}", result.urls.len(), result.domain);
                for url in &result.urls {
                    println!("{url}");
                }
            }
        }
        WebAction::Screenshot(a) => {
            let status = Status::begin(format!("Capturing {}", a.url));
            let shot = status.end(backend.screenshot(&a.url, a.full_page).await, |_| {
                format!("Captured {}", a.url)
            })?;
            if crate::json_mode() {
                println!("{}", serde_json::to_string_pretty(&shot)?);
            } else {
                println!("{}", shot.url);
            }
        }
    }
    Ok(())
}
