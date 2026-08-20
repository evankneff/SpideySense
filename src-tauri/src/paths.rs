//! Resolves where the config and logs live.
//!
//! Two modes, checked in this order:
//!   1. Portable  - a `config.toml` sitting next to the .exe wins. Logs go to `logs/` beside it.
//!   2. Installed - `%APPDATA%\frigate-popup\config.toml`, logs in `%APPDATA%\frigate-popup\logs\`.
//!
//! Portable mode is opt-in purely by the file existing, so an installed copy never
//! accidentally picks up a stray file from a download folder.

use anyhow::{Context, Result};
use std::path::PathBuf;

pub const APP_DIR_NAME: &str = "frigate-popup";
pub const CONFIG_FILE_NAME: &str = "config.toml";

/// Directory holding the running executable.
fn exe_dir() -> Result<PathBuf> {
    let exe = std::env::current_exe().context("could not determine the executable path")?;
    exe.parent()
        .map(PathBuf::from)
        .context("executable path has no parent directory")
}

/// `%APPDATA%\frigate-popup`
fn appdata_dir() -> Result<PathBuf> {
    let base = dirs::config_dir()
        .context("could not determine the user config directory (is %APPDATA% set?)")?;
    Ok(base.join(APP_DIR_NAME))
}

/// True when a `config.toml` sits next to the executable.
pub fn is_portable() -> bool {
    exe_dir()
        .map(|d| d.join(CONFIG_FILE_NAME).is_file())
        .unwrap_or(false)
}

/// The root directory for this install's config and logs.
pub fn root_dir() -> Result<PathBuf> {
    if is_portable() {
        exe_dir()
    } else {
        appdata_dir()
    }
}

pub fn config_path() -> Result<PathBuf> {
    Ok(root_dir()?.join(CONFIG_FILE_NAME))
}

pub fn log_dir() -> Result<PathBuf> {
    Ok(root_dir()?.join("logs"))
}

/// Newest file in the log directory, used by the tray's "Open log file" item.
pub fn newest_log_file() -> Result<Option<PathBuf>> {
    let dir = log_dir()?;
    if !dir.is_dir() {
        return Ok(None);
    }
    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in std::fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let modified = entry.metadata()?.modified()?;
        if newest.as_ref().is_none_or(|(t, _)| modified > *t) {
            newest = Some((modified, entry.path()));
        }
    }
    Ok(newest.map(|(_, p)| p))
}
