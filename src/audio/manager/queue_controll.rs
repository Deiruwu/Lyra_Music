use crate::audio::manager::manager::TrackManager;
use crate::audio::manager::error_mananger::ManagerError;
use crate::model::Track;

impl TrackManager {

    pub fn move_in_queue(&self, from: usize, to: usize) -> Result<(), ManagerError> {
        let mut ps = self.playback.lock().unwrap();
        if from >= ps.queue.len() || to >= ps.queue.len() {
            return Err(ManagerError::IndexOutOfRange);
        }
        let track = ps.queue.remove(from).unwrap();
        ps.queue.insert(to, track);
        drop(ps);
        self.broadcast_queue_update();
        Ok(())
    }

    pub fn remove_from_queue(&self, index: usize) -> Result<(), ManagerError> {
        let mut ps = self.playback.lock().unwrap();

        if index >= ps.queue.len() {
            return Err(ManagerError::IndexOutOfRange);
        }

        ps.queue.remove(index);
        drop(ps);
        self.broadcast_queue_update();
        Ok(())
    }

    pub fn enqueue(&self, track: Track) {
        self.enqueue_internal(std::iter::once(track), false);
    }

    pub fn enqueue_front(&self, track: Track) {
        self.enqueue_internal(std::iter::once(track), true);
    }

    pub fn enqueue_many(&self, tracks: Vec<Track>) {
        self.enqueue_internal(tracks, false);
    }

    pub fn enqueue_deduplicated(&self, track: Track) -> bool {
        {
            let ps = self.playback.lock().unwrap();

            if let Some(current) = ps.current_track.as_ref() {
                if current.track.id == track.id {
                    return false;
                }
            }

            if ps.queue.iter().any(|t| t.id == track.id) {
                return false;
            }

            if ps.history.iter().any(|t| t.id == track.id) {
                return false;
            }
        }

        self.enqueue(track);
        true
    }
}