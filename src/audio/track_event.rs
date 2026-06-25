use std::sync::Arc;
use crate::model::audio_tech::PlayableTrack;
use crate::model::Track;

#[derive(Debug, Clone)]
pub enum TrackEvent {
    TrackChanged(Arc<PlayableTrack>),
    Paused,
    Resumed,
    Stopped,
}

#[derive(Debug, Clone)]
pub enum QueueEvent {
    QueueChanged,
    DownloadRequired(Arc<Track>),
    DownloadStarted(Arc<Track>),
    DownloadFinished(Arc<Track>),
}