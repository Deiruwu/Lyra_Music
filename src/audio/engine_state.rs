use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::time::{Duration, Instant};
use crate::audio::decoder::{ChannelMode, TARGET_SAMPLE_RATE, TARGET_CHANNELS};
use crate::model::audio_tech::PlayableTrack;

/// `crossfade_position_ms` sin posición que fijar.
const NO_ANCHOR: u32 = u32::MAX;

// ---  Comandos IPC ---
pub enum AudioCommand {
    Play {
        track: Arc<PlayableTrack>,
        mode: ChannelMode,
    },
    /// Carga la pista en `position` y la deja en pausa (restaurar sesión).
    Load {
        track: Arc<PlayableTrack>,
        mode: ChannelMode,
        position: Duration,
    },
    Pause,
    Resume,
    Stop,
    Seek(Duration),
    SetVolume(f32),
    /// La que sigue en la cola, abierta de antemano para fundirla con el final de la
    /// actual (`None` = no hay o la transición está apagada).
    PrepareNext(Option<Arc<PlayableTrack>>),
}

// --- ESTADO COMPARTIDO LOCK-FREE ---
//
// Tabla de estados del motor:
//
//   0 = Stopped   — sin pista, silencio.
//   1 = Playing   — decodificando y enviando audio al hardware.
//   2 = Paused    — pista cargada, pero el hardware emite silencio.
//   3 = Finished  — el decoder llegó al final; el supervisor leerá este
//                   valor y avanzará a la próxima pista de la cola.
//   4 = Downloading — la siguiente pista no tiene archivo local todavía;
//                   el DownloadWorker está resolviendo la descarga.
//                   El supervisor espera hasta que vuelva ha estado 3.
//
pub struct EngineState {
    pub status: AtomicU8,
    // Volumen almacenado como bits (u32) para poder usar operaciones atómicas
    pub volume_bits: AtomicU32,
    /// Total histórico de muestras individuales (floats) entregadas al hardware
    /// desde el último Play/Seek. Es un contador exacto sin pérdida por división.
    pub total_samples_consumed: AtomicU64,
    /// Posición (ms) en el momento en que se fijó el ancla actual
    /// (último Play, Seek, o Resume).
    position_anchor_ms: AtomicU32,
    /// Instante de reloj de pared en que se fijó el ancla, en nanos
    /// desde el arranque del proceso (process_start).
    anchor_instant_nanos: AtomicI64,
    /// Referencia común para convertir Instant -> i64 de forma atómica.
    process_start: Instant,
    pub flush_flag: AtomicBool,
    /// Duración de la transición entre canciones, en ms (0 = apagada).
    pub crossfade_ms: AtomicU32,
    /// El motor ya pasó (fundiendo) a la canción preparada; el supervisor pone al
    /// día cola e historial sin volver a mandar `Play` (ver `take_crossfade`).
    crossfaded: AtomicBool,
    /// Posición de la canción nueva al anunciarla (silencio inicial saltado), o
    /// `NO_ANCHOR` si ya se fijó (un salto durante el cruce).
    crossfade_position_ms: AtomicU32,
    /// Cuándo se anunció, para sumar lo que sonó hasta que el supervisor lo atiende.
    crossfade_announced_nanos: AtomicI64,
    /// El motor tiene abierta una siguiente para la transición (la descartan `Play`,
    /// `Load`, `Stop` y el propio cruce).
    pub next_prepared: AtomicBool,
}

impl EngineState {
    /// Inicializa el estado base del motor.
    /// Detenido, con volumen al 100% (1.0) y en la posición cero.
    pub fn new() -> Self {
        Self {
            status: AtomicU8::new(0),
            volume_bits: AtomicU32::new(1.0f32.to_bits()),
            total_samples_consumed: AtomicU64::new(0),
            position_anchor_ms: AtomicU32::new(0),
            anchor_instant_nanos: AtomicI64::new(0),
            process_start: Instant::now(),
            flush_flag: AtomicBool::new(false),
            crossfade_ms: AtomicU32::new(0),
            crossfaded: AtomicBool::new(false),
            crossfade_position_ms: AtomicU32::new(NO_ANCHOR),
            crossfade_announced_nanos: AtomicI64::new(0),
            next_prepared: AtomicBool::new(false),
        }
    }

    fn now_nanos(&self) -> i64 {
        self.process_start.elapsed().as_nanos() as i64
    }

    /// Fija el ancla de tiempo: "en este instante, la posición es `ms`".
    /// Llamar en Play (con 0), Seek (con el target), y Resume (con la
    /// posición que tenía al pausar).
    pub fn set_position_anchor(&self, ms: u32) {
        self.position_anchor_ms.store(ms, Ordering::Relaxed);
        self.anchor_instant_nanos.store(self.now_nanos(), Ordering::Relaxed);

        // Convertimos los milisegundos del ancla a muestras y reseteamos el contador
        let samples = (ms as u64 * TARGET_SAMPLE_RATE as u64 * TARGET_CHANNELS as u64) / 1000;
        self.total_samples_consumed.store(samples, Ordering::Relaxed);
    }

    /// La canción nueva de la transición ya suena, desde `position_ms` (o `None` si su
    /// posición ya se fijó por un salto).
    pub fn announce_crossfade(&self, position_ms: Option<u32>) {
        self.crossfade_position_ms.store(position_ms.unwrap_or(NO_ANCHOR), Ordering::Relaxed);
        self.crossfade_announced_nanos.store(self.now_nanos(), Ordering::Relaxed);
        self.crossfaded.store(true, Ordering::Release);
    }

    /// Para el supervisor: `Some` si el motor pasó a la canción preparada, con la posición
    /// que tiene ahora (lo anunciado más lo que sonó desde entonces) si hay que fijarla.
    pub fn take_crossfade(&self) -> Option<Option<u32>> {
        if !self.crossfaded.swap(false, Ordering::AcqRel) {
            return None;
        }
        let position = self.crossfade_position_ms.load(Ordering::Relaxed);
        if position == NO_ANCHOR {
            return Some(None);
        }
        let since_ms = (self.now_nanos() - self.crossfade_announced_nanos.load(Ordering::Relaxed)).max(0) / 1_000_000;
        Some(Some(position + since_ms as u32))
    }

    /// Congela el ancla en la posición actual sin tocar el reloj —
    /// llamar en Pause, para que get_position() deje de avanzar.
    pub fn freeze_position_anchor(&self) {
        let frozen = self.get_position().as_millis() as u32;
        self.position_anchor_ms.store(frozen, Ordering::Relaxed);
        self.anchor_instant_nanos.store(self.now_nanos(), Ordering::Relaxed);
    }

    /// Suma muestras efectivamente entregadas al hardware (llamado desde
    /// el hot path de CPAL). Cero divisiones, solo suma entera pura.
    pub fn add_consumed_samples(&self, delta_samples: u64) {
        self.total_samples_consumed.fetch_add(delta_samples, Ordering::Relaxed);
    }

    /// Devuelve el volumen actual como un f32 real listo para usar.
    pub fn get_volume(&self) -> f32 {
        f32::from_bits(self.volume_bits.load(Ordering::Relaxed))
    }

    /// Devuelve la posición actual convertida en un Duration estándar.
    pub fn get_position(&self) -> Duration {
        let anchor_ms = self.position_anchor_ms.load(Ordering::Relaxed) as i64;
        let anchor_nanos = self.anchor_instant_nanos.load(Ordering::Relaxed);
        let elapsed_ms = (self.now_nanos() - anchor_nanos) / 1_000_000;

        let wall_clock_ms = anchor_ms + elapsed_ms.max(0);

        // La división a milisegundos se hace aquí sobre el acumulador gigante.
        // La pérdida por truncamiento es de máximo 1ms para TODA la canción.
        let total_samples = self.total_samples_consumed.load(Ordering::Relaxed);
        let consumed_ceiling_ms = ((total_samples * 1000) / (TARGET_SAMPLE_RATE as u64 * TARGET_CHANNELS as u64)) as i64;

        let position_ms = wall_clock_ms.min(consumed_ceiling_ms).max(0);

        Duration::from_millis(position_ms as u64)
    }

    pub fn is_playing(&self) -> bool {
        self.status.load(Ordering::Relaxed) == 1
    }

    /// True cuando el motor está esperando a que el DownloadWorker termine
    /// de bajar la siguiente pista antes de poder reproducirla.
    pub fn is_downloading(&self) -> bool {
        self.status.load(Ordering::Relaxed) == 4
    }
}