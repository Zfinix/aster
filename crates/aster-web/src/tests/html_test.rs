#![cfg(test)]

use super::*;

#[test]
fn attr_reads_either_quote_style() {
    assert_eq!(
        attr(r#"<a href="https://x.test" class='result-link'"#, "href").as_deref(),
        Some("https://x.test")
    );
    assert_eq!(
        attr(r#"<a href='https://x.test' class="result-link""#, "href").as_deref(),
        Some("https://x.test")
    );
}

#[test]
fn attr_is_none_when_the_value_is_unquoted_or_absent() {
    assert_eq!(attr("<a href=https://x.test>", "href"), None);
    assert_eq!(attr("<a class='result-link'>", "href"), None);
}

#[test]
fn to_text_strips_tags_and_collapses_space() {
    let fragment = "The  <b>Agent</b>\n <b>Client</b>   Protocol &amp; friends";
    assert_eq!(to_text(fragment), "The Agent Client Protocol & friends");
}

#[test]
fn to_document_keeps_paragraph_breaks() {
    let html = "<h1>Title</h1><p>First para.</p><p>Second para.</p>";
    assert_eq!(to_document(html), "Title\n\nFirst para.\n\nSecond para.");
}

#[test]
fn to_document_drops_script_and_style_bodies() {
    let html = "<style>body{color:red}</style><p>Real text</p><script>alert('x')</script>";
    assert_eq!(to_document(html), "Real text");
}

#[test]
fn percent_decode_handles_escapes_plus_and_malformed_input() {
    assert_eq!(
        percent_decode("https%3A%2F%2Fx.test%2Fa+b"),
        "https://x.test/a b"
    );
    assert_eq!(percent_decode("100%"), "100%");
    assert_eq!(percent_decode("%zz"), "%zz");
}

#[test]
fn decode_entities_resolves_ampersand_last() {
    assert_eq!(decode_entities("&amp;lt;"), "&lt;");
    assert_eq!(decode_entities("a &lt;b&gt; &quot;c&quot;"), "a <b> \"c\"");
}
