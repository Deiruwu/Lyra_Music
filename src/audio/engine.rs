use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rtrb::{Consumer, RingBuffer};
use crossbeam_channel::Receiver;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;

use crate::audio::decoder::{SymphoniaDecoder, AudioDecoder, TARGET_SAMPLE_RATE, TARGET_CHANNELS};
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
            sample_rate: cpal::SampleRate(TARGET_SAMPLE_RATE),
            buffer_size: cpal::BufferSize::Default,
        };

        // 1. Instanciar el canal de comandos (IPC)
        let (tx, rx) = crossbeam_channel::unbounded();
        let state = Arc::new(EngineState::new());

        // 2. Crear el Ring Buffer Lock-Free
        // 192,000 floats = 2 segundos exactos de buffer a 48kHz Estéreo.
        // Ocupa menos de 1 MB en RAM. Cero alocaciones dinámicas a partir de aquí.
        let (producer, mut consumer) = RingBuffer::<f32>::new(192_000);

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

        let stream = device.build_output_stream(
            &config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                // EL HOT-PATH: Nada de bloqueos.
                write_audio_to_hardware(data, &mut consumer, &cpal_state);
            },
            |err| eprintln!("[CPAL ERROR] Stream de hardware roto: {}", err),
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

// --- EL HOT PATH (HILO DEL SISTEMA OPERATIVO) ---

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
    let mut consumed = 0u32;

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

    // Actualizar posición basado en samples REALMENTE reproducidos por el hardware
    // consumed / TARGET_CHANNELS = frames reales, a TARGET_SAMPLE_RATE

    if consumed > 0 {
        let delta_ms = (consumed as u64 * 1000 / (TARGET_SAMPLE_RATE as u64 * TARGET_CHANNELS as u64)) as u32;
        state.position_ms.fetch_add(delta_ms, Ordering::Relaxed);
    }

}

// --- EL HILO WORKER (SYMPHONIA) ---

fn run_worker_loop(
    command_rx: Receiver<AudioCommand>,
    mut producer: rtrb::Producer<f32>,
    state: Arc<EngineState>,
) {
    let mut current_decoder: Option<SymphoniaDecoder> = None;

    loop {
        // RECEPCIÓN DE COMANDOS INTELIGENTE:
        // Si hay una canción sonando, usamos try_recv() para no bloquear el hilo.
        // Si no hay nada sonando, recv() DUERME el hilo hasta que llegue un comando (0% CPU).
        let cmd = if current_decoder.is_some() && state.status.load(Ordering::Relaxed) == 1 {
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

                    state.flush_flag.store(true, Ordering::Release);
                    while state.flush_flag.load(Ordering::Acquire) {
                        thread::yield_now();
                    }

                    match track.track.file_path.as_deref() {
                        Some(path) => match SymphoniaDecoder::open(path, mode) {
                            Ok(dec) => {
                                current_decoder = Some(dec);
                                state.position_ms.store(0, Ordering::Relaxed);
                                state.status.store(1, Ordering::Relaxed);
                            }
                            Err(e) => {
                                eprintln!("[WORKER] Falla al abrir archivo: {}", e);
                                state.status.store(0, Ordering::Relaxed);
                            }
                        },
                        None => {
                            eprintln!("[WORKER] Track sin file_path");
                            state.status.store(0, Ordering::Relaxed);
                        }
                    }
                }
                AudioCommand::Pause => {
                    state.status.store(2, Ordering::Relaxed);
                }
                AudioCommand::Resume => {
                    state.status.store(1, Ordering::Relaxed);
                }
                AudioCommand::Stop => {
                    current_decoder = None;
                    state.status.store(0, Ordering::Relaxed);

                    state.flush_flag.store(true, Ordering::Release);
                    while state.flush_flag.load(Ordering::Acquire) {
                        thread::yield_now();
                    }
                }
                AudioCommand::Seek(target) => {
                    if let Some(dec) = &mut current_decoder {
                        if let Err(e) = dec.seek(target) {
                            eprintln!("[WORKER] Falla al hacer seek: {}", e);
                        } else {
                            state.flush_flag.store(true, Ordering::Release);
                            while state.flush_flag.load(Ordering::Acquire) {
                                thread::yield_now();
                            }
                            state.position_ms.store(target.as_millis() as u32, Ordering::Relaxed);
                        }
                    }
                }
                AudioCommand::SetVolume(v) => {
                    state.volume_bits.store(v.to_bits(), Ordering::Relaxed);
                }
            }
        }

        // EXTRACCIÓN Y LLENADO DEL BUFFER
        if state.status.load(Ordering::Relaxed) == 1 {
            if let Some(decoder) = &mut current_decoder {
                // Esperamos a tener suficiente espacio libre en el ring buffer.
                // 16384 es holgado para cualquier chunk de salida del resampler (max ~2048 frames * 2 canales * 2x margen).
                if producer.slots() >= 16384 {
                    match decoder.decode_next() {
                        Ok(Some(samples)) => {
                            for sample in samples {
                                let _ = producer.push(sample);
                            }
                        }
                        Ok(None) => {
                            eprintln!("[WORKER] Decoder terminó, esperando que el buffer se vacíe...");
                            // Esperamos a que CPAL consuma todo lo que queda
                            loop {
                                // El producer sabe cuánto espacio libre hay
                                // Si slots() == capacidad total, el buffer está vacío
                                if producer.slots() >= 192_000 - 1 {
                                    break;
                                }
                                thread::sleep(std::time::Duration::from_millis(10));
                            }
                            let real_duration_ms = state.position_ms.load(Ordering::Relaxed);
                            eprintln!("[WORKER] Duración real medida: {}ms", real_duration_ms);
                            current_decoder = None;
                            state.status.store(3, Ordering::Relaxed);
                        }
                        Err(e) => {
                            eprintln!("[WORKER] Error de decodificación: {}", e);
                            current_decoder = None;
                            state.status.store(0, Ordering::Relaxed);
                        }
                    }
                } else {
                    // Backpressure: el ring buffer está casi lleno, CPAL consumirá pronto.
                    thread::sleep(std::time::Duration::from_millis(5));
                }
            }
        }
    }
}