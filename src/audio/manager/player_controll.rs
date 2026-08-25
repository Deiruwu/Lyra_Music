use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;
use crate::audio::engine_state::AudioCommand;
use crate::audio::manager::manager::{probe_track, QueueSlot, TrackManager};
use crate::audio::manager::error_mananger::ManagerError;
use crate::audio::queue_shuffle;
use crate::audio::track_event::{QueueEvent, TrackEvent};
use crate::model::Track;

impl TrackManager {

    /// Reproduce `context_tracks[start_index]` y encola el resto.
    ///
    /// Sin shuffle: "reproducir desde acá en adelante" — los tracks
    /// antes de `start_index` se descartan de la cola a propósito (no es
    /// un bug, es la semántica de este modo). En shuffle, en cambio,
    /// TODOS los demás tracks de la vista (antes y después del
    /// clickeado) entran al sorteo — `refill_queue` ya baraja `remaining`
    /// cuando `shuffle_enabled`, así que alcanza con no descartarlos de
    /// entrada.
    pub fn play_context(&self, context_tracks: Vec<Track>, start_index: usize) {
        if start_index >= context_tracks.len() { return; }

        let shuffle_enabled = self.playback.lock().unwrap().shuffle_enabled;

        let (first, remaining_tracks): (Track, Vec<Track>) = if shuffle_enabled {
            let mut tracks = context_tracks;
            let first = tracks.remove(start_index);
            (first, tracks)
        } else {
            let mut tracks_iter = context_tracks.into_iter().skip(start_index);
            let first = tracks_iter.next().unwrap();
            (first, tracks_iter.collect())
        };

        let first_track = Arc::new(first);
        let remaining: Vec<QueueSlot> = remaining_tracks.into_iter().map(|t| QueueSlot::new(Arc::new(t))).collect();

        {
            let mut ps = self.playback.lock().unwrap();
            ps.refill_queue(remaining);
            ps.clear_current_to_history();
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
            ps.advance_to(Arc::clone(&playable));
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

    pub fn get_position(&self) -> Duration {
        self.state.get_position()
    }

    pub fn seek(&self, position: Duration) {
        let _ = self.engine_tx.send(AudioCommand::Seek(position));
    }
}