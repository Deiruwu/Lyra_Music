use std::sync::Arc;
use crate::model::audio_tech::PlayableTrack;

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
}