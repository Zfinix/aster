//! Who is allowed to talk to this server. Any page in any browser on this
//! machine can reach a loopback port, and a turn here runs commands in the
//! repo, so a request has to prove it came from Aster's own page.

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Redirect, Response};

/// The cookie the token is parked in after the first visit, so the URL the
/// user copies around does not have to keep carrying it.
const COOKIE: &str = "aster_token";

pub async fn guard(
    State(state): State<Arc<crate::state::AppState>>,
    request: Request,
    next: Next,
) -> Response {
    let headers = request.headers().clone();
    let for_page = !request.uri().path().starts_with("/api/");
    if !host_allowed(
        &state,
        headers.get(header::HOST).and_then(|h| h.to_str().ok()),
    ) {
        return match for_page {
            true => crate::pages::wrong_address(),
            false => refuse("this address is not the one Aster is serving"),
        };
    }
    if let Some(origin) = headers.get(header::ORIGIN).and_then(|o| o.to_str().ok())
        && !origin_allowed(&state, origin)
    {
        return refuse("that page is not allowed to talk to Aster");
    }

    let Some(token) = state.token.as_deref() else {
        return next.run(request).await;
    };
    if cookie(&headers, COOKIE).is_some_and(|value| value == token)
        || headers
            .get("x-aster-token")
            .and_then(|t| t.to_str().ok())
            .is_some_and(|value| value == token)
    {
        return next.run(request).await;
    }
    // A fresh browser arrives with the token in the URL. Park it in a cookie
    // and send it back to a clean one, so a shared screen does not leak it.
    let query = request.uri().query().unwrap_or_default();
    if request.method() == axum::http::Method::GET
        && query_value(query, "token").as_deref() == Some(token)
    {
        let cookie = format!("{COOKIE}={token}; Path=/; SameSite=Strict; HttpOnly");
        let path = request.uri().path().to_string();
        return (
            [(header::SET_COOKIE, cookie)],
            Redirect::to(if path.is_empty() { "/" } else { &path }),
        )
            .into_response();
    }
    match for_page {
        true => crate::pages::needs_token(),
        false => refuse("this server needs the token it was started with"),
    }
}

fn refuse(why: &str) -> Response {
    (StatusCode::FORBIDDEN, format!("{why}\n")).into_response()
}

/// A loopback server answers only to loopback names on its own port, so a site
/// that resolves its own domain to 127.0.0.1 cannot reach it. Bound anywhere
/// else, the name is unknowable from here and the token is what stands guard.
fn host_allowed(state: &crate::state::AppState, host: Option<&str>) -> bool {
    if !state.bind.ip().is_loopback() {
        return true;
    }
    let Some(host) = host else {
        return false;
    };
    let (name, port) = split_host(host);
    matches!(name, "localhost" | "127.0.0.1" | "::1" | "[::1]")
        && port.is_none_or(|port| port == state.bind.port())
}

fn origin_allowed(state: &crate::state::AppState, origin: &str) -> bool {
    let Some((scheme, rest)) = origin.split_once("://") else {
        // `null`, from a sandboxed frame or a file:// page.
        return false;
    };
    matches!(scheme, "http" | "https") && host_allowed(state, Some(rest))
}

/// Split `host[:port]`, leaving a bracketed IPv6 literal intact.
fn split_host(host: &str) -> (&str, Option<u16>) {
    match host.rsplit_once(':') {
        Some((name, port)) if !name.ends_with(']') || port.chars().all(|c| c.is_ascii_digit()) => {
            (name, port.parse().ok())
        }
        _ => (host, None),
    }
}

fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value.to_string())
}

fn query_value(query: &str, name: &str) -> Option<String> {
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value.to_string())
}

#[cfg(test)]
#[path = "tests/guard_test.rs"]
mod tests;
