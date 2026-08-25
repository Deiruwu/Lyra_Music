use std::collections::{HashMap, HashSet, VecDeque};
use rand::rng;
use rand::seq::SliceRandom;
use rand::RngExt;
use uuid::Uuid;
use crate::audio::manager::manager::QueueSlot;
use crate::model::Track;

pub fn shuffle(mut slots: Vec<QueueSlot>) -> Vec<QueueSlot> {
    slots.shuffle(&mut rng());
    slots
}

/// Igual que `shuffle`, pero sobre `Track` crudo en vez de `QueueSlot` —
/// para barajar la vista ENTERA antes de decidir qué entra primero
/// ("Reproducir todo" en modo shuffle), en vez de barajar solo la cola
/// restante una vez que el primer track ya quedó fijado.
pub fn shuffle_tracks(mut tracks: Vec<Track>) -> Vec<Track> {
    tracks.shuffle(&mut rng());
    tracks
}

/// Inserta `slot` en una posición aleatoria de `queue`.
pub fn insert_shuffled(queue: &mut VecDeque<QueueSlot>, slot: QueueSlot) {
    let idx = rng().random_range(0..=queue.len());
    queue.insert(idx, slot);
}

/// Reconstruye `current` en el orden de `order` (ids de antes del shuffle),
/// descartando los que ya no estén y agregando al final, en el orden en que
/// aparecen en `current`, los que se sumaron durante el shuffle.
pub fn restore_order(current: VecDeque<QueueSlot>, order: &[Uuid]) -> VecDeque<QueueSlot> {
    let order_set: HashSet<&Uuid> = order.iter().collect();
    let mut extra_order = Vec::new();

    let mut by_id: HashMap<Uuid, QueueSlot> = current
        .into_iter()
        .map(|slot| {
            if !order_set.contains(&slot.id) {
                extra_order.push(slot.id);
            }
            (slot.id, slot)
        })
        .collect();

    let mut restored = VecDeque::with_capacity(by_id.len());
    for id in order.iter().chain(extra_order.iter()) {
        if let Some(slot) = by_id.remove(id) {
            restored.push_back(slot);
        }
    }
    restored
}
