use std::sync::Arc;
use std::time::Duration;
use crate::audio::decoder::ChannelMode;
use crate::audio::engine_state::AudioCommand;
use crate::audio::manager::manager::{probe_track, QueueSlot, TrackManager, HISTORY_CAP};
use crate::model::audio_tech::PlayableTrack;
use crate::model::Track;

impl TrackManager {
    /// Restaura una sesión guardada: cola e historial tal cual, y la canción
    /// actual cargada en pausa en `position`. Si la actual ya no se puede abrir,
    /// queda al frente de la cola. Devuelve la canción cargada, si hubo.
    pub fn restore_session(
        &self,
        current: Option<Track>,
        position: Duration,
        queue: Vec<Track>,
        history: Vec<Track>,
    ) -> Option<Arc<PlayableTrack>> {
        let playable = current
            .as_ref()
            .filter(|t| t.file_path.is_some())
            .and_then(|t| probe_track(t, "MANAGER:restore_session").ok());

        {
            let mut ps = self.playback.lock().unwrap();

            let mut slots: Vec<QueueSlot> = queue.into_iter().map(|t| QueueSlot::new(Arc::new(t))).collect();
            if playable.is_none()
                && let Some(track) = current {
                    slots.insert(0, QueueSlot::new(Arc::new(track)));
                }

            ps.original_order = slots.iter().map(|s| s.id).collect();
            ps.queue = slots.into();
            ps.history = history.into_iter().rev().take(HISTORY_CAP).rev().collect();
            ps.current_track = playable.clone();
            ps.auto_advance = true;
        }

        if let Some(track) = &playable {
            let _ = self.engine_tx.send(AudioCommand::Load {
                track: Arc::clone(track),
                mode: ChannelMode::Stereo,
                position,
            });
        }

        self.broadcast_queue_update();
        playable
    }
}
