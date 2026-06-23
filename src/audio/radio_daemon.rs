use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use tokio::sync::broadcast::error::RecvError;
use tokio::task;

use crate::audio::manager::TrackManager;
use crate::audio::track_event::{QueueEvent, TrackEvent};
use crate::microservices::client::MicroserviceClient;

// ── Defaults ──────────────────────────────────────────────────────────────────

/// Cuántas canciones mantener adelantadas en la cola.
const DEFAULT_QUEUE_TARGET: usize = 8;

// ── Worker ────────────────────────────────────────────────────────────────────

pub struct RadioWorker {
    manager:      Arc<TrackManager>,
    client:       Arc<MicroserviceClient>,
    enabled:      Arc<AtomicBool>,
    queue_target: Arc<AtomicUsize>,
}

impl RadioWorker {
    pub fn new(manager: Arc<TrackManager>, client: Arc<MicroserviceClient>) -> Self {
        Self {
            manager,
            client,
            enabled:      Arc::new(AtomicBool::new(false)),
            queue_target: Arc::new(AtomicUsize::new(DEFAULT_QUEUE_TARGET)),
        }
    }

    // ── Controles públicos ────────────────────────────────────────────────────

    pub fn set_enabled(&self, on: bool) {
        self.enabled.store(on, Ordering::Relaxed);
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub fn set_queue_target(&self, n: usize) {
        self.queue_target.store(n.max(1), Ordering::Relaxed);
    }

    pub fn queue_target(&self) -> usize {
        self.queue_target.load(Ordering::Relaxed)
    }

    // ── Spawn ─────────────────────────────────────────────────────────────────

    /// Lanza el daemon en el runtime de Tokio actual.
    /// Devuelve `Arc<RadioWorker>` para que la UI pueda llamar
    /// `set_enabled` / `set_queue_target` desde cualquier hilo.
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

        let mut current_seed = String::new();

        loop {
            tokio::select! {
                result = event_rx.recv() => {
                    match result {
                        Ok(TrackEvent::TrackChanged(track)) => {
                            current_seed = track.track.id.clone();

                            println!("[RADIO] Actualizando current: {}: {}",track.track.title ,track.track.id);

                            if self.is_enabled() {
                                self.fill_queue(&current_seed).await;
                            }
                        }
                        Ok(TrackEvent::Stopped) => {
                            current_seed.clear();
                        }
                        Ok(_) => {}
                        Err(RecvError::Closed)   => break,
                        Err(RecvError::Lagged(_)) => {}
                    }
                }

                result = queue_rx.recv() => {
                    match result {
                        Ok(QueueEvent::QueueChanged) => {
                            if self.is_enabled() {
                                self.fill_queue(&current_seed).await;
                            }
                        }
                        Err(RecvError::Closed)   => break,
                        Err(RecvError::Lagged(_)) => {}
                    }
                }
            }
        }

        eprintln!("[RADIO] Daemon detenido (canal cerrado).");
    }

    // ── Lógica de relleno ─────────────────────────────────────────────────────

    /// Si la cola está por debajo del target, pide tracks al microservicio y los encola.
    async fn fill_queue(&self, seed_id: &str) {
        let target      = self.queue_target();
        let current_len = self.manager.get_queue_snapshot().len();

        if current_len >= target {
            return;
        }

        let needed = target - current_len;

        match self.client.radio(seed_id).await {
            Ok(tracks) => {
                let mut enqueued = 0;

                for track in tracks {
                    if enqueued >= needed {
                        break;
                    }

                    if self.manager.enqueue_deduplicated(track) {
                        enqueued += 1;
                    }
                }

                if enqueued < needed {
                    eprintln!(
                        "[RADIO] Solo se encolaron {enqueued}/{needed} tracks \
                         (pocos resultados no duplicados del microservicio)."
                    );
                }
            }
            Err(e) => {
                eprintln!("[RADIO] Error al pedir radio: {e}");
            }
        }
    }
}