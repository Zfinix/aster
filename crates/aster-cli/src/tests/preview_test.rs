use std::fs;
use std::path::Path;

use super::*;

fn repo() -> tempfile::TempDir {
    tempfile::tempdir().expect("tempdir")
}

#[test]
fn a_loopback_url_is_local_and_carries_its_port() {
    let dir = repo();
    let preview = resolve(dir.path(), "http://localhost:5173/pricing").expect("resolved");
    assert!(preview.local);
    assert_eq!(preview.port, Some(5173));
    assert_eq!(preview.url, "http://localhost:5173/pricing");
}

#[test]
fn loopback_is_recognised_by_address_as_well_as_by_name() {
    let dir = repo();
    for target in [
        "http://127.0.0.1:3000",
        "http://[::1]:3000",
        "http://0.0.0.0:3000",
        "http://app.localhost:3000",
    ] {
        assert!(resolve(dir.path(), target).expect(target).local, "{target}");
    }
}

#[test]
fn a_public_url_needs_the_users_say_so() {
    let dir = repo();
    let preview = resolve(dir.path(), "https://example.com/pricing").expect("resolved");
    assert!(!preview.local);
    // Only loopback is probed; a public host is not ours to connect to.
    assert_eq!(preview.port, None);
}

#[test]
fn userinfo_does_not_make_a_public_host_look_local() {
    let dir = repo();
    let preview = resolve(dir.path(), "http://localhost@example.com/").expect("resolved");
    assert!(!preview.local, "example.com is the host, not localhost");
}

#[test]
fn a_bare_host_and_port_becomes_an_http_url() {
    let dir = repo();
    let preview = resolve(dir.path(), "localhost:5173").expect("resolved");
    assert_eq!(preview.url, "http://localhost:5173");
    assert!(preview.local);
}

#[test]
fn a_bare_port_means_this_machine() {
    let dir = repo();
    let preview = resolve(dir.path(), ":8080/about").expect("resolved");
    assert_eq!(preview.url, "http://localhost:8080/about");
}

#[test]
fn a_repo_file_opens_without_asking() {
    let dir = repo();
    fs::write(dir.path().join("index.html"), "<h1>hi</h1>").expect("write");
    let preview = resolve(dir.path(), "index.html").expect("resolved");
    assert!(preview.local);
    assert!(preview.url.starts_with("file:///"), "{}", preview.url);
    assert!(preview.url.ends_with("/index.html"), "{}", preview.url);
}

#[test]
fn a_directory_opens_its_index() {
    let dir = repo();
    fs::create_dir(dir.path().join("dist")).expect("mkdir");
    fs::write(dir.path().join("dist/index.html"), "<h1>hi</h1>").expect("write");
    let preview = resolve(dir.path(), "dist").expect("resolved");
    assert!(preview.url.ends_with("/dist/index.html"), "{}", preview.url);
}

#[test]
fn a_directory_without_an_index_says_so() {
    let dir = repo();
    fs::create_dir(dir.path().join("dist")).expect("mkdir");
    let err = resolve(dir.path(), "dist").expect_err("no index");
    assert!(format!("{err:#}").contains("no index.html"), "{err:#}");
}

#[test]
fn a_file_outside_the_repo_asks_first() {
    let dir = repo();
    let outside = repo();
    fs::write(outside.path().join("page.html"), "<h1>hi</h1>").expect("write");
    let target = outside.path().join("page.html");
    let preview = resolve(dir.path(), &target.to_string_lossy()).expect("resolved");
    assert!(!preview.local);
}

#[test]
fn a_missing_file_tells_the_model_to_build_it_first() {
    let dir = repo();
    let err = resolve(dir.path(), "dist/index.html").expect_err("missing");
    let text = format!("{err:#}");
    assert!(text.contains("no such file"), "{text}");
    assert!(text.contains("Build the page first"), "{text}");
}

#[test]
fn spaces_and_fragment_characters_are_escaped_in_a_file_url() {
    let dir = repo();
    fs::write(dir.path().join("my page.html"), "<h1>hi</h1>").expect("write");
    let preview = resolve(dir.path(), "my page.html").expect("resolved");
    assert!(preview.url.ends_with("my%20page.html"), "{}", preview.url);
}

#[test]
fn schemes_that_are_not_pages_are_refused() {
    let dir = repo();
    for target in [
        "javascript:alert(1)",
        "data:text/html,<h1>hi</h1>",
        "vscode://file/etc/passwd",
    ] {
        let err = resolve(dir.path(), target).expect_err(target);
        assert!(
            format!("{err:#}").contains("opens pages"),
            "{target}: {err:#}"
        );
    }
}

#[test]
fn an_empty_target_asks_for_one() {
    let dir = repo();
    let err = resolve(dir.path(), "   ").expect_err("empty");
    assert!(format!("{err:#}").contains("needs a `target`"), "{err:#}");
}

#[tokio::test]
async fn a_dead_port_is_refused_before_the_browser_sees_it() {
    // Port 1 is reserved and never bound by a dev server.
    let err = probe(1).await.expect_err("nothing listening");
    assert!(
        format!("{err:#}").contains("Start the dev server first"),
        "{err:#}"
    );
}

#[tokio::test]
async fn a_listening_port_passes_the_probe() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let port = listener.local_addr().expect("addr").port();
    probe(port).await.expect("the port is open");
}

/// Drives the whole tool with a launcher that records instead of opening a
/// window, so the success path is exercised without a browser appearing.
async fn opened(dir: &Path, ctx: &SessionCtx, target: &str) -> String {
    fn noop(_: &str) -> anyhow::Result<()> {
        Ok(())
    }
    open_with(dir, None, ctx, target, None, noop)
        .await
        .unwrap_or_else(|e| format!("error: {e:#}"))
}

#[tokio::test]
async fn a_public_url_is_refused_when_nobody_can_approve_it() {
    let dir = repo();
    let out = opened(dir.path(), &SessionCtx::default(), "https://example.com/").await;
    assert!(out.contains("declined"), "{out}");
    assert!(out.contains("give them the link"), "{out}");
}

#[tokio::test]
async fn a_repo_page_needs_no_approval() {
    let dir = repo();
    fs::write(dir.path().join("index.html"), "<h1>hi</h1>").expect("write");
    let out = opened(dir.path(), &SessionCtx::default(), "index.html").await;
    assert!(out.starts_with("opened file:///"), "{out}");
}

#[tokio::test]
async fn a_loopback_url_with_no_server_behind_it_never_reaches_the_browser() {
    let dir = repo();
    let out = opened(dir.path(), &SessionCtx::default(), "http://localhost:1/").await;
    assert!(out.contains("Start the dev server first"), "{out}");
}

#[tokio::test]
async fn the_same_page_is_not_opened_twice_in_one_session() {
    let dir = repo();
    fs::write(dir.path().join("index.html"), "<h1>hi</h1>").expect("write");
    let ctx = SessionCtx::default();
    assert!(
        opened(dir.path(), &ctx, "index.html")
            .await
            .starts_with("opened")
    );
    let again = opened(dir.path(), &ctx, "index.html").await;
    assert!(again.contains("already open"), "{again}");
    assert!(again.contains("reload"), "{again}");
}

#[tokio::test]
async fn a_running_server_is_opened() {
    let dir = repo();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let target = format!("http://localhost:{port}/pricing");
    let out = opened(dir.path(), &SessionCtx::default(), &target).await;
    assert_eq!(
        out,
        format!(
            "opened {target} in the user's browser; say so in your reply and describe what they are looking at, without opening it again"
        )
    );
}
