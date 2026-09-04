#![forbid(unsafe_code)]
//! `aster serve`: Aster's own front-end, in a browser on this machine. The page is
//! the desktop app's, built for the web, and every turn runs as a child `aster`
//! process, so a tab and a terminal drive the same CLI in the same repo.

mod assets;
mod cli;
mod files;
mod guard;
mod host;
mod info;
mod pages;
mod paths;
mod run;
mod sessions;
mod settings;
mod state;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use axum::routing::{get, post};
use axum::{Router, middleware};
use tokio::net::TcpListener;
use ulid::Ulid;

use state::AppState;

pub const DEFAULT_PORT: u16 = 4187;

const PORT_SCAN: u16 = 10;

pub struct ServeConfig {
    pub host: IpAddr,
    pub port: Option<u16>,
    pub repo_root: PathBuf,
}

impl Default for ServeConfig {
    fn default() -> Self {
        Self {
            host: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: None,
            repo_root: PathBuf::from("."),
        }
    }
}

/// A bound server, not yet serving. Split from [`Server::run`] so the caller
/// can print the address, and open it, before the first request arrives.
pub struct Server {
    listener: TcpListener,
    state: Arc<AppState>,
    pub url: String,
    pub addr: SocketAddr,
}

pub async fn bind(config: ServeConfig) -> Result<Server> {
    let listener = listen(config.host, config.port).await?;
    let addr = listener.local_addr().context("reading the bound address")?;

    // Off this machine, the port is reachable by anyone who can route to it,
    // so the URL carries a secret and the first visit trades it for a cookie.
    let token = (!addr.ip().is_loopback()).then(|| Ulid::new().to_string());
    let url = match &token {
        Some(token) => format!("{}/?token={token}", origin(addr)),
        None => format!("{}/", origin(addr)),
    };
    let repo_root = config
        .repo_root
        .canonicalize()
        .unwrap_or(config.repo_root.clone());

    Ok(Server {
        listener,
        state: Arc::new(AppState::new(repo_root, addr, token)),
        url,
        addr,
    })
}

impl Server {
    /// True when this binary carries a built UI. False means the API is up but
    /// a browser has nothing to load.
    pub fn has_ui(&self) -> bool {
        assets::is_built()
    }

    pub async fn run(self) -> Result<()> {
        let app = router(self.state);
        axum::serve(self.listener, app)
            .await
            .context("serving the browser UI")
    }
}

fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/events", get(host::events))
        .route("/api/host", post(host::message))
        .fallback(get(assets::serve))
        // Not `route_layer`: that would leave the page itself, which is every
        // request that is not an API call, outside the guard.
        .layer(middleware::from_fn_with_state(state.clone(), guard::guard))
        .with_state(state)
}

async fn listen(host: IpAddr, port: Option<u16>) -> Result<TcpListener> {
    if let Some(port) = port {
        return TcpListener::bind(SocketAddr::new(host, port))
            .await
            .with_context(|| format!("could not listen on {host}:{port}"));
    }
    for port in DEFAULT_PORT..DEFAULT_PORT + PORT_SCAN {
        if let Ok(listener) = TcpListener::bind(SocketAddr::new(host, port)).await {
            return Ok(listener);
        }
    }
    bail!(
        "ports {DEFAULT_PORT}-{} are all in use; pass --port to pick another",
        DEFAULT_PORT + PORT_SCAN - 1
    )
}

fn origin(addr: SocketAddr) -> String {
    match addr.ip().is_loopback() {
        true => format!("http://localhost:{}", addr.port()),
        false => format!("http://{addr}"),
    }
}

#[cfg(test)]
#[path = "tests/lib_test.rs"]
mod tests;
