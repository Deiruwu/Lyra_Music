use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use tokio::sync::broadcast::error::RecvError;
use tokio::task;

use crate::audio::mananger::manager::TrackManager;
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
            enabled: Arc::new(AtomicBool::new(false)),
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
                    _ => {}}
                }
            }
        }

        eprintln!("[RADIO] Daemon detenido (canal cerrado).");
    }

    // ── Lógica de relleno ─────────────────────────────────────────────────────

    async fn fill_queue(&self, current_seed: &str) {
        let target = self.queue_target();
        let queue_snapshot = self.manager.get_queue_snapshot();
        let current_len = queue_snapshot.len();

        // 1. PATRÓN LOW WATERMARK (Marca de agua baja al 20%)
        // Calculamos el umbral crítico de pánico. Si target es 10, watermark es 2.
        let watermark = (target * 20 / 100).max(1);

        // CONDICIÓN DE CORTE ABSOLUTA: Si la cola está por encima del 20%,
        // el daemon tiene estrictamente prohibido tocar el socket de red.
        if current_len > watermark {
            return;
        }

        let needed = target.saturating_sub(current_len);
        if needed == 0 {
            return;
        }

        // 2. RECOLECCIÓN DE ENTROPÍA (Seed Hopping)
        // Construimos un radar de semillas candidatas ordenadas por lógica temporal:
        //   1. La pista actual (current_seed).
        //   2. Las pistas de la cola EN ORDEN INVERSO (de la última hacia atrás).
        //   3. El historial reciente (por si el usuario vació la cola a mano).
        let mut candidate_seeds = Vec::new();

        if !current_seed.is_empty() {
            candidate_seeds.push(current_seed.to_string());
        }

        // ¿Por qué .rev()? Porque la última canción de la cola define hacia
        // dónde se dirige el mood de la sesión, no de dónde viene.
        for track in queue_snapshot.iter().rev() {
            if !candidate_seeds.contains(&track.id) {
                candidate_seeds.push(track.id.clone());
            }
        }

        // Si la cola era muy corta, rascamos el fondo del historial
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

        // 3. BUCLE DE EXHAUSTIVIDAD CON CORTAFUEGOS (Máximo 3 semillas por ciclo)
        let mut total_enqueued = 0;
        let max_seeds_to_try = candidate_seeds.len().min(3);

        for seed_id in candidate_seeds.iter().take(max_seeds_to_try) {
            let still_needed = needed - total_enqueued;
            if still_needed == 0 {
                break;
            }

            match self.client.radio(seed_id).await {
                Ok(tracks) => {
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

                    // CRÍTICO: Si esta semilla nos dio al menos UNA canción nueva,
                    // asumimos que el filón es bueno y rompemos el hopping.
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

        // 4. FEEDBACK RIGUROSO Y SILENCIOSO
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