use std::sync::Arc;
use std::sync::atomic::Ordering;

use tokio::sync::broadcast::error::RecvError;
use tokio::sync::Mutex;
use tokio::task;

use crate::audio::mananger::manager::TrackManager;
use crate::audio::track_event::QueueEvent;
use crate::microservices::client::MicroserviceClient;
use crate::model::Track;

pub struct DownloadWorker {
    manager:   Arc<TrackManager>,
    client:    Arc<MicroserviceClient>,
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
                    if let Some(track) = self.next_predownload_candidate().await {
                        self.download(track, false).await;
                    }
                }

                // Estos eventos los emite el propio worker; ignorarlos para
                // no reaccionar a los propios broadcasts.
                Ok(QueueEvent::DownloadStarted(_))  => {}
                Ok(QueueEvent::DownloadFinished(_)) => {}
            }
        }

        eprintln!("[DOWNLOAD] Daemon detenido (canal cerrado).");
    }

    // ── Pre-descarga proactiva ────────────────────────────────────────────────

    async fn next_predownload_candidate(&self) -> Option<Track> {
        let first = self.manager.get_queue_snapshot().into_iter().next()?;

        if first.file_path.is_some() {
            return None;
        }

        if self.active_id.lock().await.as_deref() == Some(first.id.as_str()) {
            return None;
        }

        if self.manager.state.status.load(Ordering::Relaxed) == 4 {
            return None;
        }

        Some((*first).clone())
    }

    // ── Descarga de emergencia ────────────────────────────────────────────────

    async fn handle_emergency(&self, track: Track) {
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
        let id         = track.id.clone();
        let track_arc  = Arc::new(track.clone());

        *self.active_id.lock().await = Some(id.clone());

        // Notificar a la UI que arrancó una descarga.
        let _ = self.manager.queue_tx.send(QueueEvent::DownloadStarted(Arc::clone(&track_arc)));

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
                eprintln!("[DOWNLOAD] Fallo al descargar \"{}\": {}", track.title, e);

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

        // Notificar a la UI que terminó (éxito o fallo).
        let _ = self.manager.queue_tx.send(QueueEvent::DownloadFinished(track_arc));

        *self.active_id.lock().await = None;
    }
}