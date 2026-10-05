use serde::Deserialize;

use crate::model::Track;

/// Evento empujado por el microservicio en el canal de `subscribe_downloads`.
/// El wire trae además `status`/`event` ambiente (siempre `"event"`/`"download"`),
/// que se ignoran al no usar `deny_unknown_fields`.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum DownloadEvent {
    Requested {
        id: String,
        title: String,
        thumbnail_small: Option<String>,
    },
    Downloading {
        id: String,
        title: String,
        thumbnail_small: Option<String>,
        downloaded_bytes: Option<u64>,
        total_bytes: Option<u64>,
        speed_bytes_per_sec: Option<f64>,
        eta_seconds: Option<u64>,
    },
    Finished {
        id: String,
        title: String,
        thumbnail_small: Option<String>,
    },
    Failed {
        id: String,
        title: String,
        thumbnail_small: Option<String>,
        message: String,
    },
    #[serde(rename = "analyzestarted")]
    AnalyzeStarted {
        id: String,
        title: String,
        thumbnail_small: Option<String>,
    },
    #[serde(rename = "analyzefinished")]
    AnalyzeFinished {
        track: Track,
    },
    #[serde(rename = "analyzefailed")]
    AnalyzeFailed {
        id: String,
        title: String,
        thumbnail_small: Option<String>,
        message: String,
    },
    #[serde(rename = "lyricsfound")]
    LyricsFound {
        id: String,
        title: String,
        thumbnail_small: Option<String>,
    },
    #[serde(rename = "lyricsnotfound")]
    LyricsNotFound {
        id: String,
        title: String,
        thumbnail_small: Option<String>,
    },
    #[serde(rename = "metadataupdated")]
    MetadataUpdated {
        track: Track,
    },
    #[serde(rename = "metadatafailed")]
    MetadataFailed {
        id: String,
        title: String,
        thumbnail_small: Option<String>,
        message: String,
    },
}

impl DownloadEvent {
    pub fn id(&self) -> &str {
        match self {
            DownloadEvent::Requested { id, .. } => id,
            DownloadEvent::Downloading { id, .. } => id,
            DownloadEvent::Finished { id, .. } => id,
            DownloadEvent::Failed { id, .. } => id,
            DownloadEvent::AnalyzeStarted { id, .. } => id,
            DownloadEvent::AnalyzeFinished { track } => &track.id,
            DownloadEvent::AnalyzeFailed { id, .. } => id,
            DownloadEvent::LyricsFound { id, .. } => id,
            DownloadEvent::LyricsNotFound { id, .. } => id,
            DownloadEvent::MetadataUpdated { track } => &track.id,
            DownloadEvent::MetadataFailed { id, .. } => id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_requested() {
        let raw = r#"{"status":"event","event":"download","state":"requested","id":"dQw4w9WgXcQ","title":"Never Gonna Give You Up","thumbnail_small":"https://.../small.jpg"}"#;
        let event: DownloadEvent = serde_json::from_str(raw).unwrap();
        match event {
            DownloadEvent::Requested { id, title, thumbnail_small } => {
                assert_eq!(id, "dQw4w9WgXcQ");
                assert_eq!(title, "Never Gonna Give You Up");
                assert_eq!(thumbnail_small.as_deref(), Some("https://.../small.jpg"));
            }
            _ => panic!("esperaba Requested"),
        }
    }

    #[test]
    fn parses_downloading() {
        let raw = r#"{"status":"event","event":"download","state":"downloading","id":"dQw4w9WgXcQ","title":"Never Gonna Give You Up","thumbnail_small":"https://.../small.jpg","downloaded_bytes":1048576,"total_bytes":4194304,"speed_bytes_per_sec":512000.0,"eta_seconds":6}"#;
        let event: DownloadEvent = serde_json::from_str(raw).unwrap();
        match event {
            DownloadEvent::Downloading { id, downloaded_bytes, total_bytes, speed_bytes_per_sec, eta_seconds, .. } => {
                assert_eq!(id, "dQw4w9WgXcQ");
                assert_eq!(downloaded_bytes, Some(1048576));
                assert_eq!(total_bytes, Some(4194304));
                assert_eq!(speed_bytes_per_sec, Some(512000.0));
                assert_eq!(eta_seconds, Some(6));
            }
            _ => panic!("esperaba Downloading"),
        }
    }

    #[test]
    fn parses_finished() {
        let raw = r#"{"status":"event","event":"download","state":"finished","id":"dQw4w9WgXcQ","title":"Never Gonna Give You Up","thumbnail_small":"https://.../small.jpg"}"#;
        let event: DownloadEvent = serde_json::from_str(raw).unwrap();
        assert_eq!(event.id(), "dQw4w9WgXcQ");
        assert!(matches!(event, DownloadEvent::Finished { .. }));
    }

    #[test]
    fn parses_failed() {
        let raw = r#"{"status":"event","event":"download","state":"failed","id":"dQw4w9WgXcQ","title":"Never Gonna Give You Up","thumbnail_small":"https://.../small.jpg","message":"yt-dlp failed: ERROR: Video unavailable"}"#;
        let event: DownloadEvent = serde_json::from_str(raw).unwrap();
        match event {
            DownloadEvent::Failed { id, message, .. } => {
                assert_eq!(id, "dQw4w9WgXcQ");
                assert_eq!(message, "yt-dlp failed: ERROR: Video unavailable");
            }
            _ => panic!("esperaba Failed"),
        }
    }

    #[test]
    fn parses_lyrics_and_metadata_notices() {
        let found = r#"{"status":"event","event":"download","state":"lyricsfound","id":"dQw4w9WgXcQ","title":"Never Gonna Give You Up","thumbnail_small":null}"#;
        assert!(matches!(serde_json::from_str::<DownloadEvent>(found).unwrap(), DownloadEvent::LyricsFound { .. }));

        let not_found = r#"{"status":"event","event":"download","state":"lyricsnotfound","id":"dQw4w9WgXcQ","title":"Never Gonna Give You Up","thumbnail_small":null}"#;
        assert!(matches!(serde_json::from_str::<DownloadEvent>(not_found).unwrap(), DownloadEvent::LyricsNotFound { .. }));

        let failed = r#"{"status":"event","event":"download","state":"metadatafailed","id":"dQw4w9WgXcQ","title":"dQw4w9WgXcQ","thumbnail_small":null,"message":"Metadata error: Track no encontrado"}"#;
        assert!(matches!(serde_json::from_str::<DownloadEvent>(failed).unwrap(), DownloadEvent::MetadataFailed { .. }));

        let updated = r#"{"status":"event","event":"download","state":"metadataupdated","track":{"id":"dQw4w9WgXcQ","title":"Never Gonna Give You Up","duration_seconds":213,"thumbnail_small":null,"thumbnail_large":null,"bpm":null,"camelot_key":null,"file_path":null,"added_at":null,"album":null,"artists":[]}}"#;
        assert_eq!(serde_json::from_str::<DownloadEvent>(updated).unwrap().id(), "dQw4w9WgXcQ");
    }

    #[test]
    fn parses_analyze_started() {
        let raw = r#"{"status":"event","event":"download","state":"analyzestarted","id":"dQw4w9WgXcQ","title":"Never Gonna Give You Up","thumbnail_small":"https://.../small.jpg"}"#;
        let event: DownloadEvent = serde_json::from_str(raw).unwrap();
        assert_eq!(event.id(), "dQw4w9WgXcQ");
        assert!(matches!(event, DownloadEvent::AnalyzeStarted { .. }));
    }

    #[test]
    fn parses_analyze_finished() {
        let raw = r#"{
            "status":"event",
            "event":"download",
            "state":"analyzefinished",
            "track": {
                "id": "dQw4w9WgXcQ",
                "title": "Never Gonna Give You Up",
                "duration_seconds": 213,
                "thumbnail_small": "https://.../small.jpg",
                "thumbnail_large": "https://.../large.jpg",
                "bpm": 113,
                "camelot_key": "8B",
                "file_path": "/cache/dQw4w9WgXcQ.opus",
                "added_at": null,
                "album": { "id": "MPREb_xxx", "name": "Whenever You Need Somebody" },
                "artists": [ { "id": "UCxxx", "name": "Rick Astley" } ]
            }
        }"#;
        let event: DownloadEvent = serde_json::from_str(raw).unwrap();
        assert_eq!(event.id(), "dQw4w9WgXcQ");
        match event {
            DownloadEvent::AnalyzeFinished { track } => {
                assert_eq!(track.bpm, Some(113));
                assert_eq!(track.camelot_key.as_deref(), Some("8B"));
                assert_eq!(track.file_path.as_deref(), Some("/cache/dQw4w9WgXcQ.opus"));
                assert_eq!(track.album.as_ref().map(|a| a.name.as_str()), Some("Whenever You Need Somebody"));
                assert_eq!(track.artists.len(), 1);
            }
            _ => panic!("esperaba AnalyzeFinished"),
        }
    }
}
