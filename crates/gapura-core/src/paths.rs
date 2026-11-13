//! Platform data and config directories (design doc, section 2.1).

use std::fs;
use std::path::PathBuf;

use crate::error::{GapuraError, Result};

/// `~/.local/share/gapura` on Linux, `%APPDATA%\gapura` on Windows.
/// Holds `vault.json.enc`, `known_hosts.json`, and `gapura.log`.
pub fn data_dir() -> Result<PathBuf> {
    let base = dirs::data_dir()
        .ok_or_else(|| GapuraError::Other("cannot resolve the user data directory".into()))?;
    Ok(base.join("gapura"))
}

/// `~/.config/gapura` on Linux, `%APPDATA%\gapura` on Windows.
/// Holds `settings.json`.
pub fn config_dir() -> Result<PathBuf> {
    let base = dirs::config_dir()
        .ok_or_else(|| GapuraError::Other("cannot resolve the user config directory".into()))?;
    Ok(base.join("gapura"))
}

/// Create a directory (and parents) if missing, return its path.
pub fn ensure_dir(path: &PathBuf) -> Result<PathBuf> {
    if !path.is_dir() {
        fs::create_dir_all(path).map_err(|e| GapuraError::PathIo {
            path: path.clone(),
            message: e.to_string(),
        })?;
    }
    Ok(path.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dirs_resolve_without_panicking() {
        // Not every environment (CI, containers) has XDG paths; the function
        // must return a clean error instead of panicking.
        if let Ok(data) = data_dir() {
            assert!(data.ends_with("gapura"));
        }
        if let Ok(config) = config_dir() {
            assert!(config.ends_with("gapura"));
        }
    }
}
