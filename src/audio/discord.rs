use std::sync::Arc;
use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};
use crate::audio::manager::TrackManager;
use crate::audio::track_event::TrackEvent;

pub struct DiscordPresence;

impl DiscordPresence {
    pub fn spawn(manager: Arc<TrackManager>) {
        std::thread::Builder::new()
            .name("discord_presence".into())
            .spawn(move || {
                let mut client = DiscordIpcClient::new("1514816988524839073");


                if let Err(e) = client.connect() {
                    eprintln!("[DISCORD] No se pudo conectar: {e}");
                    return;
                }

                eprintln!("[DISCORD] Conectado");

                let mut event_rx = manager.event_tx.subscribe();

                loop {
                    match event_rx.blocking_recv() {
                        Ok(TrackEvent::TrackChanged(track)) => {
                            let artists = track.track.artists.iter()
                                .map(|a| a.name.clone())
                                .collect::<Vec<_>>()
                                .join(", ");

                            let _ = client.set_activity(
                                activity::Activity::new()
                                    .details(&track.track.title)
                                    .state(&artists)
                                    .assets(
                                        activity::Assets::new()
                                            .large_image("5a2ddaacd258579c6cbaa35a66a022d9")
                                    )
                            );
                        }
                        Ok(TrackEvent::Stopped) => {
                            let _ = client.clear_activity();
                        }
                        Ok(_) => {}
                        Err(_) => break,
                    }
                }
            })
            .expect("Fallo al lanzar hilo Discord");
    }
}