use std::sync::Arc;
use std::sync::atomic::Ordering;
use crate::audio::manager::manager::TrackManager;
use crate::model::audio_tech::PlayableTrack;
use crate::model::Track;

// ── Helpers para el DownloadWorker ────────────────────────────────────────
impl TrackManager {
    /// Reemplaza el track al frente de la cola por su versión descargada.
    ///
    /// Solo actúa si el track al frente tiene el mismo `id` que `new_track`
    /// (para evitar reemplazos erróneos si la cola cambió mientras se descargaba).
    ///
    /// Si `resume_play` es `true`, restaura el estado "Finished" (3) para que
    /// el supervisor retome la reproducción inmediatamente.
    pub fn replace_queue_front(&self, new_track: Track, resume_play: bool) {
        {

            let mut ps = self.playback.lock().unwrap();
            match ps.queue.front_mut() {
                Some(front) if front.track.id == new_track.id => {
                    front.track = Arc::new(new_track);
                }
                _ => {
                    return;
                }
            }
            if resume_play {
                ps.auto_advance = true;
            }
        }

        if resume_play {
            self.state.status.store(3, Ordering::Relaxed);
        }

        self.broadcast_queue_update();
    }

    /// Elimina el track al frente de la cola (descarga fallida irrecuperable)
    /// y reanuda el ciclo del supervisor para que intente con el siguiente.
    ///
    /// Solo actúa si el frente coincide con `track_id`.
    pub fn remove_queue_front_and_resume(&self, track_id: &str) {
        {
            let mut ps = self.playback.lock().unwrap();
            if ps.queue.front().is_some_and(|slot| slot.track.id == track_id) {
                ps.pop_front_tracked();
            } else {
                return;
            }
            ps.auto_advance = true;
        }

        // Devolver a "Finished" para que el supervisor pruebe con la siguiente pista.
        self.state.status.store(3, Ordering::Relaxed);
        self.broadcast_queue_update();
    }

    /// Pone los datos frescos de `track` en cola, historial, playlist ligada y en la que suena.
    pub fn refresh_track(&self, track: &Track) {
        let fresh = Arc::new(track.clone());
        {
            let mut ps = self.playback.lock().unwrap();
            for slot in ps.queue.iter_mut().filter(|slot| slot.track.id == track.id) {
                slot.track = Arc::clone(&fresh);
            }
            for old in ps.history.iter_mut().filter(|old| old.id == track.id) {
                *old = track.clone();
            }
            if let Some(link) = ps.link.as_mut() {
                link.refresh(&fresh);
            }
            if let Some(current) = ps.current_track.as_ref()
                && current.track.id == track.id
                && current.track.file_path == track.file_path
            {
                let audio = current.audio.clone();
                ps.current_track = Some(Arc::new(PlayableTrack { track: track.clone(), audio }));
            }
        }
        self.broadcast_queue_update();
    }
}
