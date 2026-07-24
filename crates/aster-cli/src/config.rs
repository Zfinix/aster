use std::env;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

// TODO: swap to the dedicated Aster Review App once registered (device flow enabled).
pub const APP_CLIENT_ID: &str = "Iv23liF94XkDt4xUdpih";

pub const APP_INSTALL_URL: &str = "https://github.com/apps/aster-review/installations/new";

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Stored {
    pub github_token: Option<String>,
}

fn config_dir() -> Result<PathBuf> {
    let dir = dirs::config_dir()
        .context("could not determine a config directory for this platform")?
        .join("aster");
    Ok(dir)
}

fn token_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("credentials.json"))
}

pub fn load() -> Stored {
    let Ok(path) = token_path() else {
        return Stored::default();
    };
    let Ok(bytes) = fs::read(path) else {
        return Stored::default();
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

pub fn store_token(token: &str) -> Result<()> {
    let dir = config_dir()?;
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let stored = Stored {
        github_token: Some(token.to_string()),
    };
    let path = token_path()?;
    let bytes = serde_json::to_vec_pretty(&stored)?;

    // On Unix, create the file with 0o600 from the start so the token is never
    // briefly world-readable between write and chmod.
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&path)
            .with_context(|| format!("writing {}", path.display()))?;
        // Narrow an existing file that predates this mode too.
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
            .with_context(|| format!("securing {}", path.display()))?;
        f.write_all(&bytes)
            .with_context(|| format!("writing {}", path.display()))?;
    }
    #[cfg(not(unix))]
    {
        fs::write(&path, &bytes).with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(())
}

pub fn clear_token() -> Result<()> {
    let path = token_path()?;
    match fs::remove_file(&path) {
        Ok(()) => println!("Logged out."),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => println!("Not logged in."),
        Err(e) => return Err(e).context("removing stored credentials"),
    }
    Ok(())
}

pub fn resolve_github_token(flag: Option<&str>) -> Option<String> {
    if let Some(t) = flag {
        return Some(t.to_string());
    }
    if let Ok(t) = env::var("GITHUB_TOKEN")
        && !t.trim().is_empty()
    {
        return Some(t);
    }
    load().github_token
}
