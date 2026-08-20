use crate::audio::manager::manager::{RepeatMode, TrackManager};
use crate::audio::queue_shuffle;

impl TrackManager {
    pub fn toggle_shuffle(&self) {
        let mut ps = self.playback.lock().unwrap();

        if ps.shuffle_enabled {
            if let Some(order) = ps.pre_shuffle_order.take() {
                let current = std::mem::take(&mut ps.queue);
                ps.queue = queue_shuffle::restore_order(current, &order);
            }
            ps.shuffle_enabled = false;
        } else {
            ps.pre_shuffle_order = Some(ps.queue.iter().map(|slot| slot.id).collect());

            let items: Vec<_> = ps.queue.drain(..).collect();
            ps.queue = queue_shuffle::shuffle(items).into();

            ps.shuffle_enabled = true;
        }

        drop(ps);
        self.broadcast_queue_update();
    }

    pub fn cycle_repeat_mode(&self) {
        let mut ps = self.playback.lock().unwrap();
        ps.repeat_mode = match ps.repeat_mode {
            RepeatMode::Off => RepeatMode::Queue,
            RepeatMode::Queue => RepeatMode::Track,
            RepeatMode::Track => RepeatMode::Off,
        };
    }

    pub fn set_repeat_mode(&self, mode: RepeatMode) {
        self.playback.lock().unwrap().repeat_mode = mode;
    }
}
