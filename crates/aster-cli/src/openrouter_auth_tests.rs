//! Tests for the shared sign-in plumbing the OpenRouter flow rides on.

use crate::mcp::oauth;

#[test]
fn pkce_pair_is_url_safe_and_consistent() {
    use base64::Engine;
    use sha2::{Digest, Sha256};

    let pkce = oauth::pkce();
    assert_eq!(pkce.verifier.len(), 43);
    assert!(
        pkce.verifier
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    );
    let expect = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(Sha256::digest(pkce.verifier.as_bytes()));
    assert_eq!(pkce.challenge, expect);
}

#[tokio::test]
async fn callback_survives_preconnect_sockets_and_decodes_the_code() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        // A speculative browser socket that never sends a request.
        drop(tokio::net::TcpStream::connect(addr).await.unwrap());
        // The real redirect: percent-encoded code, split across two writes.
        let mut real = tokio::net::TcpStream::connect(addr).await.unwrap();
        real.write_all(b"GET /callback?code=abc%2F123&state=x")
            .await
            .unwrap();
        real.flush().await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        real.write_all(b" HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
            .await
            .unwrap();
        let mut reply = String::new();
        let _ = real.read_to_string(&mut reply).await;
        assert!(reply.contains("200 OK"), "{reply}");
    });

    let callback = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        oauth::await_callback(listener),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(callback.code, "abc/123");
    assert_eq!(callback.state.as_deref(), Some("x"));
}

#[tokio::test]
async fn a_denied_sign_in_is_an_error_not_a_missing_code() {
    use tokio::io::AsyncWriteExt;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let mut socket = tokio::net::TcpStream::connect(addr).await.unwrap();
        socket
            .write_all(b"GET /callback?error=access_denied HTTP/1.1\r\nHost: h\r\n\r\n")
            .await
            .unwrap();
    });

    let err = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        oauth::await_callback(listener),
    )
    .await
    .unwrap()
    .unwrap_err();
    assert!(err.to_string().contains("declined"), "{err}");
}
