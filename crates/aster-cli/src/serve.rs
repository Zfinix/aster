//! `aster serve` — the agent in a browser tab, from this repo, on this machine.

use std::env;
use std::net::IpAddr;

use anyhow::{Context, Result};
use aster_serve::ServeConfig;
use clap::Args;

#[derive(Args)]
pub struct ServeArgs {
    /// Port to listen on. Without it, the first free port from 4187.
    #[arg(long, value_name = "PORT")]
    port: Option<u16>,

    /// Address to bind. Off loopback the URL carries a token, since the port
    /// is then reachable by anything that can route to this machine.
    #[arg(long, value_name = "ADDR", default_value = "127.0.0.1")]
    host: IpAddr,

    /// Stay put instead of opening a browser window.
    #[arg(long)]
    no_open: bool,
}

pub async fn run(args: ServeArgs) -> Result<()> {
    let repo_root = env::current_dir().context("could not determine the current directory")?;
    let server = aster_serve::bind(ServeConfig {
        host: args.host,
        port: args.port,
        repo_root: repo_root.clone(),
    })
    .await?;

    if crate::json_mode() {
        println!(
            "{}",
            serde_json::json!({
                "ok": true,
                "url": server.url,
                "address": server.addr.to_string(),
                "repo_root": repo_root.display().to_string(),
                "ui": server.has_ui(),
            })
        );
    } else {
        banner(&server, &repo_root);
    }
    // Opening is the point of the command, so it happens unless waved off.
    if !args.no_open && !crate::json_mode() {
        let _ = open::that_detached(&server.url);
    }
    server.run().await
}

fn banner(server: &aster_serve::Server, repo_root: &std::path::Path) {
    use console::style;

    let name = repo_root
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| repo_root.display().to_string());
    println!();
    println!("  {} {}", style("Aster").bold(), style(&server.url).cyan());
    println!("  {} {}", style("repo").dim(), style(name).dim());
    if !server.has_ui() {
        println!(
            "  {} {}",
            style("note").yellow(),
            style("this build has no browser UI; open the page to see how to get one").dim()
        );
    }
    println!("  {}", style("ctrl-c to stop").dim());
    println!();
}
