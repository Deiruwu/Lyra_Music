use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use mpris_server::{
    zbus, Property, Server, Signal,
    LoopStatus, Metadata, PlaybackRate, PlaybackStatus, Time, TrackId, Uri, Volume,
    PlayerInterface, RootInterface,
};
use mpris_server::zbus::fdo;

use crate::audio::manager::TrackManager;
use crate::audio::track_event::TrackEvent;
use crate::model::audio_tech::PlayableTrack;

pub struct MprisServer;

impl MprisServer {
    pub fn spawn(manager: Arc<TrackManager>) {
        std::thread::Builder::new()
            .name("mpris_tokio".into())
            .spawn(move || {
                let rt = tokio::runtime::Runtime::new()
                    .expect("Fallo al crear runtime de Tokio para MPRIS");
                rt.block_on(async move {
                    if let Err(e) = run_server(manager).await {
                        eprintln!("[MPRIS] Error fatal: {e}");
                    }
                });
            })
            .expect("Fallo al lanzar hilo MPRIS");
    }
}

async fn run_server(manager: Arc<TrackManager>) -> Result<(), Box<dyn std::error::Error>> {
    let player = MprisPlayer { manager: Arc::clone(&manager) };
    let server = Server::new("atelier", player).await?;
    eprintln!("[MPRIS] Activo en D-Bus como 'org.mpris.MediaPlayer2.atelier'");

    let mut event_rx = manager.event_tx.subscribe();
    let mut last_pos: u64 = 0;

    loop {
        tokio::select! {
        // Reacciona inmediatamente a eventos de negocio
            Ok(event) = event_rx.recv() => {
                match event {
                    TrackEvent::TrackChanged(track) => {
                        let _ = server.properties_changed([
                            Property::Metadata(build_metadata(&track)),
                            Property::PlaybackStatus(PlaybackStatus::Playing),
                        ]).await;
                    }
                    TrackEvent::Paused => {
                        let _ = server.properties_changed([
                            Property::PlaybackStatus(PlaybackStatus::Paused),
                        ]).await;
                    }
                    TrackEvent::Resumed => {
                        let _ = server.properties_changed([
                            Property::PlaybackStatus(PlaybackStatus::Playing),
                        ]).await;
                    }
                    TrackEvent::Stopped => {
                        let _ = server.properties_changed([
                            Property::PlaybackStatus(PlaybackStatus::Stopped),
                        ]).await;
                    }
                    _ => {}
                }
            }

            // Tick cada 500ms solo para seek detection
            _ = tokio::time::sleep(Duration::from_millis(500)) => {
                let status = manager.state.status.load(Ordering::Relaxed);
                let pos_ms = manager.state.get_position().as_millis() as u64;

                let jumped_back = pos_ms < last_pos.saturating_sub(1000);
                let jumped_forward = pos_ms > last_pos + 3000;

                if status == 1 && (jumped_back || jumped_forward) {
                    let _ = server.emit(Signal::Seeked {
                        position: Time::from_millis(pos_ms as i64),
                    }).await;
                }
                last_pos = pos_ms;
            }
        }
    }
}

struct MprisPlayer {
    manager: Arc<TrackManager>,
}

impl RootInterface for MprisPlayer {
    async fn raise(&self) -> fdo::Result<()> { Ok(()) }
    async fn quit(&self)  -> fdo::Result<()> { Ok(()) }

    async fn can_quit(&self)           -> fdo::Result<bool> { Ok(false) }
    async fn fullscreen(&self)         -> fdo::Result<bool> { Ok(false) }
    async fn set_fullscreen(&self, _: bool) -> zbus::Result<()> { Ok(()) }
    async fn can_set_fullscreen(&self) -> fdo::Result<bool> { Ok(false) }
    async fn can_raise(&self)          -> fdo::Result<bool> { Ok(false) }
    async fn has_track_list(&self)     -> fdo::Result<bool> { Ok(false) }
    async fn identity(&self)      -> fdo::Result<String> { Ok("Atelier".into()) }
    async fn desktop_entry(&self) -> fdo::Result<String> { Ok("atelier".into()) }

    async fn supported_uri_schemes(&self) -> fdo::Result<Vec<String>> { Ok(vec![]) }
    async fn supported_mime_types(&self)  -> fdo::Result<Vec<String>> {
        Ok(vec![
            "audio/flac".into(),
            "audio/mp4".into(),
            "audio/aac".into(),
            "audio/mpeg".into(),
        ])
    }
}

impl PlayerInterface for MprisPlayer {
    async fn next(&self) -> fdo::Result<()> {
        self.manager.skip_next();
        Ok(())
    }

    async fn previous(&self) -> fdo::Result<()> {
        Ok(())
    }

    async fn pause(&self) -> fdo::Result<()> {
        self.manager.pause();
        Ok(())
    }

    async fn play_pause(&self) -> fdo::Result<()> {
        match self.manager.state.status.load(Ordering::Relaxed) {
            1 => self.manager.pause(),
            _ => self.manager.resume(),
        }
        Ok(())
    }

    async fn stop(&self) -> fdo::Result<()> {
        self.manager.stop();
        Ok(())
    }

    async fn play(&self) -> fdo::Result<()> {
        self.manager.resume();
        Ok(())
    }

    async fn seek(&self, offset: Time) -> fdo::Result<()> {
        let current_ms = self.manager.state.get_position().as_millis() as i64;
        let new_ms = (current_ms + offset.as_millis()).max(0) as u64;
        self.manager.seek(Duration::from_millis(new_ms));
        Ok(())
    }

    async fn set_position(&self, _track_id: TrackId, position: Time) -> fdo::Result<()> {
        self.manager.seek(Duration::from_millis(position.as_millis() as u64));
        Ok(())
    }

    async fn position(&self) -> fdo::Result<Time> {
        Ok(Time::from_millis(self.manager.state.get_position().as_millis() as i64))
    }

    async fn open_uri(&self, _uri: Uri) -> fdo::Result<()> {
        Ok(())
    }

    async fn playback_status(&self) -> fdo::Result<PlaybackStatus> {
        Ok(map_status(self.manager.state.status.load(Ordering::Relaxed)))
    }

    async fn loop_status(&self)                    -> fdo::Result<LoopStatus>   { Ok(LoopStatus::None) }
    async fn set_loop_status(&self, _: LoopStatus) -> zbus::Result<()>          { Ok(()) }
    async fn rate(&self)                           -> fdo::Result<PlaybackRate> { Ok(1.0) }
    async fn set_rate(&self, _: PlaybackRate)      -> zbus::Result<()>          { Ok(()) }
    async fn shuffle(&self)                        -> fdo::Result<bool>         { Ok(false) }
    async fn set_shuffle(&self, _: bool)           -> zbus::Result<()>          { Ok(()) }

    async fn metadata(&self) -> fdo::Result<Metadata> {
        let arc_current_track = self.manager.get_current_track();
        let current_track  = arc_current_track.lock().unwrap();
        Ok(match &*current_track {
            Some(track) => build_metadata(&track),
            None        => Metadata::new(),
        })
    }

    async fn volume(&self) -> fdo::Result<Volume> {
        Ok(self.manager.state.get_volume() as f64)
    }

    async fn set_volume(&self, volume: Volume) -> zbus::Result<()> {
        self.manager.set_volume(volume as f32);
        Ok(())
    }

    async fn minimum_rate(&self) -> fdo::Result<PlaybackRate> { Ok(1.0) }
    async fn maximum_rate(&self) -> fdo::Result<PlaybackRate> { Ok(1.0) }

    async fn can_go_next(&self)     -> fdo::Result<bool> { Ok(true)  }
    async fn can_go_previous(&self) -> fdo::Result<bool> { Ok(false) }
    async fn can_play(&self)        -> fdo::Result<bool> { Ok(true)  }
    async fn can_pause(&self)       -> fdo::Result<bool> { Ok(true)  }
    async fn can_seek(&self)        -> fdo::Result<bool> { Ok(true)  }
    async fn can_control(&self)     -> fdo::Result<bool> { Ok(true)  }
}

fn map_status(status: u8) -> PlaybackStatus {
    match status {
        1 => PlaybackStatus::Playing,
        2 => PlaybackStatus::Paused,
        _ => PlaybackStatus::Stopped,
    }
}

fn build_metadata(track: &PlayableTrack) -> Metadata {
    let mut meta = Metadata::new();

    let track_id = TrackId::try_from(format!("/org/atelier/track/{}", track.track.id))
        .unwrap_or(TrackId::NO_TRACK);
    meta.set_trackid(Some(track_id));
    meta.set_title(Some(track.track.title.clone()));

    let artist_names: Vec<String> = track.track.artists
        .iter()
        .map(|a| a.name.clone())
        .collect();
    if !artist_names.is_empty() {
        meta.set_artist(Some(artist_names));
    }

    if let Some(album) = &track.track.album {
        meta.set_album(Some(album.name.clone()));
    }

    if let Some(thumbnail) = &track.track.thumbnail_large {
        meta.set_art_url(Some(thumbnail.clone()));
    }

    if let Some(secs) = track.audio.duration_secs {
        meta.set_length(Some(Time::from_millis(secs as i64 * 1000)));
    }

    if let Some(path) = &track.track.file_path {
        meta.set_url(Some(format!("file://{path}")));
    }

    meta
}