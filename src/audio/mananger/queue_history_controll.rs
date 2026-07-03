use crate::audio::mananger::manager::TrackManager;

impl TrackManager {
    pub fn history_len(&self) -> usize {
        self.playback.lock().unwrap().history.len()
    }

    pub fn clear_history(&self) {
        self.playback.lock().unwrap().history.clear();
    }
}