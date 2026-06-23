use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};

use crate::audio::manager::TrackManager;
use crate::audio::track_event::TrackEvent;
use crate::model::audio_tech::PlayableTrack;

const APP_ID:        &str = "1514816988524839073";
const DEFAULT_ASSET: &str = "5a2ddaacd258579c6cbaa35a66a022d9";
const DEFAULT_TEXT:  &str = "Lyra Music";

pub struct DiscordPresence;

impl DiscordPresence {
    pub fn spawn(manager: Arc<TrackManager>) {
        std::thread::Builder::new()
            .name("discord_presence".into())
            .spawn(move || {
                let mut client = DiscordIpcClient::new(APP_ID);

                if let Err(e) = client.connect() {
                    eprintln!("[DISCORD] No se pudo conectar: {e}");
                    return;
                }

                println!("[DISCORD] Conectado");

                let mut current_track: Option<Arc<PlayableTrack>> = None;
                let mut is_paused = false;

                let mut event_rx = manager.event_tx.subscribe();

                loop {
                    match event_rx.blocking_recv() {
                        Ok(TrackEvent::TrackChanged(track)) => {
                            is_paused = false;
                            current_track = Some(Arc::clone(&track));
                            set_activity(&mut client, &manager, &track, false);
                        }

                        Ok(TrackEvent::Paused) => {
                            is_paused = true;
                            if let Some(ref track) = current_track {
                                set_activity(&mut client, &manager, track, true);
                            }
                        }

                        Ok(TrackEvent::Resumed) => {
                            is_paused = false;
                            if let Some(ref track) = current_track {
                                set_activity(&mut client, &manager, track, false);
                            }
                        }

                        Ok(TrackEvent::Stopped) => {
                            current_track = None;
                            is_paused = false;
                            let _ = client.clear_activity();
                        }

                        Ok(_) => {}
                        Err(_) => break,
                    }

                    // Suprimir warning si is_paused no se usa en alguna rama futura
                    let _ = is_paused;
                }
            })
            .expect("Fallo al lanzar hilo Discord");
    }
}

fn set_activity(
    client:   &mut DiscordIpcClient,
    manager:  &TrackManager,
    track:    &PlayableTrack,
    is_paused: bool,
) {
    let t = &track.track;

    let title = if t.title.is_empty() { "Reproduciendo".to_string() } else { t.title.clone() };

    let artists = t.format_artists();
    let state = if artists.len() >= 2 { artists } else { DEFAULT_TEXT.to_string() };

    // ── Assets ────────────────────────────────────────────────────────────────
    let large_image = t.thumbnail_large
        .as_deref()
        .or(t.thumbnail_small.as_deref())
        .unwrap_or(DEFAULT_ASSET);

    let album_text = t.album
        .as_ref()
        .map(|a| a.name.as_str())
        .unwrap_or(DEFAULT_TEXT);

    let mut assets = activity::Assets::new()
        .large_image(large_image)
        .large_text(album_text);

    if is_paused {
        assets = assets.small_text("En pausa");
    }

    let history_len = manager.history_len();
    let queue_len   = manager.get_queue_snapshot().len();
    let pos         = (history_len + 1) as i32;
    let total       = (history_len + 1 + queue_len) as i32;

    let timestamps = if !is_paused {
        let now_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        let pos_secs = manager.get_position().as_secs() as i64;
        let start    = now_secs - pos_secs;

        let mut ts = activity::Timestamps::new().start(start);

        if t.duration_seconds > 0 {
            ts = ts.end(start + t.duration_seconds as i64);
        }

        Some(ts)
    } else {
        None
    };

    // ── Construir actividad ───────────────────────────────────────────────────
    let mut a = activity::Activity::new()
        .activity_type(activity::ActivityType::Listening)
        .details(&title)
        .state(&state)
        .assets(assets)
        .party(activity::Party::new().id("lyra").size([pos, total]));

    if let Some(ts) = timestamps {
        a = a.timestamps(ts);
    }

    if let Err(e) = client.set_activity(a) {
        eprintln!("[DISCORD] Error al enviar actividad: {e}");
    }
}