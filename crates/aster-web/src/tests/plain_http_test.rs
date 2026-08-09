use super::*;

#[test]
fn strips_basic_html() {
    let html = "<html><body><p>hello</p><p>world</p></body></html>";
    let md = html_to_markdown(html);
    assert!(md.contains("hello"));
    assert!(md.contains("world"));
    assert!(!md.contains("<p>"));
    assert!(!md.contains("<html>"));
}

#[test]
fn removes_script_and_style() {
    let html = "<html><head><style>body{}</style></head><body><script>alert(1)</script><p>ok</p></body></html>";
    let md = html_to_markdown(html);
    assert!(md.contains("ok"));
    assert!(!md.contains("alert"));
    assert!(!md.contains("body{}"));
}

#[test]
fn empty_html_gives_placeholder() {
    let md = html_to_markdown("<html><head></head><body></body></html>");
    assert_eq!(md, "[no text content extracted]");
}

#[test]
fn body_routing_detects_documents_by_magic_bytes() {
    let rtf = br"{\rtf1\ansi Hello from RTF}";
    let md = body_to_markdown("application/octet-stream", rtf, "https://x.test/doc").unwrap();
    assert!(md.contains("Hello from RTF"), "{md}");
}

#[test]
fn body_routing_strips_html_bodies() {
    let md = body_to_markdown("text/html", b"<p>hi</p>", "https://x.test").unwrap();
    assert!(md.contains("hi"));
    assert!(!md.contains("<p>"));
}

#[test]
fn body_routing_passes_other_text_through() {
    let md = body_to_markdown("application/json", br#"{"a":1}"#, "https://x.test").unwrap();
    assert_eq!(md, r#"{"a":1}"#);
}
