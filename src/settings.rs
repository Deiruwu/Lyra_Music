use std::env;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

use crate::audio::manager::manager::RepeatMode;
use crate::ui::search_feature::search_bar::SearchFilter;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub sidebar_expanded: bool,
    pub shuffle_enabled: bool,
    pub repeat_mode: RepeatMode,
    pub volume: f32,
    pub radio_enabled: bool,
    /// Último filtro elegido en el buscador.
    pub search_filter: SearchFilter,
    /// Columnas de depuración (reproducciones / última vez) en el Explorador.
    pub explorer_play_stats: bool,
    /// Playlists habilitadas en Remix.
    pub remix_playlists: Vec<String>,
    /// Acento elegido en Ajustes (`#rrggbb`); `None` = el violeta de fábrica.
    pub accent_color: Option<String>,
    /// Transición entre canciones (fundir el final de una con el inicio de la siguiente).
    pub crossfade_enabled: bool,
    /// Duración de la transición, en segundos (se recuerda aunque esté apagada).
    pub crossfade_seconds: f32,
    /// Dónde corre track_manager (se aplica al reiniciar).
    pub server: ServerSettings,
}

/// Local: atelier lanza su propio track_manager. Remote: se conecta a uno ya desplegado.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServerMode {
    Local,
    Remote,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerSettings {
    pub mode: ServerMode,
    /// Host y puerto del servidor remoto.
    pub host: String,
    pub port: u16,
}

impl Default for ServerSettings {
    /// Remoto si el `.env` define `TRACK_MANAGER_HOST`; si no, local.
    fn default() -> Self {
        let port = env::var("TRACK_MANAGER_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(7878);
        match env::var("TRACK_MANAGER_HOST") {
            Ok(host) if !host.is_empty() => Self { mode: ServerMode::Remote, host, port },
            _ => Self { mode: ServerMode::Local, host: String::new(), port },
        }
    }
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            sidebar_expanded: false,
            shuffle_enabled: false,
            repeat_mode: RepeatMode::Off,
            volume: 1.0,
            radio_enabled: false,
            search_filter: SearchFilter::default(),
            explorer_play_stats: false,
            remix_playlists: Vec::new(),
            accent_color: None,
            crossfade_enabled: false,
            crossfade_seconds: 6.0,
            server: ServerSettings::default(),
        }
    }
}

/// `$XDG_CONFIG_HOME/atelier` si está definido, o `~/.config/atelier` por
/// defecto. Mismo patrón que `ui::utils::data_dir::data_dir`, pero para
/// `.config` en vez de `.local/share`.
pub(crate) fn config_dir() -> PathBuf {
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
