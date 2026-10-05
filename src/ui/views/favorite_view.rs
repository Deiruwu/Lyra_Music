use iced::{Element, Length, Task};
use iced::widget::image::Handle;
use iced::widget::{column, space};
use crate::model::Track;
use crate::ui::assets::icons::Icon;
use crate::ui::utils::async_thumbnail::AsyncThumbnail;
use crate::ui::views::catalog_store::CatalogStore;
use crate::ui::views::states_view::{ListAction, TrackViewState};
use crate::ui::views::view_data::{NavId, ViewData};
use crate::ui::widgets::catalog_search_input::catalog_search_input;
use crate::ui::widgets::catalog_status_message::{catalog_status_message, StatusTone};
use crate::ui::widgets::track_list_builder::{TrackBuilder, TrackEvent};
use crate::ui::widgets::track_list_out_message::TrackListOutMessage;
use crate::ui::widgets::track_context_builder::TrackContextMenuBuilder;
use crate::ui::widgets::playlist_header::collection_header;

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
}

#[derive(Debug, Clone)]
pub enum FavoritesExtra {}

pub type FavoritesOutMessage = TrackListOutMessage<FavoritesExtra>;

// ─── ESTADO DE LA VISTA ─────────────────────────────────────────

pub struct FavoritesView {
    pub list: TrackViewState,
}

// ─── IMPLEMENTACIÓN ─────────────────────────────────────────────

impl FavoritesView {
    pub fn new() -> Self {
        Self {
            list: TrackViewState::new(),
        }
    }

    pub fn update(
        &mut self,
        msg: FavoritesMessage,
        rendered_tracks: &[&Track],
        playlists: &[(String, String)],
        catalog_store: &CatalogStore,
    ) -> (Task<FavoritesMessage>, FavoritesOutMessage) {
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
        };

        (Task::none(), out)
    }

    pub fn view<'a>(
        &'a self,
        rendered_tracks: Vec<&'a Track>,
        thumbnails: &'a AsyncThumbnail,
        mosaic: Vec<Option<Handle>>,
        now_playing_id: Option<String>,
        is_playing: bool,
    ) -> Element<'a, FavoritesMessage> {
        let header = collection_header(
            "PLAYLIST",
            "Me gusta",
            &rendered_tracks,
            mosaic,
            now_playing_id.is_some(),
            is_playing,
            FavoritesMessage::PlayAll,
            FavoritesMessage::TogglePlayback,
        );

        let search_bar = catalog_search_input(
            "Buscar en tus favoritos...",
            &self.list.search_filter,
            FavoritesMessage::SearchInputChanged,
        );

        let fixed_header = column![
            header,
            space().height(Length::Fixed(16.0)),
            search_bar,
        ];

        let body_content: Element<'_, FavoritesMessage> = if rendered_tracks.is_empty() {
            catalog_status_message(
                "Aún no has marcado ninguna canción con \"Me gusta\".",
                StatusTone::Muted,
            )
        } else {
            let tracks_refs: Vec<&Track> = rendered_tracks;

            TrackBuilder::new(
                tracks_refs,
                &self.list.scroll,
                thumbnails,
                &self.list.tracks_selection.selected_ids,
                "favorites_catalog_scroll",
            )
                .index_sortable()
                .sort(self.list.active_sort_key, self.list.sort_direction_asc)
                .playing(now_playing_id, is_playing)
                .icon_hovered(self.list.playing_icon_hovered)
                .on_event(FavoritesMessage::Table)
                .build()
        };

        column![
            fixed_header,
            space().height(Length::Fixed(16.0)),
            body_content,
        ]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}