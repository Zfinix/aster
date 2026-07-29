#![cfg(test)]

use super::*;

#[test]
fn crawl_options_defaults() {
    let opts = CrawlOptions::default();
    assert_eq!(opts.max_pages, 100);
    assert_eq!(opts.max_depth, None);
    assert_eq!(opts.stop_after_ms, 80000);
    assert!(!opts.follow_subdomains);
    assert!(opts.parse_pdfs);
    assert!(opts.include_links);
}

#[test]
fn crawl_options_serde_roundtrip() {
    let json = serde_json::json!({
        "max_pages": 50,
        "max_depth": 2,
        "stop_after_ms": 30000,
        "follow_subdomains": true,
        "parse_pdfs": false,
        "url_regex": "^/docs/",
        "use_main_content_only": true,
        "include_links": false,
        "timeout_ms": 60000
    });
    let opts: CrawlOptions = serde_json::from_value(json.clone()).expect("deserialize");
    let back = serde_json::to_value(&opts).expect("serialize");
    assert_eq!(back["max_pages"], json["max_pages"]);
    assert_eq!(back["max_depth"], json["max_depth"]);
    assert_eq!(back["stop_after_ms"], json["stop_after_ms"]);
}

#[test]
fn empty_result_serializes() {
    let result = CrawlResult {
        pages: vec![],
        num_urls: 0,
        num_succeeded: 0,
        num_failed: 0,
        max_crawl_depth: 0,
        credits_consumed: None,
    };
    let json = serde_json::to_string(&result).expect("serialize");
    assert!(json.contains("\"num_urls\":0"));
}

/// Which providers a backend has keys for. Building from this rather than from
/// the environment keeps the dispatch tests independent of each other.
#[derive(Default)]
struct Configured {
    context_dev: bool,
    firecrawl: bool,
    browserbase: bool,
    cloudflare: bool,
}

fn backend(with: Configured) -> WebBackend {
    let timeout = WebConfig::default().defaults.timeout_ms;
    WebBackend {
        context_dev: with
            .context_dev
            .then(|| context_dev::ContextDevClient::new("ctxt-key".into(), timeout)),
        firecrawl: with
            .firecrawl
            .then(|| firecrawl::FirecrawlClient::new("fc-key".into(), timeout)),
        browserbase: with
            .browserbase
            .then(|| browserbase::BrowserbaseClient::new("bb-key".into(), timeout)),
        cloudflare: with.cloudflare.then(|| {
            cloudflare_br::CloudflareBrClient::new("acct".into(), "cf-token".into(), timeout)
        }),
        plain_http: plain_http::PlainHttpClient::new(timeout),
    }
}

fn tool_names(backend: &WebBackend) -> Vec<String> {
    register_tools(backend)
        .iter()
        .map(|t| t.name.clone())
        .collect()
}

#[test]
fn backend_not_api_backed_without_keys() {
    assert!(!backend(Configured::default()).is_api_backed());
}

#[test]
fn any_single_provider_makes_the_backend_api_backed() {
    for with in [
        Configured {
            context_dev: true,
            ..Default::default()
        },
        Configured {
            firecrawl: true,
            ..Default::default()
        },
        Configured {
            browserbase: true,
            ..Default::default()
        },
        Configured {
            cloudflare: true,
            ..Default::default()
        },
    ] {
        assert!(backend(with).is_api_backed());
    }
}

#[test]
fn plain_http_always_offers_extract() {
    assert!(tool_names(&backend(Configured::default())).contains(&"extract".to_string()));
}

#[test]
fn crawl_is_offered_only_by_a_provider_that_supports_it() {
    let none = tool_names(&backend(Configured::default()));
    assert!(!none.contains(&"crawl".to_string()));

    for with in [
        Configured {
            context_dev: true,
            ..Default::default()
        },
        Configured {
            firecrawl: true,
            ..Default::default()
        },
        Configured {
            cloudflare: true,
            ..Default::default()
        },
    ] {
        assert!(tool_names(&backend(with)).contains(&"crawl".to_string()));
    }
}

#[test]
fn search_is_offered_only_by_firecrawl_and_browserbase() {
    for with in [
        Configured {
            firecrawl: true,
            ..Default::default()
        },
        Configured {
            browserbase: true,
            ..Default::default()
        },
    ] {
        assert!(tool_names(&backend(with)).contains(&"search".to_string()));
    }

    for with in [
        Configured::default(),
        Configured {
            context_dev: true,
            ..Default::default()
        },
        Configured {
            cloudflare: true,
            ..Default::default()
        },
    ] {
        assert!(!tool_names(&backend(with)).contains(&"search".to_string()));
    }
}

#[tokio::test]
async fn crawl_without_a_capable_provider_says_which_keys_to_set() {
    let err = backend(Configured::default())
        .crawl("https://example.com", &CrawlOptions::default())
        .await
        .expect_err("plain HTTP cannot crawl");
    let msg = err.to_string();
    assert!(msg.contains("CONTEXT_DEV_API_KEY"), "{msg}");
    assert!(msg.contains("FIRECRAWL_API_KEY"), "{msg}");
}

#[tokio::test]
async fn search_without_a_capable_provider_says_which_keys_to_set() {
    let err = backend(Configured::default())
        .search("rust", 5)
        .await
        .expect_err("only Firecrawl and Browserbase search");
    let msg = err.to_string();
    assert!(msg.contains("FIRECRAWL_API_KEY"), "{msg}");
    assert!(msg.contains("BROWSERBASE_API_KEY"), "{msg}");
}

#[test]
fn every_tool_name_is_offered_once_with_all_providers_configured() {
    let all = backend(Configured {
        context_dev: true,
        firecrawl: true,
        browserbase: true,
        cloudflare: true,
    });
    let mut names = tool_names(&all);
    names.sort();
    assert_eq!(names, ["crawl", "extract", "search"]);
}

#[test]
fn the_highest_priority_provider_describes_a_shared_tool() {
    // Context.dev wins `extract` over Firecrawl because it also serves it.
    let both = backend(Configured {
        context_dev: true,
        firecrawl: true,
        ..Default::default()
    });
    let extract = register_tools(&both)
        .into_iter()
        .find(|t| t.name == "extract")
        .expect("extract is always offered");
    assert!(
        extract.description.contains("Context.dev"),
        "{}",
        extract.description
    );
}

#[test]
fn config_defaults_are_reasonable() {
    let config = WebConfig::default();
    assert_eq!(config.defaults.timeout_ms, 120000);
}
