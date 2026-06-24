use std::sync::Arc;
use std::sync::atomic::Ordering;

use tokio::sync::broadcast::error::RecvError;
use tokio::sync::Mutex;
use tokio::task;

use crate::audio::manager::TrackManager;
use crate::audio::track_event::QueueEvent;
use crate::microservices::client::MicroserviceClient;
use crate::model::Track;

// ── Worker ────────────────────────────────────────────────────────────────────

/// Daemon que observa la cola y garantiza que las pistas siempre tengan
/// un archivo local antes de que el reproductor las necesite.
///
/// Responsabilidades:
///   1. **Pre-descarga proactiva** — cuando la primera pista de la cola no
///      tiene `file_path`, la descarga silenciosamente mientras aún está
///      sonando la pista anterior.
///   2. **Descarga de emergencia** — cuando el supervisor intentó avanzar
///      a una pista sin archivo y emitió `QueueEvent::DownloadRequired`,
///      descarga lo antes posible y reactiva la reproducción al terminar.
///
/// `TrackManager` no conoce al cliente del microservicio; toda la lógica
/// de red vive aquí.
pub struct DownloadWorker {
    manager: Arc<TrackManager>,
    client:  Arc<MicroserviceClient>,
    active_id: Mutex<Option<String>>,
}

impl DownloadWorker {
    pub fn new(manager: Arc<TrackManager>, client: Arc<MicroserviceClient>) -> Self {
        Self {
            manager,
            client,
            active_id: Mutex::new(None),
        }
    }

    pub fn spawn(self) -> Arc<Self> {
        let worker = Arc::new(self);
        let w = Arc::clone(&worker);
        task::spawn(async move { w.run().await });
        worker
    }

    // ── Loop principal ────────────────────────────────────────────────────────

    async fn run(&self) {
        let mut queue_rx = self.manager.queue_tx.subscribe();

        // Revisión inicial por si ya hay algo sin descargar al arrancar.
        if let Some(track) = self.next_predownload_candidate().await {
            self.download(track, false).await;
        }

        loop {
            match queue_rx.recv().await {
                Ok(QueueEvent::QueueChanged) => {
                    if let Some(track) = self.next_predownload_candidate().await {
                        self.download(track, false).await;
                    }
                }

                Ok(QueueEvent::DownloadRequired(track)) => {
                    self.handle_emergency((*track).clone()).await;
                }

                Err(RecvError::Closed)    => break,
                Err(RecvError::Lagged(_)) => {
                    // Nos perdimos algunos eventos; revisar el estado actual.
                    if let Some(track) = self.next_predownload_candidate().await {
                        self.download(track, false).await;
                    }
                }
            }
        }

        eprintln!("[DOWNLOAD] Daemon detenido (canal cerrado).");
    }

    // ── Pre-descarga proactiva ────────────────────────────────────────────────

    async fn next_predownload_candidate(&self) -> Option<Track> {
        let first = self.manager.get_queue_snapshot().into_iter().next()?;

        if first.file_path.is_some() {
            return None; // Ya tiene archivo local.
        }

        // No duplicar si ya estamos descargando esta pista.
        if self.active_id.lock().await.as_deref() == Some(first.id.as_str()) {
            return None;
        }

        // No interferir con una emergencia activa; ella tiene prioridad.
        if self.manager.state.status.load(Ordering::Relaxed) == 4 {
            return None;
        }

        Some((*first).clone())
    }

    // ── Descarga de emergencia ────────────────────────────────────────────────

    async fn handle_emergency(&self, track: Track) {
        // Si la pre-descarga ya arrancó para esta pista, no hay que duplicar
        // la petición al microservicio; cuando termine verá status==4 y
        // llamará a `replace_queue_front` con `resume_play=true`.
        if self.active_id.lock().await.as_deref() == Some(track.id.as_str()) {
            println!(
                "[DOWNLOAD] Emergencia para \"{}\" ya en curso, esperando resultado.",
                track.title
            );
            return;
        }

        println!(
            "[DOWNLOAD] Descarga de emergencia: \"{}\" ({})",
            track.title, track.id
        );

        self.download(track, true).await;
    }

    // ── Núcleo de descarga ────────────────────────────────────────────────────

    async fn download(&self, track: Track, is_emergency: bool) {
        let id = track.id.clone();

        *self.active_id.lock().await = Some(id.clone());

        match self.client.download(&id).await {
            Ok(downloaded) => {
                println!(
                    "[DOWNLOAD] Completado: \"{}\" → {:?}",
                    downloaded.title,
                    downloaded.file_path.as_deref().unwrap_or("<sin ruta>")
                );

                let must_resume = is_emergency
                    || self.manager.state.status.load(Ordering::Relaxed) == 4;

                self.manager.replace_queue_front(downloaded, must_resume);
            }

            Err(e) => {
                eprintln!(
                    "[DOWNLOAD] Fallo al descargar \"{}\": {}",
                    track.title, e
                );

                let must_resume = is_emergency
                    || self.manager.state.status.load(Ordering::Relaxed) == 4;

                if must_resume {
                    eprintln!(
                        "[DOWNLOAD] Saltando \"{}\" (descarga fallida irrecuperable).",
                        track.title
                    );
                    self.manager.remove_queue_front_and_resume(&id);
                }
            }
        }

        // Limpiar estado de descarga activa.
        *self.active_id.lock().await = None;
    }
}