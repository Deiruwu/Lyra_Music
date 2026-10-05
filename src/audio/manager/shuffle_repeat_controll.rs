use std::sync::atomic::Ordering;
use crate::audio::manager::manager::{RepeatMode, TrackManager};
use crate::audio::queue_shuffle;
use crate::model::Track;
use uuid::Uuid;

impl TrackManager {
    pub fn toggle_shuffle(&self) {
        let mut ps = self.playback.lock().unwrap();

        if ps.shuffle_enabled {
            let current = std::mem::take(&mut ps.queue);
            let order: Vec<Uuid> = ps.original_order.iter().cloned().collect();
            ps.queue = queue_shuffle::restore_order(current, &order);
            ps.shuffle_enabled = false;
        } else {
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

    /// Fija `shuffle_enabled` directamente, sin reordenar la cola — pensado
    /// para restaurar el estado guardado al arrancar, cuando la cola
    /// todavía está vacía.
    pub fn set_shuffle_enabled(&self, enabled: bool) {
        self.playback.lock().unwrap().shuffle_enabled = enabled;
    }

    pub fn is_radio_enabled(&self) -> bool {
        self.radio_enabled.load(Ordering::Relaxed)
    }

    /// Activa/desactiva la radio; al activarla avisa al `RadioWorker` para que rellene ya.
    pub fn set_radio_enabled(&self, enabled: bool) {
        self.radio_enabled.store(enabled, Ordering::Relaxed);
        if enabled {
            self.broadcast_queue_update();
        }
    }

    /// Reproduce `track` sola y activa la radio para que la cola se llene a partir de ella.
    pub fn start_radio(&self, track: Track) {
        self.radio_enabled.store(true, Ordering::Relaxed);
        self.play_context(vec![track], 0);
    }
}
