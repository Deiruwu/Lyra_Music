use std::sync::Arc;
use std::time::Duration;

use crate::audio::manager::manager::TrackManager;
use crate::audio::track_event::TrackEvent;
use crate::db::play_history_manager::PlayHistoryManager;
use crate::microservices::client::MicroserviceClient;
use crate::model::Track;

/// Umbral mínimo de escucha antes de contar una reproducción en el
/// historial persistido — evita ensuciarlo con skips rápidos.
const THRESHOLD: Duration = Duration::from_secs(8);

pub struct PlayHistoryRecorder;

impl PlayHistoryRecorder {
    pub fn spawn(manager: Arc<TrackManager>, play_history: Arc<PlayHistoryManager>, client: MicroserviceClient) {
        std::thread::Builder::new()
            .name("play_history_recorder".into())
            .spawn(move || {
                let rt = tokio::runtime::Runtime::new()
                    .expect("Fallo al crear runtime de Tokio para play_history_recorder");

                rt.block_on(async move {
                    let mut event_rx = manager.event_tx.subscribe();
                    let mut pending: Option<(Track, tokio::time::Instant)> = None;

                    loop {
                        let deadline = pending.as_ref().map(|(_, since)| *since + THRESHOLD);

                        tokio::select! {
                            Ok(event) = event_rx.recv() => match event {
                                TrackEvent::TrackChanged(playable) => {
                                    pending = Some((playable.track.clone(), tokio::time::Instant::now()));
                                }
                                TrackEvent::Stopped => pending = None,
                                _ => {}
                            },
                            _ = tokio::time::sleep_until(deadline.unwrap_or_else(|| tokio::time::Instant::now() + Duration::from_secs(3600))), if deadline.is_some() => {
                                if let Some((track, _)) = pending.take() {
                                    let _ = play_history.record_play(&track).await;
                                    let _ = client.mark_as_played(&track.id).await;
                                }
                            }
                        }
                    }
                });
            })
            .expect("Fallo al lanzar hilo play_history_recorder");
    }
}
