//! Tag normalization for `aster upgrade --version`.

use super::normalize_tag;

#[test]
fn accepts_the_spellings_install_sh_accepts() {
    assert_eq!(normalize_tag("0.4.0"), "cli-v0.4.0");
    assert_eq!(normalize_tag("v0.4.0"), "cli-v0.4.0");
    assert_eq!(normalize_tag("cli-v0.4.0"), "cli-v0.4.0");
}
