use super::*;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use crate::state::AppState;

fn state(ip: IpAddr) -> AppState {
    AppState::new(
        std::path::PathBuf::from("."),
        SocketAddr::new(ip, 4187),
        None,
    )
}

fn loopback() -> AppState {
    state(IpAddr::V4(Ipv4Addr::LOCALHOST))
}

#[test]
fn loopback_names_on_the_served_port_are_allowed() {
    let state = loopback();
    for host in [
        "localhost:4187",
        "127.0.0.1:4187",
        "[::1]:4187",
        "localhost",
    ] {
        assert!(host_allowed(&state, Some(host)), "{host}");
    }
}

#[test]
fn a_rebound_name_is_refused() {
    let state = loopback();
    // What a site pointing its own domain at 127.0.0.1 would send.
    assert!(!host_allowed(&state, Some("evil.test:4187")));
    assert!(!host_allowed(&state, Some("localhost:9999")));
    assert!(!host_allowed(&state, None));
}

#[test]
fn off_loopback_the_token_stands_guard_instead_of_the_name() {
    let state = state(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)));
    assert!(host_allowed(&state, Some("box.local:4187")));
    assert!(host_allowed(&state, None));
}

#[test]
fn only_same_origin_pages_may_call() {
    let state = loopback();
    assert!(origin_allowed(&state, "http://localhost:4187"));
    assert!(!origin_allowed(&state, "http://evil.test"));
    // A sandboxed frame sends `null`, which names no page at all.
    assert!(!origin_allowed(&state, "null"));
}

#[test]
fn ipv6_literals_keep_their_brackets() {
    assert_eq!(split_host("[::1]:4187"), ("[::1]", Some(4187)));
    assert_eq!(split_host("localhost"), ("localhost", None));
    assert_eq!(split_host("127.0.0.1:80"), ("127.0.0.1", Some(80)));
}

#[test]
fn the_token_cookie_is_read_out_of_a_full_jar() {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::COOKIE,
        "theme=dark; aster_token=abc123; other=1".parse().unwrap(),
    );
    assert_eq!(cookie(&headers, COOKIE), Some("abc123".to_string()));
    assert_eq!(cookie(&headers, "missing"), None);
}

#[test]
fn the_token_is_read_out_of_the_query() {
    assert_eq!(query_value("a=1&token=xyz", "token"), Some("xyz".into()));
    assert_eq!(query_value("", "token"), None);
}
