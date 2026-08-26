use std::collections::HashSet;
use crate::ui::widgets::context_menu::ContextMenuItem;
use crate::ui::widgets::track_context_builder::TrackContextAction;

/// Salida común a toda vista basada en TrackBuilder (Explorer, Favorites,
/// Playlist). Antes cada vista tenía su propio enum *OutMessage con las
/// mismas ~8 variantes de audio/BD/filtro copiadas y pegadas — esto las deja
/// en un solo lugar. Lo que sí es propio de cada vista (borrar del catálogo,
/// reordenar, quitar de playlist) va en `Extra`, definido por cada vista.
#[derive(Debug, Clone)]
pub enum TrackListOutMessage<Extra> {
    Idle,

    // ─── Audio Engine ───────────────────────────────────────────
    RequestPlayContext { start_track_id: String },
    /// "Reproducir todo" sin track puntual — en shuffle, sortea la
    /// lista completa antes de decidir cuál va primero (ver
    /// `TrackManager::play_context_shuffled`).
    RequestPlayAll,
    RequestEnqueue(Vec<String>),
    RequestFrontEnqueue(Vec<String>),
    RequestPlayRadio(String),
    RequestTogglePlayback,

    // ─── Mutación de BD (Dominio) común ─────────────────────────
    RequestToggleLike(Vec<String>),
    RequestAddToPlaylist { target_playlist_id: String, track_ids: Vec<String> },

    // ─── Mutación de Estado Global (Filtros y Ordenamiento) ─────
    RequestSearch(String),
    RequestChangeSort(usize),

    // ─── Navegación ──────────────────────────────────────────────
    RequestOpenArtist(String),
    RequestOpenAlbum(String),

    // ─── Context menu de canción ─────────────────────────────────
    ContextMenuRightClicked {
        track_id: String,
        items: Vec<ContextMenuItem<TrackContextAction>>,
        selected_ids: HashSet<String>,
    },

    // ─── Lo propio de cada vista ─────────────────────────────────
    Extra(Extra),
}

impl<Extra> TrackListOutMessage<Extra> {
    /// Azúcar para que las vistas no tengan que escribir
    /// `TrackListOutMessage::Extra(MiExtra::Foo)` a cada rato.
    pub fn extra(value: Extra) -> Self {
        TrackListOutMessage::Extra(value)
    }
}