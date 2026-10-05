use std::path::PathBuf;
use std::sync::Mutex;
use serde::{Deserialize, Serialize};

use crate::audio::manager::manager::PlaybackOrigin;
use crate::model::Track;
use crate::settings::config_dir;

/// Serializa las escrituras: el autosave guarda desde un hilo aparte.
static SAVE_LOCK: Mutex<()> = Mutex::new(());

/// Lo que se estaba reproduciendo al cerrar, para retomarlo al abrir.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PlaybackSession {
    pub current: Option<Track>,
    pub position_ms: u64,
    pub queue: Vec<Track>,
    pub history: Vec<Track>,
    pub origin: Option<PlaybackOrigin>,
}

fn session_path() -> PathBuf {
    config_dir().join("session.json")
}

impl PlaybackSession {
    pub fn load() -> Self {
        std::fs::read_to_string(session_path())
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }

    /// Escribe a un temporal y lo renombra, para no dejar el archivo a medias si se corta.
    pub fn save(&self) -> std::io::Result<()> {
        let _guard = SAVE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::fs::create_dir_all(config_dir())?;
        let raw = serde_json::to_string(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let tmp = session_path().with_extension("json.tmp");
        std::fs::write(&tmp, raw)?;
        std::fs::rename(tmp, session_path())
    }
}
