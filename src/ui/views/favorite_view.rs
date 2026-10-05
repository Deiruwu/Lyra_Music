use iced::{Color, Element, Task};
use iced::widget::image::Handle;
use iced::widget::Id;
use crate::model::Track;
use crate::ui::assets::icons::Icon;
use crate::ui::cover_palette;
use crate::ui::utils::async_thumbnail::AsyncThumbnail;
use crate::ui::views::catalog_store::CatalogStore;
use crate::ui::views::states_view::{ListAction, TrackViewState};
use crate::ui::views::view_data::{NavId, ViewData};
use crate::ui::widgets::collection_page::{action_bar, contains_now_playing, empty_page, filter_input, icon_toggle, mosaic_header, ACTION_BAR_HEIGHT, CONTENT_PADDING_X, ROWS_OFFSET};
use crate::ui::widgets::playlist_header::{play_button, EDGE_HEADER_HEIGHT};
use crate::ui::widgets::track_list_builder::{TrackBuilder, TrackEvent};
use crate::ui::widgets::track_list_out_message::TrackListOutMessage;
use crate::ui::widgets::track_context_builder::TrackContextMenuBuilder;

pub const VIEW_DATA: ViewData = ViewData::new(
    NavId::Favorites,
    Icon::HeartFull,
    "Me gusta",
);

// ─── MENSAJES INTERNOS ────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum FavoritesMessage {
    SearchInputChanged(String),
    Table(TrackEvent),
    PlayAll,
    TogglePlayback,
    ToggleFilter,
    ToggleShuffle,
}

#[derive(Debug, Clone)]
pub enum FavoritesExtra {
    ToggleShuffle,
}

const FILTER_INPUT_ID: &str = "favorites_filter_input";

pub type FavoritesOutMessage = TrackListOutMessage<FavoritesExtra>;

// ─── ESTADO DE LA VISTA ─────────────────────────────────────────

pub struct FavoritesView {
    pub list: TrackViewState,
    /// Campo de filtro visible (lupa activa).
    filter_open: bool,
}

// ─── IMPLEMENTACIÓN ─────────────────────────────────────────────

impl FavoritesView {
    pub fn new() -> Self {
        let mut list = TrackViewState::new();
        list.rows_offset = ROWS_OFFSET;
        Self { list, filter_open: false }
    }

    /// Ctrl+F: muestra el filtro de la página y le da foco con el texto seleccionado.
    pub fn open_filter(&mut self) -> Task<FavoritesMessage> {
        self.filter_open = true;
        let id = Id::new(FILTER_INPUT_ID);
        Task::batch([iced::widget::operation::focus(id.clone()), iced::widget::operation::select_all(id)])
    }

    pub fn update(
        &mut self,
        msg: FavoritesMessage,
        rendered_tracks: &[&Track],
        playlists: &[(String, String)],
        catalog_store: &CatalogStore,
    ) -> (Task<FavoritesMessage>, FavoritesOutMessage) {
        let mut task = Task::none();
        let out = match &msg {
            // ─── EVENTOS DE LA TABLA (TrackBuilder) ────────────────────────
            FavoritesMessage::Table(event) => {
                let action = self.list.process_event(event.clone(), rendered_tracks);

                match action {
                    ListAction::PlayContext(id) => FavoritesOutMessage::RequestPlayContext { start_track_id: id },
                    ListAction::SortChanged(key) => FavoritesOutMessage::RequestChangeSort(key),
                    ListAction::OpenArtist(id) => FavoritesOutMessage::RequestOpenArtist(id),
                    ListAction::OpenAlbum(id) => FavoritesOutMessage::RequestOpenAlbum(id),
                    ListAction::TogglePlayback => FavoritesOutMessage::RequestTogglePlayback,
                    ListAction::None => FavoritesOutMessage::Idle,

                    ListAction::OpenContextMenu { anchor_id, selected_ids } => {
                        let is_liked = true;
                        let member_of = catalog_store.playlists_containing_track(&anchor_id);

                        let is_downloaded = catalog_store.track_by_id(&anchor_id).is_some_and(|t| t.file_path.is_some());
                        let items = TrackContextMenuBuilder::new(is_liked)
                            .with_playlists(playlists, None, &member_of)
                            .with_tools(is_downloaded)
                            .build();

                        FavoritesOutMessage::ContextMenuRightClicked {
                            track_id: anchor_id, // Usamos el ancla
                            items,
                            selected_ids,
                        }
                    }
                }
            }

            // ─── EVENTOS INTERNOS ───────────────────────────────
            FavoritesMessage::SearchInputChanged(query) => {
                self.list.apply_search_filter(query.clone());
                FavoritesOutMessage::RequestSearch(query.clone())
            }
            FavoritesMessage::PlayAll => FavoritesOutMessage::RequestPlayAll,
            FavoritesMessage::TogglePlayback => FavoritesOutMessage::RequestTogglePlayback,
            FavoritesMessage::ToggleShuffle => FavoritesOutMessage::extra(FavoritesExtra::ToggleShuffle),
            FavoritesMessage::ToggleFilter => {
                self.filter_open = !self.filter_open;
                if self.filter_open {
                    task = iced::widget::operation::focus(Id::new(FILTER_INPUT_ID));
                    FavoritesOutMessage::Idle
                } else if !self.list.search_filter.is_empty() {
                    // Al cerrar la lupa no queda un filtro escondido.
                    self.list.apply_search_filter(String::new());
                    FavoritesOutMessage::RequestSearch(String::new())
                } else {
                    FavoritesOutMessage::Idle
                }
            }
        };

        (task, out)
    }

    pub fn view<'a>(
        &'a self,
        rendered_tracks: Vec<&'a Track>,
        thumbnails: &'a AsyncThumbnail,
        mosaic: Vec<Option<Handle>>,
        base_color: Color,
        now_playing_id: Option<String>,
        is_playing: bool,
        is_shuffled: bool,
    ) -> Element<'a, FavoritesMessage> {
        let header = mosaic_header("PLAYLIST", "Me gusta", &rendered_tracks, mosaic, base_color);
        let is_current = contains_now_playing(&rendered_tracks, now_playing_id.as_deref());
        let toolbar = self.view_action_bar(is_current, is_current && is_playing, is_shuffled);
        let band = cover_palette::band_tint(base_color);

        if rendered_tracks.is_empty() {
            let message = if self.list.search_filter.trim().is_empty() {
                "Aún no has marcado ninguna canción con \"Me gusta\"."
            } else {
                "Nada en tus favoritos coincide con el filtro."
            };
            return empty_page(header, toolbar, message, band);
        }

        TrackBuilder::new(
            rendered_tracks,
            &self.list.scroll,
            thumbnails,
            &self.list.tracks_selection.selected_ids,
            "favorites_catalog_scroll",
        )
            .leading(header, EDGE_HEADER_HEIGHT)
            .toolbar(toolbar, ACTION_BAR_HEIGHT)
            .band(band)
            .content_padding_x(CONTENT_PADDING_X)
            .index_sortable()
            .sort(self.list.active_sort_key, self.list.sort_direction_asc)
            .playing(now_playing_id, is_playing)
            .icon_hovered(self.list.playing_icon_hovered)
            .on_event(FavoritesMessage::Table)
            .build()
    }

    /// Reproducir y aleatorio; a la derecha el filtro (lupa).
    fn view_action_bar(&self, is_current: bool, is_playing: bool, is_shuffled: bool) -> Element<'_, FavoritesMessage> {
        let play_message = if is_current { FavoritesMessage::TogglePlayback } else { FavoritesMessage::PlayAll };
        let left = vec![
            play_button(is_playing, play_message),
            icon_toggle(Icon::Shuffle, is_shuffled, FavoritesMessage::ToggleShuffle),
        ];

        let mut right = Vec::new();
        if self.filter_open {
            right.push(filter_input("Filtrar tus favoritos…", &self.list.search_filter, FavoritesMessage::SearchInputChanged, FILTER_INPUT_ID));
        }
        right.push(icon_toggle(Icon::Search, self.filter_open, FavoritesMessage::ToggleFilter));

        action_bar(left, right)
    }
}
