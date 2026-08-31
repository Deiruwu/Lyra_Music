use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use std::sync::atomic::Ordering;

use crossbeam_channel::Sender;
use tokio::sync::broadcast;
use uuid::Uuid;
use crate::model::audio_tech::PlayableTrack;
use crate::model::Track;
use crate::audio::decoder::ChannelMode;
use crate::audio::engine_state::{AudioCommand, EngineState};
use crate::audio::engine::AudioEngine;
use crate::audio::manager::error_mananger::ManagerError;
use crate::audio::track_event::{QueueEvent, TrackEvent};
use crate::audio::queue_shuffle;

const HISTORY_CAP: usize = 100;

// ── Estado consolidado ───────────────────────────────────────────────────────

/// Entrada de la cola: identidad de slot (`id`) separada del `track.id`,
/// para que dos instancias de la misma canción en cola no colisionen
/// (animator, drag & drop, futuro undo de shuffle).
#[derive(Clone)]
pub struct QueueSlot {
    pub id: Uuid,
    pub track: Arc<Track>,
}

impl QueueSlot {
    pub fn new(track: Arc<Track>) -> Self {
        Self { id: Uuid::new_v4(), track }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum RepeatMode {
    #[default]
    Off,
    Queue,
    Track,
}

/// Vista desde la que se originó la reproducción actual.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaybackOrigin {
    Explorer,
    Favorites,
    Playlist(String),
    Album(String),
    Artist(String),
    Home,
}

/// Todo el estado mutable de reproducción vive aquí, detrás de un único
/// `Mutex`. Esto evita el patrón anterior de tomar y soltar locks separados
/// para `current_track`, `queue`, `history` y `auto_advance` por turnos
/// (lo cual dejaba ventanas donde el supervisor podía leer un estado
/// inconsistente entre un lock y el siguiente).
pub(super) struct PlaybackState {
    pub(super) current_track: Option<Arc<PlayableTrack>>,
    pub(super) queue: VecDeque<QueueSlot>,
    pub(super) history: VecDeque<Track>,
    pub(super) auto_advance: bool,
    pub(super) repeat_mode: RepeatMode,
    pub(super) shuffle_enabled: bool,
    pub(super) original_order: VecDeque<Uuid>,
}

impl PlaybackState {
    fn new() -> Self {
        Self {
            current_track: None,
            queue: VecDeque::new(),
            history: VecDeque::new(),
            auto_advance: true,
            repeat_mode: RepeatMode::Off,
            shuffle_enabled: false,
            original_order: VecDeque::new(),
        }
    }

    /// Agrega un slot a la cola, respetando el modo shuffle salvo que sea al frente.
    pub(super) fn queue_push(&mut self, slot: QueueSlot, to_front: bool) {
        if to_front {
            self.original_order.push_front(slot.id);
            self.queue.push_front(slot);
            return;
        }
        self.original_order.push_back(slot.id);
        if self.shuffle_enabled {
            queue_shuffle::insert_shuffled(&mut self.queue, slot);
        } else {
            self.queue.push_back(slot);
        }
    }

    /// Reemplaza toda la cola pendiente por `slots`, fijando el nuevo orden canónico.
    pub(super) fn refill_queue(&mut self, slots: Vec<QueueSlot>) {
        self.original_order = slots.iter().map(|s| s.id).collect();
        self.queue = if self.shuffle_enabled {
            queue_shuffle::shuffle(slots).into()
        } else {
            slots.into()
        };
    }

    /// Saca el slot al frente de la cola y lo desvincula del orden canónico.
    pub(super) fn pop_front_tracked(&mut self) -> Option<QueueSlot> {
        let slot = self.queue.pop_front()?;
        self.original_order.retain(|id| *id != slot.id);
        Some(slot)
    }

    /// Saca los primeros `n` slots de la cola y los desvincula del orden canónico.
    fn drain_front_tracked(&mut self, n: usize) -> Vec<QueueSlot> {
        let drained: Vec<QueueSlot> = self.queue.drain(0..n).collect();
        let ids: std::collections::HashSet<Uuid> = drained.iter().map(|s| s.id).collect();
        self.original_order.retain(|id| !ids.contains(id));
        drained
    }

    /// Empuja la pista actual (si hay) al historial y la reemplaza por
    /// `playable`. Usar esto para "cambiar de pista activa" siempre que
    /// la saliente deba archivarse (flujo normal hacia adelante).
    ///
    /// Si el caller YA decidió a dónde va la pista saliente (p.ej.
    /// `skip_prev`/`skip_to_history_index`, que la re-encolan a mano al
    /// frente de la cola), usar `set_current_track` en su lugar — de lo
    /// contrario la pista queda archivada DOS veces (acá y en la cola),
    /// lo que producía un ping-pong infinito entre las mismas dos
    /// canciones al presionar "anterior" repetidamente.
    pub(super) fn advance_to(&mut self, playable: Arc<PlayableTrack>) {
        if let Some(current) = self.current_track.take() {
            push_to_history_inner(&mut self.history, current.track.clone());
        }
        self.set_current_track(playable);
    }

    /// Fija la pista actual sin archivar la saliente en el historial —
    /// ver `advance_to` para cuándo usar cada una.
    pub(super) fn set_current_track(&mut self, playable: Arc<PlayableTrack>) {
        self.auto_advance = true;
        self.current_track = Some(playable);
    }

    /// Caso "el track al frente no tiene archivo local": lo devuelve a la
    /// cola, archiva lo que estaba sonando y marca auto_advance para que
    /// el supervisor retome en cuanto el DownloadWorker termine.
    fn requeue_for_download(&mut self, slot: QueueSlot) {
        self.original_order.push_front(slot.id);
        self.queue.push_front(slot);
        if let Some(current) = self.current_track.take() {
            push_to_history_inner(&mut self.history, current.track.clone());
        }
        self.auto_advance = true;
    }

    pub(super) fn clear_current_to_history(&mut self) {
        if let Some(current) = self.current_track.take() {
            push_to_history_inner(&mut self.history, current.track.clone());
        }
    }

    pub(super) fn reset_with_context(&mut self, before: Vec<Track>, remaining: Vec<QueueSlot>) {
        self.current_track = None;
        self.history.clear();
        for track in before {
            push_to_history_inner(&mut self.history, track);
        }
        self.refill_queue(remaining);
    }
}

fn push_to_history_inner(h: &mut VecDeque<Track>, track: Track) {
    if h.len() >= HISTORY_CAP {
        h.pop_front();
    }
    h.push_back(track);
}

/// Intenta "probear" un track (convertirlo en `PlayableTrack`). Centraliza
/// el `match PlayableTrack::new(...) { Ok/Err }` que antes se repetía en
/// `play_now`, `skip_next`, `skip_to_index` y el supervisor, cada uno con
/// su propio mensaje de log ligeramente distinto.
pub(super) fn probe_track(track: &Track, context: &str) -> Result<Arc<PlayableTrack>, ManagerError> {
    PlayableTrack::new(track.clone())
        .map(Arc::new)
        .map_err(|e| {
            eprintln!("[{}] No se pudo probear '{}': {:?}", context, track.id, e);
            ManagerError::ProbeFailed
        })
}

/// Marca el estado como "esperando descarga" (status 4) y notifica al
/// DownloadWorker vía `queue_tx`. Antes este bloque (log + store + send)
/// estaba copiado 3 veces con solo el `context` del log cambiando.
fn signal_download_required(
    engine_state: &EngineState,
    queue_tx: &broadcast::Sender<QueueEvent>,
    track: Arc<Track>,
    context: &str,
) {
    eprintln!(
        "[{}] \"{}\" sin archivo local, solicitando descarga de emergencia.",
        context, track.title
    );
    engine_state.status.store(4, Ordering::Relaxed);
    let _ = queue_tx.send(QueueEvent::DownloadRequired(track));
}

// ── TrackManager ──────────────────────────────────────────────────────────────

pub struct TrackManager {
    pub(super) engine_tx: Sender<AudioCommand>,
    pub state: Arc<EngineState>,

    pub(super) playback: Arc<Mutex<PlaybackState>>,

    origin: Mutex<Option<PlaybackOrigin>>,

    pub event_tx: broadcast::Sender<TrackEvent>,
    pub queue_tx: broadcast::Sender<QueueEvent>,
}

impl TrackManager {
    pub fn new() -> Result<(Self, AudioEngine), ManagerError> {
        let engine = AudioEngine::start().map_err(ManagerError::EngineStartFailed)?;

        let engine_tx = engine.controller_tx.clone();
        let state = Arc::clone(&engine.state);

        let playback = Arc::new(Mutex::new(PlaybackState::new()));
        let supervisor_playback = Arc::clone(&playback);
        let supervisor_state = Arc::clone(&state);
        let supervisor_tx = engine_tx.clone();

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

                    // Estado 4 = esperando descarga; el DownloadWorker reactivará
                    // el ciclo cuando termine (vuelve al estado 3).
                    if status != 3 {
                        continue;
                    }

                    let mut ps = supervisor_playback.lock().unwrap();
                    if !ps.auto_advance {
                        continue;
                    }

                    if ps.repeat_mode == RepeatMode::Track {
                        if let Some(current) = ps.current_track.clone() {
                            drop(ps);
                            let _ = supervisor_tx.send(AudioCommand::Play {
                                track: current,
                                mode: ChannelMode::Stereo,
                            });
                            continue;
                        }
                    }

                    if ps.queue.is_empty() && ps.repeat_mode == RepeatMode::Queue && !ps.history.is_empty() {
                        let replay: Vec<QueueSlot> = ps.history.drain(..).map(|t| QueueSlot::new(Arc::new(t))).collect();
                        ps.refill_queue(replay);
                    }

                    let Some(next_slot) = ps.pop_front_tracked() else {
                        ps.clear_current_to_history();
                        drop(ps);
                        supervisor_state.status.store(0, Ordering::Relaxed);
                        let _ = event_tx_supervisor.send(TrackEvent::Stopped);
                        continue;
                    };

                    if next_slot.track.file_path.is_none() {
                        let next_track = Arc::clone(&next_slot.track);
                        ps.requeue_for_download(next_slot);
                        drop(ps);
                        signal_download_required(
                            &supervisor_state,
                            &queue_tx_supervisor,
                            next_track,
                            "SUPERVISOR",
                        );
                        continue;
                    }

                    let playable = match probe_track(&next_slot.track, "SUPERVISOR") {
                        Ok(p) => p,
                        Err(_) => continue,
                    };

                    ps.advance_to(Arc::clone(&playable));
                    drop(ps);

                    let _ = event_tx_supervisor.send(TrackEvent::TrackChanged(Arc::clone(&playable)));
                    let _ = queue_tx_supervisor.send(QueueEvent::QueueChanged);
                    let _ = supervisor_tx.send(AudioCommand::Play {
                        track: playable,
                        mode: ChannelMode::Stereo,
                    });
                }
            })
            .map_err(ManagerError::SupervisorSpawnFailed)?;

        let manager = Self {
            engine_tx,
            state,
            playback,
            origin: Mutex::new(None),
            event_tx,
            queue_tx,
        };

        Ok((manager, engine))
    }

    pub(super) fn broadcast_queue_update(&self) {
        let _ = self.queue_tx.send(QueueEvent::QueueChanged);
    }

    pub(super) fn play_track(&self, track: Arc<PlayableTrack>) {
        let _ = self.engine_tx.send(AudioCommand::Play {
            track,
            mode: ChannelMode::Stereo,
        });
    }

    pub(super) fn enqueue_internal<I>(&self, tracks: I, to_front: bool)
    where
        I: IntoIterator<Item = Track>,
        I::IntoIter: DoubleEndedIterator,
    {
        let slots_iter = tracks.into_iter().map(|t| QueueSlot::new(Arc::new(t)));
        let mut added_any = false;

        {
            let mut ps = self.playback.lock().unwrap();
            if to_front {
                for slot in slots_iter.rev() {
                    ps.queue_push(slot, true);
                    added_any = true;
                }
            } else {
                for slot in slots_iter {
                    ps.queue_push(slot, false);
                    added_any = true;
                }
            }
        }

        if !added_any {
            return;
        }

        self.broadcast_queue_update();

        if self.state.status.load(Ordering::Relaxed) == 0 {
            self.skip_next();
        }
    }


    /// Saltar `n` elementos de la cola (archivándolos en el historial junto
    /// con la pista actual) y reproducir el que quede al frente.
    ///
    /// `skip_next()` y `skip_to_index(index)` son ambos casos particulares
    /// de esta operación (`n = 0` y `n = index` respectivamente) — antes
    /// eran dos métodos de ~50 líneas casi idénticas.
    pub(super) fn skip_internal(&self, n: usize) -> Result<(), ManagerError> {
        let next_track = {
            let mut ps = self.playback.lock().unwrap();

            if n > 0 {
                if n > ps.queue.len() {
                    return Err(ManagerError::InvalidQueueIndex { index: n, len: ps.queue.len() });
                }

                ps.clear_current_to_history();
                let skipped = ps.drain_front_tracked(n);
                for slot in skipped {
                    push_to_history_inner(&mut ps.history, (*slot.track).clone());
                }
            }

            if ps.queue.is_empty() && ps.repeat_mode == RepeatMode::Queue && !ps.history.is_empty() {
                let replay: Vec<QueueSlot> = ps.history.drain(..).map(|t| QueueSlot::new(Arc::new(t))).collect();
                ps.refill_queue(replay);
            }

            ps.pop_front_tracked()
        };

        let Some(next_slot) = next_track else {
            if n == 0 {
                self.stop();
                return Ok(());
            }
            return Err(ManagerError::QueueEmptiedUnexpectedly);
        };

        if next_slot.track.file_path.is_none() {
            let next_track = Arc::clone(&next_slot.track);
            {
                let mut ps = self.playback.lock().unwrap();
                ps.requeue_for_download(next_slot);
            }
            let _ = self.engine_tx.send(AudioCommand::Stop);
            signal_download_required(&self.state, &self.queue_tx, next_track, "MANAGER:skip");
            self.broadcast_queue_update();
            return Ok(());
        }

        let playable = probe_track(&next_slot.track, "MANAGER:skip")?;

        {
            let mut ps = self.playback.lock().unwrap();
            ps.advance_to(Arc::clone(&playable));
        }

        let _ = self.event_tx.send(TrackEvent::TrackChanged(Arc::clone(&playable)));
        self.broadcast_queue_update();
        self.play_track(playable);

        Ok(())
    }


    // ── gets de cola y track actual ─────────────────────────────────────────────

    pub fn get_current_track(&self) -> Option<Arc<PlayableTrack>> {
        self.playback.lock().unwrap().current_track.clone()
    }

    pub fn get_queue_snapshot(&self) -> Vec<QueueSlot> {
        self.playback.lock().unwrap().queue.iter().cloned().collect()
    }

    pub fn get_history_snapshot(&self) -> Vec<Track> {
        self.playback.lock().unwrap().history.iter().cloned().collect()
    }

    pub fn is_shuffled(&self) -> bool {
        self.playback.lock().unwrap().shuffle_enabled
    }

    pub fn repeat_mode(&self) -> RepeatMode {
        self.playback.lock().unwrap().repeat_mode
    }

    pub fn set_playback_origin(&self, origin: PlaybackOrigin) {
        *self.origin.lock().unwrap() = Some(origin);
    }

    pub fn get_playback_origin(&self) -> Option<PlaybackOrigin> {
        self.origin.lock().unwrap().clone()
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: &str) -> Track {
        Track {
            id: id.to_string(),
            title: id.to_string(),
            duration_seconds: 0,
            thumbnail_small: None,
            thumbnail_large: None,
            bpm: None,
            camelot_key: None,
            file_path: Some(format!("{id}.mp3")),
            added_at: None,
            state: Default::default(),
            liked: false,
            album: None,
            artists: vec![],
        }
    }

    fn playable(id: &str) -> Arc<PlayableTrack> {
        Arc::new(PlayableTrack {
            track: track(id),
            audio: crate::model::audio_tech::AudioProperties {
                sample_rate: 44100,
                channels: 2,
                bit_depth: None,
                codec: "test".to_string(),
                duration_secs: None,
            },
        })
    }

    fn slots(ids: &[&str]) -> Vec<QueueSlot> {
        ids.iter().map(|id| QueueSlot::new(Arc::new(track(id)))).collect()
    }

    #[test]
    fn reset_with_context_mid_list_populates_history_and_discards_old_current() {
        let mut ps = PlaybackState::new();
        ps.current_track = Some(playable("stale"));
        push_to_history_inner(&mut ps.history, track("old_history"));

        ps.reset_with_context(
            vec![track("a"), track("b"), track("c")],
            slots(&["e", "f"]),
        );

        assert!(ps.current_track.is_none());
        let hist: Vec<String> = ps.history.iter().map(|t| t.id.clone()).collect();
        assert_eq!(hist, vec!["a", "b", "c"]);
        let queue_ids: Vec<String> = ps.queue.iter().map(|s| s.track.id.clone()).collect();
        assert_eq!(queue_ids, vec!["e", "f"]);
    }

    #[test]
    fn reset_with_context_from_index_zero_leaves_history_empty() {
        let mut ps = PlaybackState::new();
        push_to_history_inner(&mut ps.history, track("old_history"));

        ps.reset_with_context(vec![], slots(&["b", "c"]));

        assert!(ps.history.is_empty());
    }

    #[test]
    fn reset_with_context_shuffle_path_clears_history_without_repopulating() {
        let mut ps = PlaybackState::new();
        ps.shuffle_enabled = true;
        push_to_history_inner(&mut ps.history, track("old_history"));

        ps.reset_with_context(vec![], slots(&["a", "b", "c"]));

        assert!(ps.history.is_empty());
        assert_eq!(ps.queue.len(), 3);
    }
}