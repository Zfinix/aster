use super::*;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use tower::ServiceExt;

fn app() -> Router {
    let bind: SocketAddr = "127.0.0.1:4187".parse().expect("addr");
    router(Arc::new(AppState::new(PathBuf::from("."), bind, None)))
}

async fn status_of(request: Request<Body>) -> StatusCode {
    app().oneshot(request).await.expect("response").status()
}

#[tokio::test]
async fn the_page_is_served_to_a_loopback_name() {
    let request = Request::builder()
        .uri("/")
        .header(header::HOST, "localhost:4187")
        .body(Body::empty())
        .expect("request");
    assert_eq!(status_of(request).await, StatusCode::OK);
}

#[tokio::test]
async fn the_page_itself_is_behind_the_guard() {
    let request = Request::builder()
        .uri("/")
        .header(header::HOST, "rebound.test")
        .body(Body::empty())
        .expect("request");
    assert_eq!(status_of(request).await, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn another_site_cannot_drive_the_agent() {
    let request = Request::builder()
        .method("POST")
        .uri("/api/host")
        .header(header::HOST, "localhost:4187")
        .header(header::ORIGIN, "https://evil.test")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"type":"ready"}"#))
        .expect("request");
    assert_eq!(status_of(request).await, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_message_from_the_served_page_is_accepted() {
    let request = Request::builder()
        .method("POST")
        .uri("/api/host")
        .header(header::HOST, "localhost:4187")
        .header(header::ORIGIN, "http://localhost:4187")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            r#"{"type":"openExternal","url":"https://example.test"}"#,
        ))
        .expect("request");
    assert_eq!(status_of(request).await, StatusCode::OK);
}

#[tokio::test]
async fn the_token_is_traded_for_a_cookie_on_the_way_in() {
    let bind: SocketAddr = "192.168.1.10:4187".parse().expect("addr");
    let state = Arc::new(AppState::new(
        PathBuf::from("."),
        bind,
        Some("s3cret".to_string()),
    ));
    let request = Request::builder()
        .uri("/?token=s3cret")
        .body(Body::empty())
        .expect("request");
    let response = router(state).oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    assert!(cookie.contains("aster_token=s3cret"), "{cookie}");
    assert!(cookie.contains("HttpOnly"), "{cookie}");
}

#[tokio::test]
async fn without_the_token_there_is_nothing_to_see() {
    let bind: SocketAddr = "192.168.1.10:4187".parse().expect("addr");
    let state = Arc::new(AppState::new(
        PathBuf::from("."),
        bind,
        Some("s3cret".into()),
    ));
    let request = Request::builder()
        .uri("/")
        .body(Body::empty())
        .expect("request");
    let response = router(state).oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[test]
fn a_loopback_bind_is_named_the_way_a_browser_likes_it() {
    let local: SocketAddr = "127.0.0.1:4187".parse().expect("addr");
    assert_eq!(origin(local), "http://localhost:4187");
    let lan: SocketAddr = "192.168.1.10:4187".parse().expect("addr");
    assert_eq!(origin(lan), "http://192.168.1.10:4187");
}
