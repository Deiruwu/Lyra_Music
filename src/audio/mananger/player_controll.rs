use std::sync::Arc;
use std::time::Duration;
use crate::audio::engine_state::AudioCommand;
use crate::audio::mananger::manager::{probe_track, TrackManager};
use crate::audio::mananger::error_mananger::ManagerError;
use crate::audio::track_event::TrackEvent;
use crate::model::Track;

impl TrackManager {

    /// Pone una pista inmediatamente, borrando lo que esté sonando.
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
                ps.queue.push_front(Arc::new(current_track));
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