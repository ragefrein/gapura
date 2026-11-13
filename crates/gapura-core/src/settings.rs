//! Non-sensitive app settings. These live as plain JSON in the config dir
//! (`~/.config/gapura/settings.json` on Linux), separate from the vault.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{GapuraError, Result};

pub const SETTINGS_FILE_NAME: &str = "settings.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    Dark,
    Light,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: Theme,
    pub terminal_font_family: String,
    pub terminal_font_size: u8,
    pub scrollback_lines: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::Dark,
            terminal_font_family: "JetBrains Mono".to_string(),
            terminal_font_size: 14,
            scrollback_lines: 10_000,
        }
    }
}

pub fn settings_path(config_dir: &Path) -> PathBuf {
    config_dir.join(SETTINGS_FILE_NAME)
}

pub fn load(config_dir: &Path) -> Result<Settings> {
    let path = settings_path(config_dir);
    if !path.is_file() {
        return Ok(Settings::default());
    }
    let text = fs::read_to_string(&path).map_err(|e| GapuraError::PathIo {
        path: path.clone(),
        message: e.to_string(),
    })?;
    Ok(serde_json::from_str(&text)?)
}

pub fn save(config_dir: &Path, settings: &Settings) -> Result<()> {
    fs::create_dir_all(config_dir).map_err(|e| GapuraError::PathIo {
        path: config_dir.to_path_buf(),
        message: e.to_string(),
    })?;
    let path = settings_path(config_dir);
    let text = serde_json::to_string_pretty(settings)?;
    fs::write(&path, text).map_err(|e| GapuraError::PathIo {
        path,
        message: e.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_roundtrip_through_disk() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        assert_eq!(load(dir).unwrap(), Settings::default());

        let custom = Settings {
            theme: Theme::Light,
            terminal_font_family: "Cascadia Code".into(),
            terminal_font_size: 16,
            scrollback_lines: 50_000,
        };
        save(dir, &custom).unwrap();
        assert_eq!(load(dir).unwrap(), custom);
    }

    #[test]
    fn unknown_fields_do_not_break_loading() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        fs::write(
            settings_path(dir),
            r#"{"theme":"dark","terminal_font_family":"x","terminal_font_size":12,"scrollback_lines":100,"future_thing":1}"#,
        )
        .unwrap();
        let s = load(dir).unwrap();
        assert_eq!(s.theme, Theme::Dark);
    }
}
