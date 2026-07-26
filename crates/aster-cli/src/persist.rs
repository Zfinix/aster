use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use aster_persist::{SessionWriter, Store};

pub type Recorder = Arc<Mutex<SessionWriter>>;

pub fn home() -> Result<PathBuf> {
    let dir = dirs::config_dir()
        .context("could not determine a config directory for this platform")?
        .join("aster");
    Ok(dir)
}

pub fn store() -> Result<Store> {
    Store::open(home()?)
}
