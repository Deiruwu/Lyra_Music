use iced::Point;

use crate::model::track_state::TrackState;
use crate::ui::widgets::context_menu_V2::ContextMenuItem;
use crate::ui::widgets::track_context_builder::TrackContextAction;

/// Lo mínimo que el padre necesita para decidir si pide descarga de
/// color, gris, o ninguna — sin tener que volver a buscar el Track
/// completo en CatalogStore. `key` ya viene resuelto con thumb_key()
/// (album_id si existe, si no track_id) porque esa lógica vive en
/// ThumbnailCache/track_row, no debería duplicarse acá.
#[derive(Debug, Clone)]
pub struct VisibleTrackRef {
    pub track_id: String,
    pub color_key: String,
    pub artwork_url: Option<String>,
    pub state: TrackState,
}

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
    RequestEnqueue(Vec<String>),
    RequestFrontEnqueue(Vec<String>),
    RequestPlayRadio(String),

    // ─── Mutación de BD (Dominio) común ─────────────────────────
    RequestToggleLike(Vec<String>),
    RequestAddToPlaylist { target_playlist_id: String, track_ids: Vec<String> },

    // ─── Mutación de Estado Global (Filtros y Ordenamiento) ─────
    RequestSearch(String),
    RequestChangeSort(usize),

    // ─── Context menu de canción ─────────────────────────────────
    ContextMenuRightClicked {
        track_id: String,
        items: Vec<ContextMenuItem<TrackContextAction>>,
    },

    // ─── Miniaturas ────────────────────────────────────────────────
    /// Emitido en TrackEvent::Scrolled: los tracks de la ventana
    /// visible (más buffer) que no tenían Handle cacheado en el render
    /// más reciente. El padre (dueño del ThumbnailCache real) decide
    /// si pide color, gris, o ambos, y con qué epoch — la vista solo
    /// informa "estos son los que se ven ahora y no tienen imagen".
    ThumbnailsNeeded(Vec<VisibleTrackRef>),

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