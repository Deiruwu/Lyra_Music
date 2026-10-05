use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use tokio::sync::broadcast::error::RecvError;
use tokio::task;

use crate::audio::manager::manager::TrackManager;
use crate::audio::music_utils::{shuffle_pool, sort_harmonic};
use crate::audio::track_event::{QueueEvent, TrackEvent};
use crate::microservices::client::MicroserviceClient;

// ── Defaults ──────────────────────────────────────────────────────────────────

/// Cuántas canciones mantener adelantadas en la cola.
const DEFAULT_QUEUE_TARGET: usize = 8;
const POOL_LIMIT: Option<usize> = Some(25);
const SHUFFLE_TAKE: usize = 15;

// ── Worker ────────────────────────────────────────────────────────────────────

pub struct RadioWorker {
    manager:      Arc<TrackManager>,
    client:       Arc<MicroserviceClient>,
    queue_target: Arc<AtomicUsize>,
}

impl RadioWorker {
    pub fn new(manager: Arc<TrackManager>, client: Arc<MicroserviceClient>) -> Self {
        Self {
            manager,
            client,
            queue_target: Arc::new(AtomicUsize::new(DEFAULT_QUEUE_TARGET)),
        }
    }

    // ── Controles públicos ────────────────────────────────────────────────────

    pub fn set_queue_target(&self, n: usize) {
        self.queue_target.store(n.max(1), Ordering::Relaxed);
    }

    pub fn queue_target(&self) -> usize {
        self.queue_target.load(Ordering::Relaxed)
    }

    // ── Spawn ─────────────────────────────────────────────────────────────────

    pub fn spawn(self) -> Arc<Self> {
        let worker = Arc::new(self);
        let w = Arc::clone(&worker);
        task::spawn(async move { w.run().await });
        worker
    }

    // ── Loop principal ────────────────────────────────────────────────────────

    async fn run(&self) {
        let mut event_rx = self.manager.event_tx.subscribe();
        let mut queue_rx = self.manager.queue_tx.subscribe();

        loop {
            tokio::select! {
                result = event_rx.recv() => {
                    match result {
                        Ok(TrackEvent::TrackChanged(track)) => {
                            println!("[RADIO] Actualizando current: {}: {}", track.track.title, track.track.id);

                            if self.manager.is_radio_enabled() {
                                self.fill_queue().await;
                            }
                        }
                        Ok(_) => {}
                        Err(RecvError::Closed)   => break,
                        Err(RecvError::Lagged(_)) => {}
                    }
                }

                result = queue_rx.recv() => {
                    match result {
                        Ok(QueueEvent::QueueChanged)
                            if self.manager.is_radio_enabled() => {
                                self.fill_queue().await;
                            }
                        Err(RecvError::Closed)   => break,
                        Err(RecvError::Lagged(_)) => {}
                        _ => {}
                    }
                }
            }
        }

        eprintln!("[RADIO] Daemon detenido (canal cerrado).");
    }

    // ── Lógica de relleno ─────────────────────────────────────────────────────

    async fn fill_queue(&self) {
        // El track actual se fija antes de emitir eventos, así que siempre es
        // la semilla correcta sin importar si llega antes QueueChanged o TrackChanged.
        let current_seed = self.manager
            .get_current_track()
            .map(|playable| playable.track.id.clone())
            .unwrap_or_default();

        let target = self.queue_target();
        let queue_snapshot = self.manager.get_queue_snapshot();
        let current_len = queue_snapshot.len();

        let watermark = (target * 20 / 100).max(1);
        if current_len > watermark {
            return;
        }

        let needed = target.saturating_sub(current_len);
        if needed == 0 {
            return;
        }

        let mut candidate_seeds = Vec::new();

        if !current_seed.is_empty() {
            candidate_seeds.push(current_seed.clone());
        }

        for slot in queue_snapshot.iter().rev() {
            if !candidate_seeds.contains(&slot.track.id) {
                candidate_seeds.push(slot.track.id.clone());
            }
        }

        if candidate_seeds.len() < 3 {
            let history = self.manager.get_history_snapshot();
            for track in history.iter().rev().take(5) {
                if !candidate_seeds.contains(&track.id) {
                    candidate_seeds.push(track.id.clone());
                }
            }
        }

        if candidate_seeds.is_empty() {
            eprintln!("[RADIO] Inanición absoluta: No hay IDs en current, cola ni historial para usar de semilla.");
            return;
        }

        let mut total_enqueued = 0;
        let max_seeds_to_try = candidate_seeds.len().min(3);

        for seed_id in candidate_seeds.iter().take(max_seeds_to_try) {
            let still_needed = needed - total_enqueued;
            if still_needed == 0 {
                break;
            }

            match self.client.radio(seed_id, POOL_LIMIT).await {
                Ok(pool) => {

                    println!("[RADIO] Pedido semilla '{}': {} tracks.", seed_id, pool.len());
                    let shuffled = shuffle_pool(pool, SHUFFLE_TAKE);
                    let tracks = sort_harmonic(shuffled);

                    let mut fresh_in_this_seed = 0;

                    for track in tracks {
                        if total_enqueued >= needed {
                            break;
                        }
                        if self.manager.enqueue_deduplicated(track) {
                            total_enqueued += 1;
                            fresh_in_this_seed += 1;
                        }
                    }

                    if fresh_in_this_seed > 0 {
                        break;
                    } else {
                        println!("[RADIO] Semilla '{}' agotada (100% duplicados). Saltando a la siguiente...", seed_id);
                    }
                }
                Err(e) => {
                    eprintln!("[RADIO] Falló la red al pedir semilla '{}': {}", seed_id, e);
                }
            }
        }

        if total_enqueued < needed {
            eprintln!(
                "[RADIO] Librería hostil: Tras iterar {} semillas distintas, solo se obtuvieron {}/{} tracks limpios.",
                max_seeds_to_try, total_enqueued, needed
            );
        } else {
            println!("[RADIO] Batería recargada: +{} tracks (Cola al target de {}).", total_enqueued, target);
        }
    }
}