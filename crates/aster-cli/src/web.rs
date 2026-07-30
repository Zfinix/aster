//! `aster web crawl` and `aster web extract`: one-shot web tools using the
//! configured provider.

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

            // A crawl is one long provider call with nothing to show until it
            // returns, so say what it is doing rather than look hung.
            let started = std::time::Instant::now();
            if !crate::json_mode() {
                eprintln!(
                    "Crawling {} (up to {} pages, {}s budget)…",
                    a.url,
                    a.max_pages,
                    a.stop_after_ms / 1000
                );
            }
            let result = backend.crawl(&a.url, &opts).await?;
            if !crate::json_mode() {
                eprintln!("Done in {:.1}s", started.elapsed().as_secs_f32());
            }
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
            if !crate::json_mode() {
                eprintln!("Reading {}…", a.url);
            }
            let page = backend.extract(&a.url).await?;
            if crate::json_mode() {
                println!("{}", serde_json::to_string_pretty(&page)?);
            } else {
                println!("{}", page.markdown);
            }
        }
    }
    Ok(())
}
