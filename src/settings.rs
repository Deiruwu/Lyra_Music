use std::env;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

use crate::audio::manager::manager::RepeatMode;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub sidebar_expanded: bool,
    pub shuffle_enabled: bool,
    pub repeat_mode: RepeatMode,
    pub volume: f32,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            sidebar_expanded: false,
            shuffle_enabled: false,
            repeat_mode: RepeatMode::Off,
            volume: 1.0,
        }
    }
}

/// `$XDG_CONFIG_HOME/atelier` si está definido, o `~/.config/atelier` por
/// defecto. Mismo patrón que `ui::utils::data_dir::data_dir`, pero para
/// `.config` en vez de `.local/share`.
fn config_dir() -> PathBuf {
    if let Some(xdg) = env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(xdg).join("atelier");
    }
    if let Some(home) = env::var_os("HOME") {
        return PathBuf::from(home).join(".config").join("atelier");
    }
    PathBuf::from(".").join("atelier")
}

fn settings_path() -> PathBuf {
    config_dir().join("settings.json")
}

impl AppSettings {
    pub fn load() -> Self {
        std::fs::read_to_string(settings_path())
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(config_dir())?;
        let raw = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(settings_path(), raw)
    }
}
