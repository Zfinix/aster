#![cfg(test)]

use super::*;

#[test]
fn search_request_serializes_camel_case() {
    let request = SearchRequest {
        query: "rust async traits".into(),
        num_results: 5,
        markdown_options: MarkdownOptions { enabled: true },
    };
    let json = serde_json::to_value(&request).unwrap();
    assert_eq!(json["query"], "rust async traits");
    assert_eq!(json["numResults"], 5);
    assert_eq!(json["markdownOptions"]["enabled"], true);
}

#[test]
fn search_response_parses_scraped_and_failed_results() {
    let json = r##"{
        "results": [
            {
                "url": "https://example.com/a",
                "title": "A",
                "description": "First result",
                "relevance": "high",
                "markdown": { "markdown": "# A", "code": "SUCCESS" }
            },
            {
                "url": "https://example.com/b",
                "markdown": { "markdown": null, "code": "TIMEOUT" }
            }
        ],
        "query": "example",
        "key_metadata": { "credits_consumed": 2, "credits_remaining": 498 }
    }"##;
    let parsed: SearchResponse = serde_json::from_str(json).unwrap();
    assert_eq!(parsed.results.len(), 2);
    assert_eq!(parsed.results[0].markdown.as_ref().unwrap().code, "SUCCESS");
    assert_eq!(
        parsed.results[0]
            .markdown
            .as_ref()
            .unwrap()
            .markdown
            .as_deref(),
        Some("# A")
    );
    assert_eq!(parsed.results[1].markdown.as_ref().unwrap().code, "TIMEOUT");
    assert!(
        parsed.results[1]
            .markdown
            .as_ref()
            .unwrap()
            .markdown
            .is_none()
    );
}

#[test]
fn sitemap_response_parses() {
    let json = r#"{
        "success": true,
        "domain": "example.com",
        "urls": ["https://example.com/", "https://example.com/docs"],
        "meta": { "sitemapsDiscovered": 1, "sitemapsFetched": 1, "sitemapsSkipped": 0, "errors": 0 }
    }"#;
    let parsed: SitemapResponse = serde_json::from_str(json).unwrap();
    assert_eq!(parsed.domain, "example.com");
    assert_eq!(parsed.urls.len(), 2);
}

#[test]
fn screenshot_response_parses() {
    let json = r#"{
        "status": "ok",
        "code": 200,
        "domain": "example.com",
        "screenshot": "https://media.brand.dev/screenshots/cache/abc.png",
        "screenshotType": "viewport",
        "width": 1920,
        "height": 1080
    }"#;
    let parsed: ScreenshotResponse = serde_json::from_str(json).unwrap();
    assert_eq!(
        parsed.screenshot,
        "https://media.brand.dev/screenshots/cache/abc.png"
    );
    assert_eq!(parsed.screenshot_type.as_deref(), Some("viewport"));
    assert_eq!(parsed.width, Some(1920));
    assert_eq!(parsed.height, Some(1080));
}

#[test]
fn screenshot_response_tolerates_missing_dimensions() {
    let json = r#"{ "screenshot": "https://media.brand.dev/x.png" }"#;
    let parsed: ScreenshotResponse = serde_json::from_str(json).unwrap();
    assert!(parsed.screenshot_type.is_none());
    assert!(parsed.width.is_none());
    assert!(parsed.height.is_none());
}
