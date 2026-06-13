use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrackState {
    Cached,
    Partial,
}

impl Default for TrackState {
    fn default() -> Self {
        TrackState::Cached
    }
}
