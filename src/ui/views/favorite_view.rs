use iced::{Element, Length, Task};
use iced::widget::{column, space, text};
use iced::keyboard::Modifiers;
use iced::Color;
use crate::JETBRAINS_MONO;
use crate::model::Track;
use crate::ui::assets::fonts::SF_PRO;
use crate::ui::assets::icons::Icon;
use crate::ui::utils::async_thumbnail::AsyncThumbnail;
use crate::ui::views::states_view::{ListAction, TrackViewState};
use crate::ui::views::view_data::{NavId, ViewData};
use crate::ui::widgets::catalog_search_input::catalog_search_input;
use crate::ui::widgets::catalog_status_message::{catalog_status_message, StatusTone};
use crate::ui::widgets::track_list_builder::{sort_tracks, TrackBuilder, TrackEvent};
use crate::ui::widgets::track_list_out_message::TrackListOutMessage;
use crate::ui::widgets::track_context_builder::TrackContextMenuBuilder;

pub const VIEW_DATA: ViewData = ViewData::new(
    NavId::Favorites,
    Icon::HeartFull,
    "Me gusta",
    JETBRAINS_MONO,
);

// ─── MENSAJES INTERNOS ────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum FavoritesMessage {
    SearchInputChanged(String),
    KeybindsChanged(Modifiers),
    Table(TrackEvent),
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
    ) -> (Task<FavoritesMessage>, FavoritesOutMessage) {
        let mut out = FavoritesOutMessage::Idle;

        match &msg {
            // ─── EVENTOS DE LA TABLA (TrackBuilder) ────────────────────────
            FavoritesMessage::Table(event) => {
                let action = self.list.process_event(event.clone(), rendered_tracks);

                out = match action {
                    ListAction::PlayContext(id) => FavoritesOutMessage::RequestPlayContext { start_track_id: id },
                    ListAction::SortChanged(key) => FavoritesOutMessage::RequestChangeSort(key),
                    ListAction::None => FavoritesOutMessage::Idle,

                    ListAction::OpenContextMenu { anchor_id, selected_ids: _ } => {
                        let is_liked = true;

                        let items = TrackContextMenuBuilder::new(is_liked)
                            .with_playlists(playlists, None)
                            .build();

                        FavoritesOutMessage::ContextMenuRightClicked {
                            track_id: anchor_id, // Usamos el ancla
                            items,
                        }
                    }
                };
            }

            // ─── EVENTOS INTERNOS ───────────────────────────────
            FavoritesMessage::SearchInputChanged(query) => {
                self.list.apply_search_filter(query.clone());
                out = FavoritesOutMessage::RequestSearch(query.clone());
            }

            FavoritesMessage::KeybindsChanged(modifiers) => {
                self.list.keybinds_press = *modifiers;
            }
        };

        let _ = rendered_tracks;
        (Task::none(), out)
    }

    pub fn view<'a>(
        &'a self,
        rendered_tracks: Vec<&'a Track>,
        thumbnails: &'a AsyncThumbnail,
    ) -> Element<'a, FavoritesMessage> {
        let title = text("Me gusta")
            .size(28)
            .font(SF_PRO)
            .style(|_| text::Style { color: Some(Color::WHITE) });

        let search_bar = catalog_search_input(
            "Buscar en tus favoritos...",
            &self.list.search_filter,
            FavoritesMessage::SearchInputChanged,
        );

        let fixed_header = column![
            title,
            space().height(Length::Fixed(12.0)),
            search_bar,
        ];

        let body_content: Element<'_, FavoritesMessage> = if rendered_tracks.is_empty() {
            catalog_status_message(
                "Aún no has marcado ninguna canción con \"Me gusta\".",
                StatusTone::Muted,
            )
        } else {
            let mut tracks_refs: Vec<&Track> = rendered_tracks;
            sort_tracks(&mut tracks_refs, self.list.active_sort_key, self.list.sort_direction_asc);

            TrackBuilder::new(
                tracks_refs,
                &self.list.scroll,
                thumbnails,
                &self.list.tracks_selection.selected_ids,
                "favorites_catalog_scroll",
            )
                .index_sortable()
                .sort(self.list.active_sort_key, self.list.sort_direction_asc)
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