use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use std::time::Duration;
use crate::audio::decoder::ChannelMode;
use crate::model::audio_tech::PlayableTrack;

// --- COMANDOS IPC ---
pub enum AudioCommand {
    Play {
        track: Arc<PlayableTrack>,
        mode: ChannelMode,
    },
    Pause,
    Resume,
    Stop,
    Seek(Duration),
    SetVolume(f32),
}

// --- ESTADO COMPARTIDO LOCK-FREE ---
pub struct EngineState {
    // 0 = Stopped, 1 = Playing, 2 = Paused
    pub status: AtomicU8,
    // Volumen almacenado como bits (u32) para poder usar operaciones atómicas
    pub volume_bits: AtomicU32,
    // Posición actual de la canción en milisegundos
    pub position_ms: AtomicU32,
    pub flush_flag: AtomicBool,
}

impl EngineState {
    /// Inicializa el estado base del motor.
    /// Detenido, con volumen al 100% (1.0) y en la posición cero.
    pub fn new() -> Self {
        Self {
            status: AtomicU8::new(0), // 0 = Stopped
            volume_bits: AtomicU32::new(1.0f32.to_bits()), // 1.0 en representación binaria f32
            position_ms: AtomicU32::new(0),
            flush_flag: AtomicBool::new(false),
        }
    }

    // --- MÉTODOS UTILERÍA (Para consumo limpio desde la UI/MPRIS) ---

    /// Devuelve el volumen actual como un f32 real listo para usar.
    pub fn get_volume(&self) -> f32 {
        f32::from_bits(self.volume_bits.load(Ordering::Relaxed))
    }

    /// Devuelve la posición actual convertida en un Duration estándar.
    pub fn get_position(&self) -> Duration {
        Duration::from_millis(self.position_ms.load(Ordering::Relaxed) as u64)
    }

    pub fn is_playing(&self) -> bool {
        self.status.load(Ordering::Relaxed) == 1
    }
}