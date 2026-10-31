//! Folder trust: a cloned repo's config may not run code, pick the endpoint
//! that receives your key, or widen access until you run `aster trust` there.

use std::path::{Path, PathBuf};
use std::sync::Once;

use anyhow::{Context, Result};
use clap::Args;

/// Whether the folder aster runs in may configure it beyond what is safe for
/// any repo. What an untrusted repo may still set is [`crate::settings`]'s call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RepoTrust {
    Trusted,
    Untrusted,
}

impl RepoTrust {
    /// Trusted when `root`, or a folder above it, was trusted with `aster trust`.
    /// `None` means the current directory, matching how plugins resolve it.
    pub(crate) fn of(root: Option<&Path>) -> Self {
        let Some(root) = resolve(root) else {
            return Self::Untrusted;
        };
        let trusted = crate::persist::store()
            .map(|store| store.trusted_folders().load())
            .unwrap_or_default();
        match trusted.iter().any(|dir| root.starts_with(dir)) {
            true => Self::Trusted,
            false => Self::Untrusted,
        }
    }
}

/// Say once per process that the repo's own config was held back, so a user
/// whose servers vanished knows the one command that brings them back.
pub(crate) fn note_untrusted(root: &Path) {
    static NOTED: Once = Once::new();
    NOTED.call_once(|| {
        eprintln!(
            "note: {} isn't trusted yet, so its .env, MCP servers, plugins, and permission \
             changes are off. If you trust this repo, run `aster trust` in it.",
            root.display()
        );
    });
}

fn resolve(root: Option<&Path>) -> Option<PathBuf> {
    let root = match root {
        Some(root) => root.to_path_buf(),
        None => std::env::current_dir().ok()?,
    };
    root.canonicalize().ok()
}

#[derive(Debug, Args)]
pub(crate) struct TrustArgs {
    /// The folder to trust. Defaults to the current one; folders inside it are trusted too.
    pub path: Option<PathBuf>,
    /// Stop trusting the folder instead.
    #[arg(long)]
    pub revoke: bool,
}

pub(crate) fn run(args: TrustArgs) -> Result<()> {
    let root = resolve(args.path.as_deref()).context("that folder doesn't exist")?;
    let trusted = crate::persist::store()?.trusted_folders();
    if !args.revoke {
        trusted.add(&root)?;
        println!(
            "Trusted {}. Aster now loads its .env, aster.yaml, .mcp.json, and plugins in full.",
            root.display()
        );
        return Ok(());
    }
    trusted.remove(&root)?;
    match RepoTrust::of(Some(&root)) {
        RepoTrust::Untrusted => println!(
            "{} is no longer trusted. Its .env, MCP servers, plugins, and permission changes are off.",
            root.display()
        ),
        RepoTrust::Trusted => println!(
            "{} is still trusted because a folder above it is. Run `aster trust --revoke` on that folder.",
            root.display()
        ),
    }
    Ok(())
}
