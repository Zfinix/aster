//! Tag normalization for `aster upgrade --version`.

use super::normalize_tag;

#[test]
fn accepts_the_spellings_install_sh_accepts() {
    assert_eq!(normalize_tag("0.4.0"), "cli-v0.4.0");
    assert_eq!(normalize_tag("v0.4.0"), "cli-v0.4.0");
    assert_eq!(normalize_tag("cli-v0.4.0"), "cli-v0.4.0");
}

#[test]
fn skips_vscode_releases_when_picking_the_latest_tag() {
    let feed = "\
<entry><id>tag:github.com,2008:Repository/1311213274/vscode-v0.6.1</id></entry>
<entry><id>tag:github.com,2008:Repository/1311213274/cli-v0.6.1</id></entry>
<entry><id>tag:github.com,2008:Repository/1311213274/cli-v0.6.0</id></entry>";
    assert_eq!(super::first_cli_tag(feed), Some("cli-v0.6.1"));
}

#[test]
fn reports_a_feed_with_no_cli_release() {
    let feed = "<entry><id>tag:github.com,2008:Repository/1311213274/vscode-v0.6.1</id></entry>";
    assert_eq!(super::first_cli_tag(feed), None);
    assert_eq!(super::first_cli_tag(""), None);
}
