use std::collections::{HashMap, HashSet, VecDeque};
use rand::rng;
use rand::seq::SliceRandom;
use uuid::Uuid;
use crate::audio::manager::manager::QueueSlot;

pub fn shuffle(mut slots: Vec<QueueSlot>) -> Vec<QueueSlot> {
    slots.shuffle(&mut rng());
    slots
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
