use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use std::sync::atomic::Ordering;

use crossbeam_channel::Sender;
use tokio::sync::broadcast;
use crate::model::audio_tech::PlayableTrack;
use crate::model::Track;
use crate::audio::decoder::ChannelMode;
use crate::audio::engine_state::{AudioCommand, EngineState};
use crate::audio::engine::AudioEngine;
use crate::audio::track_event::{QueueEvent, TrackEvent};

const HISTORY_CAP: usize = 100;

pub struct TrackManager {
    engine_tx: Sender<AudioCommand>,
    pub state: Arc<EngineState>,

    current_track: Arc<Mutex<Option<Arc<PlayableTrack>>>>,
    queue: Arc<Mutex<VecDeque<Arc<Track>>>>,

    history: Arc<Mutex<VecDeque<Track>>>,

    auto_advance: Arc<Mutex<bool>>,

    pub event_tx: broadcast::Sender<TrackEvent>,
    pub queue_tx: broadcast::Sender<QueueEvent>,
}

impl TrackManager {
    pub fn new() -> Result<(Self, AudioEngine), String> {
        let engine = AudioEngine::start()?;

        let engine_tx = engine.controller_tx.clone();
        let state = Arc::clone(&engine.state);

        let queue = Arc::new(Mutex::new(VecDeque::<Arc<Track>>::new()));
        let current_track: Arc<Mutex<Option<Arc<PlayableTrack>>>> = Arc::new(Mutex::new(None));
        let history = Arc::new(Mutex::new(VecDeque::<Track>::new()));
        let auto_advance = Arc::new(Mutex::new(true));

        let supervisor_state = Arc::clone(&state);
        let supervisor_tx = engine_tx.clone();
        let supervisor_queue = Arc::clone(&queue);
        let supervisor_current = Arc::clone(&current_track);
        let supervisor_history = Arc::clone(&history);
        let supervisor_advance = Arc::clone(&auto_advance);

        let (event_tx, _) = broadcast::channel(16);
        let (queue_tx, _) = broadcast::channel(16);
        let event_tx_supervisor = event_tx.clone();
        let queue_tx_supervisor = queue_tx.clone();

        thread::Builder::new()
            .name("trackmanager_supervisor".into())
            .spawn(move || {
                loop {
                    thread::sleep(Duration::from_millis(500));

                    let status = supervisor_state.status.load(Ordering::Relaxed);
                    let can_advance = *supervisor_advance.lock().unwrap();

                    if status == 3 && can_advance {
                        let mut q = supervisor_queue.lock().unwrap();

                        if let Some(next_track) = q.pop_front() {
                            drop(q);

                            let playable = match PlayableTrack::new((*next_track).clone()) {
                                Ok(p) => Arc::new(p),
                                Err(e) => {
                                    eprintln!("[SUPERVISOR] No se pudo probear '{}': {:?}", next_track.id, e);
                                    continue;
                                }
                            };

                            if let Some(current) = supervisor_current.lock().unwrap().as_ref() {
                                push_to_history(&supervisor_history, current.track.clone());
                            }

                            *supervisor_current.lock().unwrap() = Some(Arc::clone(&playable));
                            let _ = event_tx_supervisor.send(TrackEvent::TrackChanged(Arc::clone(&playable)));
                            let _ = queue_tx_supervisor.send(QueueEvent::QueueChanged);
                            let _ = supervisor_tx.send(AudioCommand::Play {
                                track: playable,
                                mode: ChannelMode::Stereo,
                            });
                        } else {
                            let mut current_guard = supervisor_current.lock().unwrap();
                            if let Some(current) = current_guard.as_ref() {
                                push_to_history(&supervisor_history, current.track.clone());
                            }
                            *current_guard = None;

                            supervisor_state.status.store(0, Ordering::Relaxed);

                            let _ = event_tx_supervisor.send(TrackEvent::Stopped);
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
            history,
            auto_advance,
            event_tx,
            queue_tx,
        };

        Ok((manager, engine))
    }

    fn broadcast_queue_update(&self) {
        let _ = self.queue_tx.send(QueueEvent::QueueChanged);
    }

    // ── Historial ─────────────────────────────────────────────────────────────

    pub fn skip_prev(&self) -> Result<(), String> {
        let prev_track = {
            let mut h = self.history.lock().unwrap();
            h.pop_back()
        };

        match prev_track {
            None => Err("No hay tracks anteriores en el historial".to_string()),
            Some(track) => {
                let playable = PlayableTrack::new(track)
                    .map_err(|e| format!("Error al re-probear el track anterior: {:?}", e))?;

                let arc_prev = Arc::new(playable);

                {
                    let current_guard = self.current_track.lock().unwrap();
                    if let Some(current) = current_guard.as_ref() {
                        let mut q = self.queue.lock().unwrap();
                        q.push_front(Arc::new(current.track.clone()));
                    }
                }

                *self.auto_advance.lock().unwrap() = true;
                *self.current_track.lock().unwrap() = Some(Arc::clone(&arc_prev));

                let _ = self.event_tx.send(TrackEvent::TrackChanged(Arc::clone(&arc_prev)));
                self.broadcast_queue_update();

                let _ = self.engine_tx.send(AudioCommand::Play {
                    track: arc_prev,
                    mode: ChannelMode::Stereo,
                });

                Ok(())
            }
        }
    }

    pub fn get_history_snapshot(&self) -> Vec<Track> {
        self.history.lock().unwrap().iter().cloned().collect()
    }

    pub fn history_len(&self) -> usize {
        self.history.lock().unwrap().len()
    }

    pub fn clear_history(&self) {
        self.history.lock().unwrap().clear();
    }

    // ── Cola ──────────────────────────────────────────────────────────────────

    /// Pone una pista inmediatamente, borrando lo que esté sonando.
    pub fn play_now(&self, track: Track) {
        if let Some(current) = self.current_track.lock().unwrap().as_ref() {
            push_to_history(&self.history, current.track.clone());
        }

        let playable = match PlayableTrack::new(track) {
            Ok(p) => Arc::new(p),
            Err(e) => {
                eprintln!("[MANAGER] No se pudo probear track en play_now: {:?}", e);
                return;
            }
        };

        *self.auto_advance.lock().unwrap() = true;
        *self.current_track.lock().unwrap() = Some(Arc::clone(&playable));
        let _ = self.event_tx.send(TrackEvent::TrackChanged(Arc::clone(&playable)));
        let _ = self.engine_tx.send(AudioCommand::Play {
            track: playable,
            mode: ChannelMode::Stereo,
        });
    }

    /// Encola un track. Si el reproductor está parado, arranca inmediatamente.
    pub fn enqueue(&self, track: Track) {
        {
            let mut q = self.queue.lock().unwrap();
            q.push_back(Arc::new(track));
        }

        self.broadcast_queue_update();

        if self.state.status.load(Ordering::Relaxed) == 0 {
            self.skip_next();
        }
    }

    /// Encola un track al frente de la cola.
    pub fn enqueue_front(&self, track: Track) {
        {
            let mut q = self.queue.lock().unwrap();
            q.push_front(Arc::new(track));
        }

        self.broadcast_queue_update();

        if self.state.status.load(Ordering::Relaxed) == 0 {
            self.skip_next();
        }
    }

    /// Encola solo si el ID no aparece en current, cola ni historial.
    pub fn enqueue_deduplicated(&self, track: Track) -> bool {
        {
            if let Some(current) = self.current_track.lock().unwrap().as_ref() {
                if current.track.id == track.id {
                    return false;
                }
            }

            let q = self.queue.lock().unwrap();
            if q.iter().any(|t| t.id == track.id) {
                return false;
            }

            let h = self.history.lock().unwrap();
            if h.iter().any(|t| t.id == track.id) {
                return false;
            }
        }

        self.enqueue(track);
        true
    }

    pub fn skip_next(&self) {
        let next_track = {
            let mut q = self.queue.lock().unwrap();
            q.pop_front()
        };

        if let Some(track) = next_track {
            let playable = match PlayableTrack::new((*track).clone()) {
                Ok(p) => Arc::new(p),
                Err(e) => {
                    eprintln!("[MANAGER] No se pudo probear track en skip_next: {:?}", e);
                    return;
                }
            };

            if let Some(current) = self.current_track.lock().unwrap().as_ref() {
                push_to_history(&self.history, current.track.clone());
            }

            *self.auto_advance.lock().unwrap() = true;
            *self.current_track.lock().unwrap() = Some(Arc::clone(&playable));

            let _ = self.event_tx.send(TrackEvent::TrackChanged(Arc::clone(&playable)));
            self.broadcast_queue_update();

            let _ = self.engine_tx.send(AudioCommand::Play {
                track: playable,
                mode: ChannelMode::Stereo,
            });
        } else {
            self.stop();
        }
    }

    pub fn skip_to_index(&self, index: usize) -> Result<(), String> {
        let mut q = self.queue.lock().unwrap();

        if index >= q.len() {
            return Err(format!(
                "Índice de cola inválido: intentó saltar al elemento {}, pero la cola solo tiene longitud {}",
                index,
                q.len()
            ));
        }

        {
            let mut h = self.history.lock().unwrap();

            if let Some(current) = self.current_track.lock().unwrap().as_ref() {
                push_to_history_inner(&mut h, current.track.clone());
            }

            for skipped in q.iter().take(index) {
                push_to_history_inner(&mut h, (**skipped).clone());
            }
        }

        q.drain(0..index);

        if let Some(next_track) = q.pop_front() {
            drop(q);

            let playable = match PlayableTrack::new((*next_track).clone()) {
                Ok(p) => Arc::new(p),
                Err(e) => return Err(format!("No se pudo probear el track destino: {:?}", e)),
            };

            *self.auto_advance.lock().unwrap() = true;
            *self.current_track.lock().unwrap() = Some(Arc::clone(&playable));

            let _ = self.event_tx.send(TrackEvent::TrackChanged(Arc::clone(&playable)));
            self.broadcast_queue_update();

            let _ = self.engine_tx.send(AudioCommand::Play {
                track: playable,
                mode: ChannelMode::Stereo,
            });

            Ok(())
        } else {
            Err("La cola se vació inesperadamente durante la extracción".to_string())
        }
    }

    pub fn move_in_queue(&self, from: usize, to: usize) -> Result<(), String> {
        let mut q = self.queue.lock().unwrap();
        if from >= q.len() || to >= q.len() {
            return Err("Índice fuera de rango".into());
        }
        let track = q.remove(from).unwrap();
        q.insert(to, track);
        drop(q);
        self.broadcast_queue_update();
        Ok(())
    }

    pub fn remove_from_queue(&self, index: usize) -> Result<(), String> {
        let mut q = self.queue.lock().unwrap();

        if index >= q.len() {
            return Err("Índice fuera de rango al intentar eliminar de la cola".into());
        }

        q.remove(index);
        drop(q);
        self.broadcast_queue_update();
        Ok(())
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
        if let Some(current) = self.current_track.lock().unwrap().as_ref() {
            push_to_history(&self.history, current.track.clone());
        }

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

    pub fn get_current_track(&self) -> Arc<Mutex<Option<Arc<PlayableTrack>>>> {
        Arc::clone(&self.current_track)
    }

    pub fn get_queue_snapshot(&self) -> Vec<Arc<Track>> {
        self.queue.lock().unwrap().iter().cloned().collect()
    }
}

// ── Helpers privados ──────────────────────────────────────────────────────────

fn push_to_history(history: &Arc<Mutex<VecDeque<Track>>>, track: Track) {
    let mut h = history.lock().unwrap();
    push_to_history_inner(&mut h, track);
}

fn push_to_history_inner(h: &mut VecDeque<Track>, track: Track) {
    if h.len() >= HISTORY_CAP {
        h.pop_front();
    }
    h.push_back(track);
}