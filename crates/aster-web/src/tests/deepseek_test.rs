use super::*;

#[test]
fn a_search_result_becomes_a_page_with_the_model_summary() {
    let parsed: SearchResponse = serde_json::from_str(
        r#"{"results":[{
            "rerank_score": 2.4,
            "title": "DeepSeek V4 model released",
            "url": "https://example.com/deepseek-v4",
            "snippet": "short",
            "summary": "DeepSeek has released the V4 model.",
            "content": "",
            "site_name": "Example News",
            "site_icon": null,
            "date": "2026-06-01T00:00:00+00:00",
            "authority": 3
        }]}"#,
    )
    .unwrap();
    let page = search_page(parsed.results.into_iter().next().unwrap());
    assert_eq!(page.markdown, "DeepSeek has released the V4 model.");
    assert_eq!(page.metadata.url, "https://example.com/deepseek-v4");
    assert_eq!(
        page.metadata.title.as_deref(),
        Some("DeepSeek V4 model released")
    );
    assert_eq!(
        page.metadata.description.as_deref(),
        Some("Example News, published 2026-06-01T00:00:00+00:00")
    );
}

#[test]
fn a_result_without_a_summary_falls_back_to_the_snippet() {
    let page = search_page(SearchResult {
        title: "t".into(),
        url: "https://example.com".into(),
        snippet: "the snippet".into(),
        summary: String::new(),
        site_name: None,
        date: None,
    });
    assert_eq!(page.markdown, "the snippet");
    assert_eq!(page.metadata.description, None);
}

#[test]
fn an_api_error_is_reduced_to_its_message() {
    let body = r#"{"error":{"message":"'count' must be between 1 and 50","type":"invalid_request_error","param":null,"code":"invalid_request_error"}}"#;
    assert_eq!(error_message(body), "'count' must be between 1 and 50");
    assert_eq!(
        error_message("<html>bad gateway</html>"),
        "<html>bad gateway</html>"
    );
}

#[test]
fn a_long_summary_is_cut_at_the_cap() {
    let long = "x".repeat(MAX_SUMMARY_CHARS + 10);
    let cut = truncate(long);
    assert_eq!(
        cut.chars().count(),
        MAX_SUMMARY_CHARS + "\n\n[truncated]".len()
    );
    assert!(cut.ends_with("[truncated]"));
    assert_eq!(truncate("short".into()), "short");
}

#[test]
fn fetched_markdown_loses_its_punctuation_escapes() {
    let escaped = "Search \\/ Fetch API\\, now\\.\n\n\\# Search \\& Fetch\n\na \\ b \\\\ c";
    assert_eq!(
        unescape(escaped),
        "Search / Fetch API, now.\n\n# Search & Fetch\n\na \\ b \\ c"
    );
}
