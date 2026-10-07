use std::collections::HashSet;
use std::sync::Arc;

use crate::audio::manager::manager::{push_to_history_inner, QueueSlot, TrackManager};
use crate::audio::manager::error_mananger::ManagerError;
use crate::model::Track;

impl TrackManager {

    pub fn move_in_queue(&self, from: usize, to: usize) -> Result<(), ManagerError> {
        let mut ps = self.playback.lock().unwrap();
        if from >= ps.queue.len() || to >= ps.queue.len() {
            return Err(ManagerError::IndexOutOfRange);
        }
        let slot = ps.queue.remove(from).unwrap();
        ps.queue.insert(to, slot);
        if !ps.shuffle_enabled {
            let id = ps.original_order.remove(from).unwrap();
            ps.original_order.insert(to, id);
        }
        drop(ps);
        self.broadcast_queue_update();
        Ok(())
    }

    pub fn remove_from_queue(&self, index: usize) -> Result<(), ManagerError> {
        let mut ps = self.playback.lock().unwrap();

        if index >= ps.queue.len() {
            return Err(ManagerError::IndexOutOfRange);
        }

        let slot = ps.queue.remove(index).unwrap();
        ps.original_order.retain(|id| *id != slot.id);
        ps.skip_in_link(&slot);
        drop(ps);
        self.broadcast_queue_update();
        Ok(())
    }

    pub fn move_queue_to_history(&self, index: usize) -> Result<(), ManagerError> {
        let mut ps = self.playback.lock().unwrap();
        if index >= ps.queue.len() {
            return Err(ManagerError::IndexOutOfRange);
        }
        let slot = ps.queue.remove(index).unwrap();
        ps.original_order.retain(|id| *id != slot.id);
        push_to_history_inner(&mut ps.history, (*slot.track).clone());
        ps.skip_in_link(&slot);
        drop(ps);
        self.broadcast_queue_update();
        Ok(())
    }

    /// Pone a sonar el slot `index` de la cola; la que sonaba pasa a ser la siguiente.
    pub fn play_queued_over_current(&self, index: usize) -> Result<(), ManagerError> {
        let slot = {
            let mut ps = self.playback.lock().unwrap();
            if index >= ps.queue.len() {
                return Err(ManagerError::IndexOutOfRange);
            }
            let slot = ps.queue.remove(index).unwrap();
            ps.original_order.retain(|id| *id != slot.id);
            slot
        };
        self.play_over_current(slot)
    }

    /// Pone a sonar la canción `steps_back` del historial; la que sonaba pasa a ser la siguiente.
    pub fn play_history_over_current(&self, steps_back: usize) -> Result<(), ManagerError> {
        let slot = {
            let mut ps = self.playback.lock().unwrap();
            if steps_back == 0 || steps_back > ps.history.len() {
                return Err(ManagerError::InvalidHistoryIndex { index: steps_back, len: ps.history.len() });
            }
            let index = ps.history.len() - steps_back;
            let track = ps.history.remove(index).unwrap();
            ps.returning_slot(Arc::new(track))
        };
        self.play_over_current(slot)
    }

    fn play_over_current(&self, slot: QueueSlot) -> Result<(), ManagerError> {
        {
            let mut ps = self.playback.lock().unwrap();
            ps.requeue_current_front();
            ps.queue_push(slot, true);
        }
        self.skip_internal(0)
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

    /// Como `enqueue_many`, pero al frente de la cola conservando el orden de `tracks`.
    pub fn enqueue_front_many(&self, tracks: Vec<Track>) {
        self.enqueue_internal(tracks, true);
    }

    pub fn enqueue_deduplicated(&self, track: Track) -> bool {
        {
            let ps = self.playback.lock().unwrap();

            if let Some(current) = ps.current_track.as_ref()
                && current.track.id == track.id {
                    return false;
                }

            if ps.queue.iter().any(|slot| slot.track.id == track.id) {
                return false;
            }

            if ps.history.iter().any(|t| t.id == track.id) {
                return false;
            }
        }

        self.enqueue(track);
        true
    }

        pub fn clear_queue(&self) -> Result<(), ManagerError> {
        let mut ps = self.playback.lock().unwrap();

        ps.queue.clear();
        ps.original_order.clear();
        ps.link = None;

        drop(ps);
        self.broadcast_queue_update();
        Ok(())
    }

    /// Saca `track_ids` de cola, historial y playlist ligada; si una sonaba, pasa a la siguiente.
    pub fn forget_tracks(&self, track_ids: &[String]) {
        let ids: HashSet<&str> = track_ids.iter().map(String::as_str).collect();
        let was_current = {
            let mut ps = self.playback.lock().unwrap();
            let removed: HashSet<_> = ps.queue.iter().filter(|s| ids.contains(s.track.id.as_str())).map(|s| s.id).collect();
            ps.queue.retain(|slot| !removed.contains(&slot.id));
            ps.original_order.retain(|id| !removed.contains(id));
            ps.history.retain(|track| !ids.contains(track.id.as_str()));
            if let Some(link) = ps.link.as_mut() {
                link.forget(&ids);
            }

            let was_current = ps.current_track.as_ref().is_some_and(|c| ids.contains(c.track.id.as_str()));
            if was_current {
                ps.current_track = None;
            }
            ps.reconcile_link(false);
            was_current
        };

        if was_current {
            self.skip_next();
        }
        self.broadcast_queue_update();
    }
}
