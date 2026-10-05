use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rtrb::{Consumer, RingBuffer};
use crossbeam_channel::Receiver;
use std::sync::atomic::Ordering;
use std::collections::VecDeque;
use std::sync::Arc;
use std::thread;
use std::time::Instant;

use crate::audio::decoder::{AudioDecoder, ChannelMode, SymphoniaDecoder, TARGET_CHANNELS, TARGET_SAMPLE_RATE};
use crate::model::audio_tech::PlayableTrack;
use crate::audio::engine_state::{AudioCommand, EngineState};

pub struct AudioEngine {
    _stream: cpal::Stream,
    pub controller_tx: crossbeam_channel::Sender<AudioCommand>,
    pub state: Arc<EngineState>,
}

impl AudioEngine {
    pub fn start() -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host.default_output_device()
            .ok_or("No se encontró dispositivo de salida de audio (PipeWire/Pulse)")?;

        let config = cpal::StreamConfig {
            channels: TARGET_CHANNELS as u16,
            sample_rate: TARGET_SAMPLE_RATE,
            buffer_size: cpal::BufferSize::Default,
        };

        // 1. Instanciar el canal de comandos (IPC)
        let (tx, rx) = crossbeam_channel::unbounded();
        let state = Arc::new(EngineState::new());

        // 2. Crear el Ring Buffer Lock-Free
        // 192,000 floats = 2 segundos exactos de buffer a 48kHz Estéreo.
        // Ocupa menos de 1 MB en RAM. Cero alocaciones dinámicas a partir de aquí.
        let (producer, mut consumer) = RingBuffer::<f32>::new(RING_CAPACITY);

        // 3. Levantar el Hilo Worker (El Productor de Symphonia)
        let worker_state = Arc::clone(&state);
        thread::Builder::new()
            .name("trackmanager_decoder_worker".into())
            .spawn(move || {
                run_worker_loop(rx, producer, worker_state);
            })
            .map_err(|e| format!("Fallo al crear hilo worker: {}", e))?;

        // 4. Levantar el Hilo de Hardware (El Consumidor CPAL)
        let cpal_state = Arc::clone(&state);

        // cpal empieza a sondear el stream (snd_pcm_avail_delay) apenas se
        // construye, y en sistemas con PipeWire el nodo puede tardar hasta
        // ~1-2s en terminar de conectarse en el grafo. Durante esa ventana
        // el plugin ALSA de PipeWire devuelve I/O error (EIO) en vez de
        // silenciarlo, aunque el audio real nunca se ve afectado. Es
        // cosmético y no se repite después del arranque, así que se
        // silencia solo acá; pasada la ventana, un error sí es real.
        let stream_start = std::time::Instant::now();

        let stream = device.build_output_stream(
            config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                // EL HOT-PATH: Nada de bloqueos.
                write_audio_to_hardware(data, &mut consumer, &cpal_state);
            },
            move |err| {
                if stream_start.elapsed() > std::time::Duration::from_secs(2) {
                    eprintln!("[CPAL ERROR] Stream de hardware roto: {}", err);
                }
            },
            None,
        ).map_err(|e| format!("Fallo al construir stream: {}", e))?;

        stream.play().map_err(|e| format!("Fallo al iniciar stream: {}", e))?;

        Ok(Self {
            _stream: stream,
            controller_tx: tx,
            state,
        })
    }
}


fn write_audio_to_hardware(output_buffer: &mut [f32], consumer: &mut Consumer<f32>, state: &EngineState) {
    if state.flush_flag.load(Ordering::Acquire) {
        while consumer.pop().is_ok() {}
        state.flush_flag.store(false, Ordering::Release);
    }

    if state.status.load(Ordering::Relaxed) != 1 {
        output_buffer.fill(0.0);
        return;
    }

    let volume = f32::from_bits(state.volume_bits.load(Ordering::Relaxed));
    let mut consumed = 0u64; // Ahora es u64 para evitar desbordamientos y coincidir con el state

    for sample in output_buffer.iter_mut() {
        match consumer.pop() {
            Ok(s) => {
                *sample = s * volume;
                consumed += 1;
            }
            Err(_) => {
                *sample = 0.0;
            }
        }
    }

    if consumed > 0 {
        state.add_consumed_samples(consumed);
    }
}

// --- EL HILO WORKER (SYMPHONIA) ---

/// Capacidad del ring buffer, en floats (2 s de estéreo a 48 kHz).
const RING_CAPACITY: usize = 192_000;
/// Floats que se empujan por vuelta (holgado para cualquier chunk del resampler).
const CHUNK_SAMPLES: usize = 16_384;
/// Una transición nunca dura menos que esto, aunque a la canción le quede menos.
const MIN_CROSSFADE_FRAMES: u64 = TARGET_SAMPLE_RATE as u64 / 2;
/// Por debajo de esto (≈ −48 dBFS) se cuenta como silencio al principio y al final.
const SILENCE_THRESHOLD: f32 = 0.004;
/// Lo más que se salta de silencio al principio de la que entra.
const MAX_LEADING_SILENCE_FRAMES: u64 = 8 * TARGET_SAMPLE_RATE as u64;
/// Cuánto más allá de la transición se decodifica la que termina, para encontrar dónde acaba su sonido.
const TAIL_LOOKAHEAD_FRAMES: u64 = 10 * TARGET_SAMPLE_RATE as u64;

/// Una canción abierta con lo ya decodificado que todavía no salió.
struct Voice {
    decoder: SymphoniaDecoder,
    pending: VecDeque<f32>,
    eof: bool,
    decoded_frames: u64,
    /// Duración conocida (del archivo o del catálogo), para saber cuándo empezar a fundir.
    duration_frames: Option<u64>,
    /// Silencio inicial saltado (la posición de la canción arranca después de él).
    leading_silence_frames: u64,
}

impl Voice {
    fn open(track: &PlayableTrack, mode: ChannelMode) -> Result<Self, String> {
        let path = track.track.file_path.as_deref().ok_or("Track sin file_path")?;
        let decoder = SymphoniaDecoder::open(path, mode).map_err(|e| e.to_string())?;
        let seconds = track
            .audio
            .duration_secs
            .filter(|&s| s > 0)
            .or((track.track.duration_seconds > 0).then_some(track.track.duration_seconds as u64));
        Ok(Self {
            decoder,
            pending: VecDeque::new(),
            eof: false,
            decoded_frames: 0,
            duration_frames: seconds.map(|s| s * TARGET_SAMPLE_RATE as u64),
            leading_silence_frames: 0,
        })
    }

    /// Decodifica hasta tener `frames` cuadros pendientes o llegar al final.
    fn fill(&mut self, frames: u64) {
        while (self.pending.len() as u64) < frames * TARGET_CHANNELS as u64 && !self.eof {
            match self.decoder.decode_next() {
                Ok(Some(chunk)) => self.pending.extend(chunk),
                Ok(None) => self.eof = true,
                Err(e) => {
                    eprintln!("[WORKER] Error de decodificación: {}", e);
                    self.eof = true;
                }
            }
        }
    }

    /// Descarta el silencio del principio (hasta `MAX_LEADING_SILENCE_FRAMES`).
    fn skip_leading_silence(&mut self) {
        while self.leading_silence_frames < MAX_LEADING_SILENCE_FRAMES {
            self.fill(TARGET_SAMPLE_RATE as u64 / 10);
            let frames = self.pending.len() / TARGET_CHANNELS;
            if frames == 0 {
                return;
            }
            let silent = silent_prefix_frames(self.pending.make_contiguous());
            self.pending.drain(..silent * TARGET_CHANNELS);
            self.leading_silence_frames += silent as u64;
            self.decoded_frames += silent as u64;
            if silent < frames {
                return;
            }
        }
    }

    /// Cuadros que le quedan con sonido (sin el silencio final), si el final ya está a menos de
    /// `lookahead` cuadros; decodifica por adelantado lo necesario para saberlo.
    fn audible_remaining(&mut self, lookahead: u64) -> Option<u64> {
        self.fill(lookahead);
        if !self.eof {
            return None;
        }
        Some(audible_frames(self.pending.make_contiguous()) as u64)
    }

    /// Corta lo que queda después de `frames` cuadros (el silencio final).
    fn truncate_to(&mut self, frames: u64) {
        self.pending.truncate(frames as usize * TARGET_CHANNELS);
    }

    fn leading_silence_ms(&self) -> u32 {
        (self.leading_silence_frames * 1000 / TARGET_SAMPLE_RATE as u64) as u32
    }

    /// Hasta `samples` floats (pares estéreo), decodificando lo que haga falta.
    fn take(&mut self, samples: usize) -> Vec<f32> {
        self.fill((samples / TARGET_CHANNELS) as u64);
        let count = samples.min(self.pending.len()) / TARGET_CHANNELS * TARGET_CHANNELS;
        self.decoded_frames += (count / TARGET_CHANNELS) as u64;
        self.pending.drain(..count).collect()
    }

    fn remaining_frames(&self) -> Option<u64> {
        self.duration_frames.map(|total| total.saturating_sub(self.decoded_frames))
    }

    fn is_finished(&self) -> bool {
        self.eof && self.pending.is_empty()
    }

    fn seek(&mut self, target: std::time::Duration) -> Result<(), String> {
        self.decoder.seek(target).map_err(|e| e.to_string())?;
        self.pending.clear();
        self.eof = false;
        self.decoded_frames = (target.as_secs_f64() * TARGET_SAMPLE_RATE as f64) as u64;
        Ok(())
    }
}

/// Transición en curso: la saliente baja mientras la actual (la nueva) sube.
struct Crossfade {
    outgoing: Option<Voice>,
    done_frames: u64,
    total_frames: u64,
}

fn flush(state: &EngineState) {
    state.flush_flag.store(true, Ordering::Release);
    while state.flush_flag.load(Ordering::Acquire) {
        thread::yield_now();
    }
}

fn run_worker_loop(
    command_rx: Receiver<AudioCommand>,
    mut producer: rtrb::Producer<f32>,
    state: Arc<EngineState>,
) {
    let mut current: Option<Voice> = None;
    let mut prepared: Option<Voice> = None;
    let mut crossfade: Option<Crossfade> = None;
    // Cuándo avisar al supervisor que ya suena la canción nueva (cuando sale del buffer), y
    // desde qué posición (el silencio inicial que se saltó).
    let mut announce_at: Option<(Instant, u32)> = None;

    loop {
        // RECEPCIÓN DE COMANDOS INTELIGENTE:
        // Si hay una canción sonando, usamos try_recv() para no bloquear el hilo.
        // Si no hay nada sonando, recv() DUERME el hilo hasta que llegue un comando (0% CPU).
        let cmd = if current.is_some() && state.status.load(Ordering::Relaxed) == 1 {
            command_rx.try_recv().ok()
        } else {
            command_rx.recv().ok()
        };

        if let Some(command) = cmd {
            match command {
                AudioCommand::Play { track, mode } => {
                    // Orden correcto: detener → limpiar → cargar → reproducir.
                    // Si cargamos el decoder ANTES del flush, CPAL puede limpiar
                    // samples del track nuevo pensando que son basura del anterior.
                    state.status.store(0, Ordering::Relaxed);
                    flush(&state);
                    (prepared, crossfade, announce_at) = (None, None, None);
                    state.next_prepared.store(false, Ordering::Release);

                    match Voice::open(&track, mode) {
                        Ok(voice) => {
                            current = Some(voice);
                            state.set_position_anchor(0);
                            state.status.store(1, Ordering::Relaxed);
                        }
                        Err(e) => {
                            eprintln!("[WORKER] Falla al abrir archivo: {}", e);
                            current = None;
                            state.status.store(0, Ordering::Relaxed);
                        }
                    }
                }
                AudioCommand::Load { track, mode, position } => {
                    state.status.store(0, Ordering::Relaxed);
                    flush(&state);
                    (prepared, crossfade, announce_at) = (None, None, None);
                    state.next_prepared.store(false, Ordering::Release);

                    match Voice::open(&track, mode) {
                        Ok(mut voice) => {
                            let start = if voice.seek(position).is_ok() { position } else { std::time::Duration::ZERO };
                            current = Some(voice);
                            state.set_position_anchor(start.as_millis() as u32);
                            state.status.store(2, Ordering::Relaxed);
                        }
                        Err(e) => eprintln!("[WORKER] Falla al abrir archivo: {}", e),
                    }
                }
                AudioCommand::Pause => {
                    state.status.store(2, Ordering::Relaxed);
                }
                AudioCommand::Resume => {
                    let current_ms = state.get_position().as_millis() as u32;
                    state.set_position_anchor(current_ms);
                    state.status.store(1, Ordering::Relaxed);
                }
                AudioCommand::Stop => {
                    (current, prepared, crossfade, announce_at) = (None, None, None, None);
                    state.next_prepared.store(false, Ordering::Release);
                    state.status.store(0, Ordering::Relaxed);
                    flush(&state);
                }
                AudioCommand::Seek(target) => {
                    if let Some(voice) = &mut current {
                        if let Err(e) = voice.seek(target) {
                            eprintln!("[WORKER] Falla al hacer seek: {}", e);
                        } else {
                            // Saltar corta la transición; si la nueva todavía no se había anunciado, ya suena.
                            crossfade = None;
                            if announce_at.take().is_some() {
                                state.next_prepared.store(false, Ordering::Release);
                                state.announce_crossfade(None);
                            }
                            flush(&state);
                            state.set_position_anchor(target.as_millis() as u32);
                        }
                    }
                }
                AudioCommand::SetVolume(v) => {
                    state.volume_bits.store(v.to_bits(), Ordering::Relaxed);
                }
                AudioCommand::PrepareNext(track) => {
                    prepared = track.and_then(|track| {
                        let mut voice = Voice::open(&track, ChannelMode::Stereo)
                            .map_err(|e| eprintln!("[WORKER] No se pudo preparar la siguiente: {}", e))
                            .ok()?;
                        voice.skip_leading_silence();
                        Some(voice)
                    });
                    state.next_prepared.store(prepared.is_some(), Ordering::Release);
                }
            }
        }

        if let Some((at, position_ms)) = announce_at
            && Instant::now() >= at
        {
            announce_at = None;
            // Recién ahora deja de haber "siguiente preparada": antes, el supervisor la volvería a mandar.
            state.next_prepared.store(false, Ordering::Release);
            state.announce_crossfade(Some(position_ms));
        }

        // EXTRACCIÓN Y LLENADO DEL BUFFER
        if state.status.load(Ordering::Relaxed) != 1 || current.is_none() {
            continue;
        }
        if producer.slots() < CHUNK_SAMPLES {
            thread::sleep(std::time::Duration::from_millis(5));
            continue;
        }

        // Cerca del final, se decodifica por adelantado la que termina para saber dónde acaba su
        // sonido; cuando eso queda a menos que la transición, entra la siguiente encima.
        let crossfade_frames = state.crossfade_ms.load(Ordering::Relaxed) as u64 * TARGET_SAMPLE_RATE as u64 / 1000;
        let lookahead = crossfade_frames + TAIL_LOOKAHEAD_FRAMES;
        if crossfade.is_none()
            && crossfade_frames > 0
            && prepared.is_some()
            && let Some(voice) = current.as_mut()
            && (voice.eof || voice.remaining_frames().is_some_and(|remaining| remaining <= lookahead))
            && let Some(audible) = voice.audible_remaining(lookahead)
            && audible <= crossfade_frames
            && let Some(next) = prepared.take()
        {
            // El silencio final de la que sale no suena: la mezcla dura lo que le queda de sonido.
            voice.truncate_to(audible);
            let position_ms = next.leading_silence_ms();
            let outgoing = current.replace(next);
            crossfade = Some(Crossfade { outgoing, done_frames: 0, total_frames: audible.max(MIN_CROSSFADE_FRAMES) });

            // La nueva empieza a sonar detrás de lo que ya está en el buffer: ahí se anuncia.
            let buffered = RING_CAPACITY - producer.slots();
            let delay = std::time::Duration::from_secs_f64(buffered as f64 / (TARGET_SAMPLE_RATE as f64 * TARGET_CHANNELS as f64));
            announce_at = Some((Instant::now() + delay, position_ms));
        }

        let Some(voice) = current.as_mut() else { continue };
        let mut samples = voice.take(CHUNK_SAMPLES);

        if let Some(fade) = &mut crossfade {
            let mut outgoing = fade.outgoing.as_mut().map(|v| v.take(CHUNK_SAMPLES)).unwrap_or_default();
            let frames = samples.len().max(outgoing.len()) / TARGET_CHANNELS;
            samples.resize(frames * TARGET_CHANNELS, 0.0);
            outgoing.resize(frames * TARGET_CHANNELS, 0.0);

            mix_crossfade(&mut samples, &outgoing, fade.done_frames, fade.total_frames);

            fade.done_frames += frames as u64;
            if fade.outgoing.as_ref().is_some_and(Voice::is_finished) {
                fade.outgoing = None;
            }
            if fade.done_frames >= fade.total_frames {
                crossfade = None;
            }
        }

        for sample in samples {
            let _ = producer.push(sample);
        }

        if current.as_ref().is_some_and(Voice::is_finished) && crossfade.is_none() {
            eprintln!("[WORKER] Decoder terminó, esperando que el buffer se vacíe...");
            // Esperamos a que CPAL consuma todo lo que queda
            while producer.slots() < RING_CAPACITY - 1 {
                thread::sleep(std::time::Duration::from_millis(10));
            }
            eprintln!("[WORKER] Duración real medida: {}ms", state.get_position().as_millis());
            current = None;
            state.status.store(3, Ordering::Relaxed);
        }
    }
}

fn is_silent(frame: &[f32]) -> bool {
    frame.iter().all(|sample| sample.abs() < SILENCE_THRESHOLD)
}

/// Cuadros en silencio al principio de `samples` (estéreo intercalado).
fn silent_prefix_frames(samples: &[f32]) -> usize {
    samples.chunks_exact(TARGET_CHANNELS).take_while(|frame| is_silent(frame)).count()
}

/// Cuadros hasta el último con sonido (lo que sigue es silencio final).
fn audible_frames(samples: &[f32]) -> usize {
    samples.chunks_exact(TARGET_CHANNELS).rposition(|frame| !is_silent(frame)).map_or(0, |last| last + 1)
}

/// Mezcla la entrante (`incoming`, se sobrescribe) con la saliente, `done` cuadros dentro de
/// una transición de `total`. Curvas seno/coseno: la potencia total se mantiene pareja.
fn mix_crossfade(incoming: &mut [f32], outgoing: &[f32], done: u64, total: u64) {
    for (frame, (new, old)) in incoming.chunks_exact_mut(TARGET_CHANNELS).zip(outgoing.chunks_exact(TARGET_CHANNELS)).enumerate() {
        let progress = ((done + frame as u64) as f32 / total.max(1) as f32).min(1.0);
        let angle = progress * std::f32::consts::FRAC_PI_2;
        let (fade_in, fade_out) = (angle.sin(), angle.cos());
        for (n, o) in new.iter_mut().zip(old) {
            *n = (*n * fade_in + o * fade_out).clamp(-1.0, 1.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{audible_frames, mix_crossfade, silent_prefix_frames};

    #[test]
    fn encuentra_el_silencio_del_principio_y_del_final() {
        let samples = [0.0, 0.001, 0.0, 0.0, 0.3, -0.2, 0.0, 0.5, 0.002, 0.0, 0.0, 0.0];
        assert_eq!(silent_prefix_frames(&samples), 2);
        assert_eq!(audible_frames(&samples), 4);
        assert_eq!(silent_prefix_frames(&[0.0; 6]), 3);
        assert_eq!(audible_frames(&[0.0; 6]), 0);
    }

    fn mixed_at(done: u64, total: u64) -> (f32, f32) {
        // Entrante en 0.5, saliente en 0.25: así se ve cuánto aporta cada una.
        let mut only_new = vec![0.5, 0.5];
        mix_crossfade(&mut only_new, &[0.0, 0.0], done, total);
        let mut only_old = vec![0.0, 0.0];
        mix_crossfade(&mut only_old, &[0.25, 0.25], done, total);
        (only_new[0] / 0.5, only_old[0] / 0.25)
    }

    #[test]
    fn la_transicion_va_de_la_saliente_a_la_entrante() {
        let (new_gain, old_gain) = mixed_at(0, 100);
        assert!(new_gain.abs() < 1e-6 && (old_gain - 1.0).abs() < 1e-6);

        let (new_gain, old_gain) = mixed_at(100, 100);
        assert!((new_gain - 1.0).abs() < 1e-6 && old_gain.abs() < 1e-6);
    }

    #[test]
    fn a_mitad_de_la_transicion_la_potencia_se_mantiene() {
        let (new_gain, old_gain) = mixed_at(50, 100);
        assert!((new_gain * new_gain + old_gain * old_gain - 1.0).abs() < 1e-5);
        assert!((new_gain - old_gain).abs() < 1e-5);
    }
}
