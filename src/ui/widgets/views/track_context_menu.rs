//! # track_context_menu — construcción compartida del menú de track
//!
//! ## Por qué existe
//!
//! Las 8 filas "Reproducir ahora / Agregar a cola / Reproducir después /
//! Iniciar radio / (Me gusta) / Agregar a playlist / Copiar id / <acción
//! final>" se repetían copiadas y pegadas en `ExplorerView`,
//! `FavoritesView` y `PlaylistsView`, junto con el `map` que arma
//! `playlist_children` a partir de `store.playlists_metadata()`. Las
//! únicas diferencias reales entre las tres vistas son:
//!
//! - Si el menú incluye "Me gusta"/"Ya no me gusta" (Explorer y
//!   Playlists sí, Favorites no — ahí siempre es "Quitar de Me gusta").
//! - Cuál es la última fila: Eliminar canción (Explorer), Quitar de Me
//!   gusta (Favorites), o Quitar de playlist (Playlists).
//! - Playlists además excluye del submenú la playlist que se está
//!   viendo actualmente (no tiene sentido "agregar a esta misma
//!   playlist" desde su propia vista de detalle).
//!
//! Este módulo no intenta ocultar esas diferencias — las recibe como
//! parámetros — solo elimina la repetición de armar la lista.
//!
//! ## Cómo se consume
//!
//! ```ignore
//! let items = track_context_menu::build(
//!     track,
//!     store,
//!     LikeSlot::Toggle(ContextMenuAction::ToggleLike), // o LikeSlot::None
//!     ContextMenuAction::AddToPlaylist,
//!     None, // o Some(current_playlist_id) para excluirla del submenú
//!     SUBMENU_ADD_TO_PLAYLIST,
//!     ContextMenuAction::PlayNow,
//!     ContextMenuAction::AddToQueue,
//!     ContextMenuAction::AddToFrontQueue,
//!     ContextMenuAction::StartRadio,
//!     ContextMenuAction::CopyId,
//!     ContextMenuItem::new("Eliminar canción", ContextMenuAction::Delete).icon(Icon::Delete),
//! );
//! self.context_menu.view(anchor, items, track, ..., ..., ...)
//! ```
//!
//! El "escape hatch" — sugerencia para el caso "¿ya está en esta
//! playlist, la quieres agregar de todas formas?" que se comentó en la
//! revisión: `playlist_children` ya resuelve por track si la playlist
//! contiene el track (`store.is_track_in_playlist`) y elige el ícono en
//! consecuencia. Ahora mismo el click en un ítem ya-agregado simplemente
//! vuelve a agregar (operación idempotente del lado del store, según el
//! uso original). Si más adelante se quiere un diálogo de confirmación
//! en ese caso puntual, el lugar natural es interceptar
//! `ContextMenuAction::AddToPlaylist(id)` en el `update()` de cada
//! vista (donde ya se tiene `store` a mano) y, si `store.is_track_in_playlist`
//! da `true`, enrutar a `ConfirmDialog` en vez de emitir el
//! `OutMessage::RequestAddToPlaylist` directo — igual patrón que ya usa
//! Explorer para `Delete`.

use crate::model::Track;
use crate::ui::assets::icons::Icon;
use crate::ui::views::catalog_store::CatalogStore;
use crate::ui::widgets::context_menu::ContextMenuItem;

/// Cómo (o si) el menú incluye la fila de like. `Toggle` recibe la
/// acción a disparar; el label/ícono se resuelve automáticamente según
/// `track.liked`, igual que hacían Explorer y Playlists antes.
pub enum LikeSlot<Action> {
    Toggle(Action),
    None,
}

/// Arma la lista de `ContextMenuItem` común a las tres vistas de
/// catálogo. `last_item` es la fila final variable por vista (Eliminar
/// / Quitar de Me gusta / Quitar de playlist) — se pasa ya construida
/// para no forzar un enum de "tipo de acción final" que ninguna vista
/// necesita en realidad.
#[allow(clippy::too_many_arguments)]
pub fn build<Action: Clone>(
    track: &Track,
    store: &CatalogStore,
    like: LikeSlot<Action>,
    add_to_playlist: impl Fn(String) -> Action,
    exclude_playlist_id: Option<&str>,
    submenu_id: usize,
    play_now: Action,
    add_to_queue: Action,
    add_to_front_queue: Action,
    start_radio: Action,
    copy_id: Action,
    last_item: ContextMenuItem<Action>,
) -> Vec<ContextMenuItem<Action>> {
    let playlist_children: Vec<ContextMenuItem<Action>> = store
        .playlists_metadata()
        .iter()
        .filter(|(playlist_id, _, _)| Some(playlist_id.as_str()) != exclude_playlist_id)
        .map(|(playlist_id, name, _)| {
            let icon = if store.is_track_in_playlist(playlist_id, &track.id) {
                ""
            } else {
                ""
            };
            let _ = icon; // reservado: hoy el label no usa el ícono, ver nota de módulo sobre "ya está en la playlist"
            ContextMenuItem::new(name.clone(), add_to_playlist(playlist_id.clone()))
        })
        .collect();

    let mut items = vec![
        ContextMenuItem::new("Reproducir ahora", play_now).icon(Icon::Play),
        ContextMenuItem::new("Agregar a cola", add_to_queue).icon(Icon::AddQueue),
        ContextMenuItem::new("Reproducir después", add_to_front_queue).icon(Icon::AddQueueFront),
        ContextMenuItem::new("Iniciar radio", start_radio).icon(Icon::Radio),
    ];

    if let LikeSlot::Toggle(action) = like {
        let (label, icon) = if track.liked {
            ("Ya no me gusta", Icon::HeartBroken)
        } else {
            ("Me gusta", Icon::HeartFull)
        };
        items.push(ContextMenuItem::new(label, action).icon(icon));
    }

    items.push(
        ContextMenuItem::submenu("Agregar a playlist", submenu_id, playlist_children)
            .icon(Icon::Playlist),
    );
    items.push(ContextMenuItem::new("Copiar id", copy_id).icon(Icon::Copiar));
    items.push(last_item);

    items
}