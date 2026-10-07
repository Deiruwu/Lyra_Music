use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;
use crate::audio::engine_state::AudioCommand;
use crate::audio::manager::manager::{probe_track, QueueSlot, TrackManager};
use crate::audio::manager::playlist_link::PlaylistLink;
use crate::audio::manager::error_mananger::ManagerError;
use crate::audio::queue_shuffle;
use crate::audio::track_event::{QueueEvent, TrackEvent};
use crate::model::Track;

impl TrackManager {

    /// Reproduce `context_tracks[start_index]`, resetea current/queue/history
    /// desde cero y arma la cola con el resto.
    ///
    /// Sin shuffle: el historial se descarta y se repuebla con
    /// `context_tracks[0..start_index]` en orden. En shuffle, el historial
    /// también se descarta pero no se repuebla — esos tracks entran al
    /// sorteo junto con el resto de la vista (`refill_queue` ya baraja
    /// `remaining` cuando `shuffle_enabled`).
    pub fn play_context(&self, context_tracks: Vec<Track>, start_index: usize) {
        self.play_context_inner(context_tracks, start_index, None);
    }

    /// `play_context` que, con `link_to`, deja la cola ligada a esa playlist.
    pub(super) fn play_context_inner(&self, context_tracks: Vec<Track>, start_index: usize, link_to: Option<String>) {
        if start_index >= context_tracks.len() { return; }

        let shuffle_enabled = self.playback.lock().unwrap().shuffle_enabled;

        let link = link_to.map(|playlist_id| {
            let anchor = if shuffle_enabled { None } else { start_index.checked_sub(1).map(|i| context_tracks[i].id.clone()) };
            PlaylistLink::new(playlist_id, context_tracks.iter().cloned().map(Arc::new).collect(), anchor)
        });

        let (before, first, remaining_tracks): (Vec<Track>, Track, Vec<Track>) = if shuffle_enabled {
            let mut tracks = context_tracks;
            let first = tracks.remove(start_index);
            (Vec::new(), first, tracks)
        } else {
            let mut tracks = context_tracks;
            let remaining_tracks = tracks.split_off(start_index + 1);
            let first = tracks.pop().unwrap();
            (tracks, first, remaining_tracks)
        };

        let first_track = Arc::new(first);
        let remaining: Vec<QueueSlot> = remaining_tracks.into_iter().map(|t| QueueSlot::new(Arc::new(t))).collect();

        {
            let mut ps = self.playback.lock().unwrap();
            ps.reset_with_context(before, remaining);
            ps.link = link;
            ps.auto_advance = true;
        }

        if first_track.file_path.is_none() {
            {
                let mut ps = self.playback.lock().unwrap();
                ps.queue_push(QueueSlot::new(Arc::clone(&first_track)), true);
            }

            let _ = self.engine_tx.send(AudioCommand::Stop);

            self.state.status.store(4, Ordering::Relaxed);
            let _ = self.queue_tx.send(QueueEvent::DownloadRequired(first_track));
            self.broadcast_queue_update();
            return;
        }

        match probe_track(&first_track, "MANAGER:play_context") {
            Ok(playable) => {
                {
                    let mut ps = self.playback.lock().unwrap();
                    ps.advance_to(Arc::clone(&playable));
                    ps.track_started(&playable.track.id, false);
                }
                let _ = self.event_tx.send(TrackEvent::TrackChanged(Arc::clone(&playable)));
                self.broadcast_queue_update();
                self.play_track(playable);
            }
            Err(_) => {
                self.broadcast_queue_update();
                self.skip_next();
            }
        }
    }

    /// "Reproducir todo" sin track puntual elegido por el usuario (botón
    /// de playlist/álbum). En shuffle, sortea la lista ENTERA —incluido
    /// lo que sería el primer track en orden original— antes de decidir
    /// qué va primero, para no reproducir siempre el mismo track 0. Sin
    /// shuffle, es idéntico a `play_context(tracks, 0)`.
    pub fn play_context_shuffled(&self, context_tracks: Vec<Track>) {
        if context_tracks.is_empty() { return; }
        let shuffle_enabled = self.playback.lock().unwrap().shuffle_enabled;
        let ordered = if shuffle_enabled {
            queue_shuffle::shuffle_tracks(context_tracks)
        } else {
            context_tracks
        };
        self.play_context(ordered, 0);
    }

    pub fn play_now(&self, track: Track) {
        let playable = match probe_track(&track, "MANAGER:play_now") {
            Ok(p) => p,
            Err(_) => return,
        };

        {
            let mut ps = self.playback.lock().unwrap();
            ps.advance_to(Arc::clone(&playable));
        }

        let _ = self.event_tx.send(TrackEvent::TrackChanged(Arc::clone(&playable)));
        self.play_track(playable);
    }

    pub fn skip_next(&self) {
        let _ = self.skip_internal(0);
    }

    pub fn skip_to_index(&self, index: usize) -> Result<(), ManagerError> {
        self.skip_internal(index)
    }

    pub fn skip_prev(&self) -> Result<(), ManagerError> {
        let prev_track = {
            let mut ps = self.playback.lock().unwrap();
            ps.history.pop_back()
        };

        let track = prev_track.ok_or(ManagerError::NoHistory)?;
        let playable = probe_track(&track, "MANAGER:skip_prev")?;

        {
            let mut ps = self.playback.lock().unwrap();
            if let Some(current) = ps.current_track.as_ref() {
                let current_track = current.track.clone();
                ps.queue_push(QueueSlot::new(Arc::new(current_track)), true);
            }
            // NO advance_to: ya re-encolamos el saliente a mano arriba,
            // así que no debe archivarse también en el historial (ver
            // docstring de `set_current_track`) — de lo contrario queda
            // duplicado en cola+historial y "anterior" repetido se traba
            // alternando entre las mismas dos canciones.
            ps.set_current_track(Arc::clone(&playable));
            ps.track_started(&playable.track.id, false);
        }

        let _ = self.event_tx.send(TrackEvent::TrackChanged(Arc::clone(&playable)));
        self.broadcast_queue_update();
        self.play_track(playable);

        Ok(())
    }

    /// Salta directo a la canción que está `n` posiciones atrás de la
    /// actual en el historial (`n == 1` = la más reciente). Todo lo que
    /// quedó entre medio —incluida la canción que sonaba antes de llamar
    /// a esto— pasa al frente de la cola, en su orden original, para
    /// poder seguir avanzando desde ahí como si nada.
    pub fn skip_to_history_index(&self, n: usize) -> Result<(), ManagerError> {
        let (target, removed) = {
            let mut ps = self.playback.lock().unwrap();
            if n == 0 || n > ps.history.len() {
                return Err(ManagerError::InvalidHistoryIndex { index: n, len: ps.history.len() });
            }
            let removed: Vec<Track> = (0..n - 1).map(|_| ps.history.pop_back().unwrap()).collect();
            let target = ps.history.pop_back().unwrap();
            (target, removed)
        };

        let playable = probe_track(&target, "MANAGER:skip_to_history_index")?;

        {
            let mut ps = self.playback.lock().unwrap();
            // Orden: el current saliente primero, después lo removido del
            // historial (más reciente primero) — cada `push_front`
            // empuja al anterior hacia atrás, así que el resultado final
            // en la cola queda en orden cronológico correcto (el más
            // viejo de los removidos al frente, el current al final de
            // este grupo).
            if let Some(current) = ps.current_track.take() {
                ps.queue_push(QueueSlot::new(Arc::new(current.track.clone())), true);
            }
            for track in removed {
                ps.queue_push(QueueSlot::new(Arc::new(track)), true);
            }
            ps.set_current_track(Arc::clone(&playable));
            ps.track_started(&playable.track.id, false);
        }

        let _ = self.event_tx.send(TrackEvent::TrackChanged(Arc::clone(&playable)));
        self.broadcast_queue_update();
        self.play_track(playable);

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
        {
            let mut ps = self.playback.lock().unwrap();
            ps.clear_current_to_history();
            ps.auto_advance = false;
        }
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

    /// Duración de la transición entre canciones (0 = apagada).
    pub fn set_crossfade(&self, seconds: f32) {
        self.state.crossfade_ms.store((seconds.max(0.0) * 1000.0) as u32, Ordering::Relaxed);
    }

    pub fn get_position(&self) -> Duration {
        self.state.get_position()
    }

    pub fn seek(&self, position: Duration) {
        let _ = self.engine_tx.send(AudioCommand::Seek(position));
    }
}