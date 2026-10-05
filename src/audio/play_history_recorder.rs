use std::sync::Arc;
use std::time::Duration;

use crate::audio::manager::manager::TrackManager;
use crate::audio::track_event::TrackEvent;
use crate::db::play_history_manager::PlayHistoryManager;
use crate::microservices::client::MicroserviceClient;
use crate::model::Track;

/// Fracción de la duración de la canción que debe transcurrir antes de
/// contarla como reproducción en el historial persistido — evita
/// ensuciarlo con skips rápidos.
const PLAY_THRESHOLD_RATIO: f64 = 0.7;

/// Umbral de respaldo cuando `duration_seconds` no es confiable (0 o negativo).
const PLAY_THRESHOLD_FALLBACK: Duration = Duration::from_secs(50);

fn play_threshold(track: &Track) -> Duration {
    if track.duration_seconds <= 0 {
        PLAY_THRESHOLD_FALLBACK
    } else {
        Duration::from_secs_f64(track.duration_seconds as f64 * PLAY_THRESHOLD_RATIO)
    }
}

pub struct PlayHistoryRecorder;

impl PlayHistoryRecorder {
    pub fn spawn(manager: Arc<TrackManager>, play_history: Arc<PlayHistoryManager>, client: MicroserviceClient) {
        std::thread::Builder::new()
            .name("play_history_recorder".into())
            .spawn(move || {
                // Ver comentario en mpris.rs: current_thread alcanza.
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("Fallo al crear runtime de Tokio para play_history_recorder");

                rt.block_on(async move {
                    let mut event_rx = manager.event_tx.subscribe();
                    let mut pending: Option<(Track, tokio::time::Instant)> = None;

                    loop {
                        let deadline = pending.as_ref().map(|(track, since)| *since + play_threshold(track));

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
