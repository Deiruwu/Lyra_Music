use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use std::sync::atomic::Ordering;

use crossbeam_channel::Sender;
use tokio::sync::broadcast;
use crate::model::audio_tech::PlayableTrack;
use crate::audio::decoder::ChannelMode;
use crate::audio::engine_state::{AudioCommand, EngineState};
use crate::audio::engine::AudioEngine;
use crate::audio::track_event::TrackEvent;

pub struct TrackManager {
    engine_tx: Sender<AudioCommand>,
    pub state: Arc<EngineState>,

    // La cola de reproducción y el historial están protegidos por Mutex 
    // porque tu interfaz gráfica y el hilo supervisor accederán a ellos a la vez.
    current_track: Arc<Mutex<Option<Arc<PlayableTrack>>>>,
    queue: Arc<Mutex<VecDeque<Arc<PlayableTrack>>>>,

    // Bandera para evitar que el auto-play se dispare si el usuario dio "Stop" manualmente
    auto_advance: Arc<Mutex<bool>>,

    pub event_tx: broadcast::Sender<TrackEvent>,
}

impl TrackManager {
    pub fn new() -> Result<(Self, AudioEngine), String> {
        let engine = AudioEngine::start()?;

        let engine_tx = engine.controller_tx.clone();
        let state = Arc::clone(&engine.state);

        let queue = Arc::new(Mutex::new(VecDeque::<Arc<PlayableTrack>>::new()));
        let current_track: Arc<Mutex<Option<Arc<PlayableTrack>>>> = Arc::new(Mutex::new(None));
        let auto_advance = Arc::new(Mutex::new(true));

        // 2. El Hilo Supervisor (El Perro Guardián)
        // Este hilo vigila si la canción terminó naturalmente para poner la siguiente.
        let supervisor_state = Arc::clone(&state);
        let supervisor_tx = engine_tx.clone();
        let supervisor_queue = Arc::clone(&queue);
        let supervisor_current = Arc::clone(&current_track);
        let supervisor_advance = Arc::clone(&auto_advance);

        let (event_tx, _) = broadcast::channel(16);
        let event_tx_supervisor = event_tx.clone();

        thread::Builder::new()
            .name("trackmanager_supervisor".into())
            .spawn(move || {
                loop {
                    thread::sleep(Duration::from_millis(500)); // Chequeo relajado cada medio segundo

                    let status = supervisor_state.status.load(Ordering::Relaxed);
                    let can_advance = *supervisor_advance.lock().unwrap();

                    // Si el estado es 3 (Finished) y el auto-advance está activo, significa
                    // que Symphonia llegó al final del archivo por sí sola y es seguro avanzar.
                    if status == 3 && can_advance {
                        let mut q = supervisor_queue.lock().unwrap();

                        if let Some(next_track) = q.pop_front() {
                            *supervisor_current.lock().unwrap() = Some(next_track.clone());
                            let _ = event_tx_supervisor.send(TrackEvent::TrackChanged(next_track.clone()));
                            let _ = supervisor_tx.send(AudioCommand::Play {
                                track: next_track,
                                mode: ChannelMode::Stereo,
                            });
                        } else {
                            // La cola se vació, limpiamos el current_track
                            *supervisor_current.lock().unwrap() = None;

                            // CRÍTICO: Reiniciamos el estado a 0 (Stopped).
                            // Si no hacemos esto, el status se quedará en 3. En el próximo ciclo de 500ms,
                            // volverá a entrar a este if, volverá a bloquear el Mutex de la cola,
                            // y volverá a fallar. Crearías contención de locks por la eternidad.
                            supervisor_state.status.store(0, Ordering::Relaxed);
                        }
                    }
                }
            })
            .map_err(|e| format!("Fallo al crear hilo supervisor: {}", e))?;

        let manager = Self {
            engine_tx,
            state,
            queue,
            current_track,
            auto_advance,
            event_tx,
        };

        Ok((manager, engine))
    }

    // --- API PÚBLICA PARA TU UI / MAIN ---

    /// Pone una pista inmediatamente, borrando lo que esté sonando.
    pub fn play_now(&self, track: PlayableTrack) {
        let track = Arc::new(track); // <- envuelves aquí, una sola vez
        *self.auto_advance.lock().unwrap() = true;
        *self.current_track.lock().unwrap() = Some(Arc::clone(&track));
        let _ = self.event_tx.send(TrackEvent::TrackChanged(Arc::clone(&track)));
        let _ = self.engine_tx.send(AudioCommand::Play {
            track,
            mode: ChannelMode::Stereo,
        });
    }

    pub fn enqueue(&self, track: PlayableTrack) {
        let track = Arc::new(track); // <- envuelves aquí
        self.queue.lock().unwrap().push_back(track);

        if self.state.status.load(Ordering::Relaxed) == 0 {
            self.skip_next();
        }
    }

    pub fn skip_next(&self) {
        let mut q = self.queue.lock().unwrap();
        if let Some(next_track) = q.pop_front() { // next_track ya es Arc<PlayableTrack>
            *self.auto_advance.lock().unwrap() = true;
            *self.current_track.lock().unwrap() = Some(Arc::clone(&next_track));
            let _ = self.event_tx.send(TrackEvent::TrackChanged(Arc::clone(&next_track)));
            let _ = self.engine_tx.send(AudioCommand::Play {
                track: next_track, // AudioCommand espera PlayableTrack, desenvuelves
                mode: ChannelMode::Stereo,
            });
        } else {
            self.stop();
        }
    }

    pub fn pause(&self) {
        let _ = self.engine_tx.send(AudioCommand::Pause);
        let _ = self.event_tx.send(TrackEvent::Paused);
    }

    pub fn resume(&self) {
        let _ = self.engine_tx.send(AudioCommand::Resume);
        let _ = self.event_tx.send(TrackEvent::Resumed);
    }

    pub fn stop(&self) {
        *self.auto_advance.lock().unwrap() = false;
        *self.current_track.lock().unwrap() = None;
        let _ = self.event_tx.send(TrackEvent::Stopped);
        let _ = self.engine_tx.send(AudioCommand::Stop);
    }

    pub fn get_volume(&self) -> f32 {
        self.state.get_volume()
    }

    pub fn set_volume(&self, volume: f32) {
        let vol = volume.clamp(0.0, 1.0);
        let _ = self.engine_tx.send(AudioCommand::SetVolume(vol));
    }

    pub fn get_position(&self) -> Duration {
        self.state.get_position()
    }

    pub fn seek(&self, position: Duration) {
        let _ = self.engine_tx.send(AudioCommand::Seek(position));
    }

    /// Útil para que la UI sepa qué dibujar
    pub fn get_current_track(&self) -> Option<Arc<PlayableTrack>> {
        self.current_track.lock().unwrap().clone()
    }

    pub fn get_queue(&self) -> Vec<Arc<PlayableTrack>> {
        self.queue.lock().unwrap().iter().cloned().collect()
    }
}