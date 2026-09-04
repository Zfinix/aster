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
    exa: bool,
    perplexity: bool,
    context_dev: bool,
    firecrawl: bool,
    browserbase: bool,
    cloudflare: bool,
}

fn backend(with: Configured) -> WebBackend {
    let timeout = WebConfig::default().defaults.timeout_ms;
    WebBackend {
        exa: with
            .exa
            .then(|| exa::ExaClient::new("exa-key".into(), timeout)),
        perplexity: with
            .perplexity
            .then(|| perplexity::PerplexityClient::new("pplx-key".into(), timeout)),
        context_dev: with
            .context_dev
            .then(|| context_dev::ContextDevClient::new("ctxt-key".into(), timeout)),
        firecrawl: firecrawl::FirecrawlClient::new(
            with.firecrawl.then(|| "fc-key".into()),
            timeout,
        ),
        browserbase: with
            .browserbase
            .then(|| browserbase::BrowserbaseClient::new("bb-key".into(), timeout)),
        cloudflare: with.cloudflare.then(|| {
            cloudflare_br::CloudflareBrClient::new("acct".into(), "cf-token".into(), timeout)
        }),
        jina: jina::JinaClient::new(None, timeout),
        duckduckgo: std::sync::Arc::new(duckduckgo::DuckDuckGoClient::new()),
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
            perplexity: true,
            ..Default::default()
        },
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
fn search_is_offered_whether_or_not_a_key_is_set() {
    for with in [
        Configured::default(),
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
        assert!(tool_names(&backend(with)).contains(&"search".to_string()));
    }
}

#[test]
fn exa_leads_search_but_not_extract() {
    let described = |with: Configured, name: &str| {
        register_tools(&backend(with))
            .into_iter()
            .find(|t| t.name == name)
            .unwrap_or_else(|| panic!("{name} is offered"))
            .description
    };
    let both = || Configured {
        exa: true,
        context_dev: true,
        ..Default::default()
    };
    assert!(described(both(), "search").contains("Exa"));
    assert!(!described(both(), "extract").contains("Exa"));

    // On its own Exa still serves extract, just below the specialists.
    let alone = Configured {
        exa: true,
        ..Default::default()
    };
    assert!(described(alone, "extract").contains("Exa"));
}

#[test]
fn exa_alone_makes_the_backend_api_backed() {
    assert!(
        backend(Configured {
            exa: true,
            ..Default::default()
        })
        .is_api_backed()
    );
}

#[test]
fn perplexity_serves_search_when_exa_is_absent() {
    let described = |with: Configured| {
        register_tools(&backend(with))
            .into_iter()
            .find(|t| t.name == "search")
            .expect("search is always offered")
            .description
            .clone()
    };

    let perplexity = Configured {
        perplexity: true,
        ..Default::default()
    };
    assert!(described(perplexity).contains("Perplexity"));

    let both_desc = described(Configured {
        exa: true,
        perplexity: true,
        ..Default::default()
    });
    assert!(both_desc.contains("Exa"));
    assert!(!both_desc.contains("Perplexity"));
}

#[test]
fn a_keyed_provider_wins_the_search_name_from_keyless_firecrawl() {
    let described = |with: Configured| {
        register_tools(&backend(with))
            .into_iter()
            .find(|t| t.name == "search")
            .expect("search is always offered")
            .description
    };
    let keyless = described(Configured::default());
    assert!(keyless.contains("Firecrawl"), "{keyless}");
    assert!(!keyless.contains("DuckDuckGo"), "{keyless}");

    let keyed = described(Configured {
        context_dev: true,
        ..Default::default()
    });
    assert!(!keyed.contains("DuckDuckGo"), "{keyed}");
    assert!(!keyed.contains("Firecrawl"), "{keyed}");
}

#[test]
fn keyless_firecrawl_serves_search_and_extract_but_never_crawl() {
    let names = tool_names(&backend(Configured::default()));
    assert!(names.contains(&"search".to_string()));
    assert!(names.contains(&"extract".to_string()));
    assert!(!names.contains(&"crawl".to_string()));
}

#[tokio::test]
async fn keyless_firecrawl_says_a_key_is_needed_to_crawl() {
    let err = firecrawl::FirecrawlClient::new(None, 1000)
        .crawl("https://example.com", &CrawlOptions::default())
        .await
        .expect_err("the keyless tier cannot crawl");
    assert!(err.to_string().contains("FIRECRAWL_API_KEY"), "{err}");
}

#[test]
fn sitemap_and_screenshot_are_offered_only_by_context_dev() {
    let names = tool_names(&backend(Configured {
        context_dev: true,
        ..Default::default()
    }));
    assert!(names.contains(&"sitemap".to_string()));
    assert!(names.contains(&"screenshot".to_string()));

    let names = tool_names(&backend(Configured {
        firecrawl: true,
        browserbase: true,
        cloudflare: true,
        ..Default::default()
    }));
    assert!(!names.contains(&"sitemap".to_string()));
    assert!(!names.contains(&"screenshot".to_string()));
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
async fn sitemap_without_context_dev_says_which_key_to_set() {
    let err = backend(Configured::default())
        .sitemap("example.com", 500, None)
        .await
        .expect_err("only Context.dev serves sitemaps");
    assert!(err.to_string().contains("CONTEXT_DEV_API_KEY"), "{err}");
}

#[tokio::test]
async fn screenshot_without_context_dev_says_which_key_to_set() {
    let err = backend(Configured::default())
        .screenshot("https://example.com", false)
        .await
        .expect_err("only Context.dev serves screenshots");
    assert!(err.to_string().contains("CONTEXT_DEV_API_KEY"), "{err}");
}

#[test]
fn every_tool_name_is_offered_once_with_all_providers_configured() {
    let all = backend(Configured {
        exa: true,
        perplexity: true,
        context_dev: true,
        firecrawl: true,
        browserbase: true,
        cloudflare: true,
    });
    let mut names = tool_names(&all);
    names.sort();
    assert_eq!(
        names,
        ["crawl", "extract", "screenshot", "search", "sitemap"]
    );
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

#[tokio::test]
async fn dispatch_returns_the_first_successful_attempt() {
    let attempts: Vec<Attempt<'_, u32>> = vec![
        ("first", Box::pin(async { anyhow::bail!("down") })),
        ("second", Box::pin(async { Ok(2) })),
        ("third", Box::pin(async { Ok(3) })),
    ];
    assert_eq!(first_success("search", attempts).await.unwrap(), 2);
}

#[tokio::test]
async fn dispatch_error_names_every_failed_provider() {
    let attempts: Vec<Attempt<'_, u32>> = vec![
        ("Exa", Box::pin(async { anyhow::bail!("dns error") })),
        (
            "DuckDuckGo",
            Box::pin(async { anyhow::bail!("rate limited") }),
        ),
    ];
    let err = first_success("search", attempts)
        .await
        .expect_err("all attempts failed");
    let msg = err.to_string();
    assert!(msg.contains("every search provider failed"), "{msg}");
    assert!(msg.contains("Exa: dns error"), "{msg}");
    assert!(msg.contains("DuckDuckGo: rate limited"), "{msg}");
}

#[test]
fn config_defaults_are_reasonable() {
    let config = WebConfig::default();
    assert_eq!(config.defaults.timeout_ms, 120000);
}
