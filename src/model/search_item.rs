use serde::{Deserialize, Serialize};
use crate::model::{Artist, ArtistProfileDto, Track};

/// Álbum tal como llega en un resultado de búsqueda (sin tracks).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlbumSearchResult {
    pub id: String,
    pub name: String,
    pub thumbnail_small: Option<String>,
    pub thumbnail_large: Option<String>,
    /// Texto crudo de YT Music ("Album", "Single", "EP"...).
    #[serde(rename = "type")]
    pub album_type: Option<String>,
    pub year: Option<String>,
    #[serde(default)]
    pub artists: Vec<Artist>,
}

/// Resultado heterogéneo de la acción `search_items`, tagueado por `kind`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum SearchItem {
    Track(Track),
    Album(AlbumSearchResult),
    Artist(ArtistProfileDto),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_mixed_search_items() {
        let raw = r#"[
            {"kind": "artist", "id": "UCRr1xG_2WIDs18a6cIiCxeA", "name": "Daft Punk",
             "thumbnail_small": "https://lh3.googleusercontent.com/x=w120-h120-p-l90-rj",
             "thumbnail_large": "https://lh3.googleusercontent.com/x=w544-h544-p-l90-rj"},
            {"kind": "track", "id": "khnokW3Mw24", "title": "Instant Crush", "duration_seconds": 338,
             "thumbnail_small": null, "thumbnail_large": null, "bpm": null, "camelot_key": null,
             "file_path": null, "added_at": null,
             "album": {"id": "MPREb_K8qWMWVqXGi", "name": "Random Access Memories"},
             "artists": [{"id": "UCRr1xG_2WIDs18a6cIiCxeA", "name": "Daft Punk"}]},
            {"kind": "album", "id": "MPREb_7ltM34kr0mH", "name": "Discovery",
             "thumbnail_small": null, "thumbnail_large": null, "type": "Album", "year": "2001",
             "artists": [{"id": "UCRr1xG_2WIDs18a6cIiCxeA", "name": "Daft Punk"}]}
        ]"#;

        let items: Vec<SearchItem> = serde_json::from_str(raw).expect("payload válido");

        assert!(matches!(&items[0], SearchItem::Artist(a) if a.name == "Daft Punk"));
        assert!(matches!(&items[1], SearchItem::Track(t) if t.id == "khnokW3Mw24" && t.artists.len() == 1));
        assert!(matches!(&items[2], SearchItem::Album(a) if a.year.as_deref() == Some("2001")));
    }
}
