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
        let path = track.file_path.clone().expect("Deberías tener una ruta valida");

        let path = match (
            std::env::var("MUSIC_SERVER_PATH").ok(),
            std::env::var("MUSIC_LOCAL_PATH").ok(),
        ) {
            (Some(server), Some(local)) if !server.is_empty() && !local.is_empty() => {
                path.replace(&server, &local)
            }
            _ => path,
        };

        track.file_path = Some(path.clone());
        probe_file(path, track.clone())
    }
}