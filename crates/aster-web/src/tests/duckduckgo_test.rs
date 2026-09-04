#![cfg(test)]

use super::*;

const PAGE: &str = r#"
<table>
  <tr>
    <td>1.&nbsp;</td>
    <td>
      <a rel="nofollow" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fagentclientprotocol.com%2Fintro&amp;rut=82882" class='result-link'>Introduction - Agent Client Protocol</a>
    </td>
  </tr>
  <tr>
    <td>&nbsp;</td>
    <td class='result-snippet'>
      The <b>Agent</b> <b>Client</b> <b>Protocol</b> (ACP) standardizes communication &amp; more.
    </td>
  </tr>
  <tr>
    <td>
      <a rel="nofollow" href="//duckduckgo.com/y.js?ad_provider=x" class='result-link'>An Advert</a>
    </td>
  </tr>
  <tr>
    <td>2.&nbsp;</td>
    <td>
      <a rel="nofollow" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fzed.dev%2Facp&amp;rut=1234" class='result-link'>Zed - ACP</a>
    </td>
  </tr>
  <tr>
    <td class='result-snippet'>Zed's page.</td>
  </tr>
</table>
"#;

#[test]
fn parse_pulls_titles_urls_and_snippets() {
    let hits = parse(PAGE, 10);
    assert_eq!(hits.len(), 2);
    assert_eq!(
        hits[0],
        Hit {
            title: "Introduction - Agent Client Protocol".into(),
            url: "https://agentclientprotocol.com/intro".into(),
            snippet: "The Agent Client Protocol (ACP) standardizes communication & more.".into(),
        }
    );
    assert_eq!(hits[1].url, "https://zed.dev/acp");
    assert_eq!(hits[1].snippet, "Zed's page.");
}

#[test]
fn parse_drops_sponsored_rows() {
    let hits = parse(PAGE, 10);
    assert!(!hits.iter().any(|h| h.title == "An Advert"), "{hits:?}");
}

#[test]
fn parse_stops_at_max_results() {
    assert_eq!(parse(PAGE, 1).len(), 1);
    assert_eq!(parse(PAGE, 0).len(), 0);
}

#[test]
fn parse_of_a_page_with_no_results_is_empty() {
    assert!(parse("<html><body>nothing here</body></html>", 10).is_empty());
}

#[test]
fn result_url_unwraps_the_redirect_and_keeps_direct_links() {
    assert_eq!(
        result_url("//duckduckgo.com/l/?uddg=https%3A%2F%2Fx.test%2Fa&amp;rut=9").as_deref(),
        Some("https://x.test/a")
    );
    assert_eq!(
        result_url("https://x.test/direct").as_deref(),
        Some("https://x.test/direct")
    );
    assert_eq!(result_url("//duckduckgo.com/y.js?ad=1"), None);
    assert_eq!(result_url("/relative"), None);
}

#[test]
fn safesearch_parses_the_documented_words_only() {
    assert_eq!(SafeSearch::parse("Strict"), Some(SafeSearch::Strict));
    assert_eq!(SafeSearch::parse("moderate"), Some(SafeSearch::Moderate));
    assert_eq!(SafeSearch::parse(" off "), Some(SafeSearch::Off));
    assert_eq!(SafeSearch::parse("maybe"), None);
}
