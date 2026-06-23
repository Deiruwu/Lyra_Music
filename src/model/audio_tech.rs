use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use crate::audio::decoder::probe_file;
use crate::audio::errors::decode_error::DecodeError;
use crate::model::Track;
// --- DOMINIO TÉCNICO ---------------------------------------------------------

/// Huella técnica extraída directamente del archivo físico por Symphonia.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioProperties {
    pub sample_rate: u32,       // ej: 44100 o 48000
    pub channels: u8,           // ej: 1 (Mono), 2 (Stereo)
    pub bit_depth: Option<u8>,  // ej: 16 o 24 bits (Option porque algunos codecs no lo reportan)
    pub codec: String,          // ej: "aac", "alac", "flac"
    pub duration_secs: Option<u64>,
}

// --- DOMINIO DE INTEGRACIÓN --------------------------------------------------

/// La unión entre la Base de Datos y el Archivo Físico.
/// Esto es lo que viaja por tus canales hacia la cola de reproducción.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayableTrack {
    pub track: Track,
    pub audio: AudioProperties,
}

impl PlayableTrack {

    pub fn new(mut track: Track) -> Result<Self, DecodeError> {
        let raw_path_str = track
            .file_path
            .take()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| DecodeError::MissingFilePath(track.id.clone()))?;

        let original_path = Path::new(&raw_path_str);

        let resolved_path: PathBuf = match (
            std::env::var("MUSIC_SERVER_PATH").ok(),
            std::env::var("MUSIC_LOCAL_PATH").ok(),
        ) {
            (Some(server), Some(local)) if !server.is_empty() && !local.is_empty() => {
                match original_path.strip_prefix(&server) {
                    Ok(tail) => Path::new(&local).join(tail),
                    Err(_) => original_path.to_path_buf(),
                }
            }
            _ => original_path.to_path_buf(),
        };

        if !resolved_path.exists() {
            return Err(DecodeError::FileNotFound(resolved_path));
        }

        track.file_path = Some(resolved_path.to_string_lossy().to_string());

        probe_file(resolved_path, track)
    }
}