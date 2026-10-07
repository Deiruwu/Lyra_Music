use std::sync::Arc;

use crate::audio::manager::error_mananger::ManagerError;
use crate::audio::manager::manager::{QueueSlot, TrackManager};

impl TrackManager {
    pub fn history_len(&self) -> usize {
        self.playback.lock().unwrap().history.len()
    }

    pub fn clear_history(&self) {
        self.playback.lock().unwrap().history.clear();
    }

    pub fn remove_from_history(&self, steps_back: usize) -> Result<(), ManagerError> {
        let mut ps = self.playback.lock().unwrap();
        if steps_back == 0 || steps_back > ps.history.len() {
            return Err(ManagerError::InvalidHistoryIndex { index: steps_back, len: ps.history.len() });
        }
        let index = ps.history.len() - steps_back;
        ps.history.remove(index);
        drop(ps);
        self.broadcast_queue_update();
        Ok(())
    }

    pub fn move_history_to_queue(&self, steps_back: usize, to_front: bool) -> Result<(), ManagerError> {
        let mut ps = self.playback.lock().unwrap();
        if steps_back == 0 || steps_back > ps.history.len() {
            return Err(ManagerError::InvalidHistoryIndex { index: steps_back, len: ps.history.len() });
        }
        let index = ps.history.len() - steps_back;
        let track = ps.history.remove(index).unwrap();
        ps.queue_push(QueueSlot::manual(Arc::new(track)), to_front);
        drop(ps);
        self.broadcast_queue_update();
        Ok(())
    }
}