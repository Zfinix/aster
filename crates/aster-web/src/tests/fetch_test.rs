#![cfg(test)]

use super::*;

fn client() -> reqwest::Client {
    reqwest::Client::new()
}

#[test]
fn loopback_and_private_addresses_are_not_public() {
    for ip in [
        "127.0.0.1",
        "10.0.0.5",
        "192.168.1.1",
        "172.16.0.1",
        "169.254.169.254",
        "100.64.0.1",
        "0.0.0.0",
        "::1",
        "fd00::1",
        "fe80::1",
    ] {
        let parsed: IpAddr = ip.parse().expect("test address parses");
        assert!(!is_public(&parsed), "{ip} should not be public");
    }
}

#[test]
fn routable_addresses_are_public() {
    for ip in ["1.1.1.1", "93.184.216.34", "2606:4700::1111"] {
        let parsed: IpAddr = ip.parse().expect("test address parses");
        assert!(is_public(&parsed), "{ip} should be public");
    }
}

#[tokio::test]
async fn non_http_schemes_are_refused() {
    let err = fetch(&client(), "file:///etc/passwd", false)
        .await
        .expect_err("file urls are not fetchable");
    assert!(err.to_string().contains("only http and https"), "{err}");
}

#[tokio::test]
async fn loopback_is_refused_unless_private_urls_are_allowed() {
    let err = fetch(&client(), "http://127.0.0.1:9/", false)
        .await
        .expect_err("loopback is blocked by default");
    assert!(err.to_string().contains("non-public address"), "{err:#}");
}

#[test]
fn truncate_marks_what_it_cut_and_leaves_short_text_alone() {
    let short = "already short".to_string();
    assert_eq!(truncate(short.clone()), short);

    let long = "x".repeat(MAX_CHARS + 100);
    let cut = truncate(long);
    assert!(cut.ends_with("[truncated]"), "{}", &cut[cut.len() - 40..]);
    assert_eq!(cut.chars().filter(|c| *c == 'x').count(), MAX_CHARS);
}

#[test]
fn truncate_cuts_on_a_character_boundary() {
    let text: String = "é".repeat(MAX_CHARS + 10);
    let cut = truncate(text);
    assert_eq!(cut.chars().filter(|c| *c == 'é').count(), MAX_CHARS);
}
