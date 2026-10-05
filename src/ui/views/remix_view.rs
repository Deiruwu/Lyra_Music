//! Remix: habilitas playlists y se arma una mezcla con sus canciones, intercaladas
//! (una de cada playlist por vuelta) y sin repetidas.
//! Las playlists se eligen en un panel lateral que ocupa la columna de la cola.

use std::collections::HashSet;

use iced::border::{rounded, Radius};
use iced::widget::{button, column, container, row, scrollable, space, text, Id};
use iced::{Alignment, Color, Element, Length, Padding, Task, Theme};

use crate::model::Track;
use crate::ui::assets::fonts::SF_PRO;
use crate::ui::assets::icons::{self, Icon};
use crate::ui::assets::{radii, spacing, typography};
use crate::ui::cover_palette;
use crate::ui::styles::button as button_style;
use crate::ui::styles::container as container_style;
use crate::ui::styles::scrollable as scrollable_style;
use crate::ui::theme::theme;
use crate::ui::utils::async_thumbnail::AsyncThumbnail;
use crate::ui::utils::cover_manager::{CoverManager, CoverVariant};
use crate::ui::utils::playlist_metadata::{format_total_duration, format_track_count};
use crate::ui::views::catalog_store::CatalogStore;
use crate::ui::views::states_view::{ListAction, TrackViewState};
use crate::ui::views::view_data::{NavId, ViewData};
use crate::ui::widgets::collection_page::{action_bar, contains_now_playing, empty_page, filter_input, icon_toggle, ACTION_BAR_HEIGHT, CONTENT_PADDING_X, ROWS_OFFSET};
use crate::ui::widgets::cover_collage::{cover_tile, CollageTile, MAX_TILES};
use crate::ui::widgets::playlist_header::{play_button, playlist_header, HeaderCover, PlaylistHeaderData, EDGE_HEADER_HEIGHT};
use crate::ui::widgets::single_line_text::single_line_text;
use crate::ui::widgets::track_context_builder::TrackContextMenuBuilder;
use crate::ui::widgets::track_list_builder::{TrackBuilder, TrackEvent};
use crate::ui::widgets::track_list_out_message::TrackListOutMessage;

pub const VIEW_DATA: ViewData = ViewData::new(NavId::Remix, Icon::Shuffle, "Remix");

pub const SCROLL_ID: &str = "remix_catalog_scroll";
const FILTER_INPUT_ID: &str = "remix_filter_input";
const PICKER_COVER_SIZE: f32 = 44.0;
const PICKER_COVER_RADIUS: f32 = 6.0;
const CHECKBOX_SIZE: f32 = 18.0;

#[derive(Debug, Clone)]
pub enum RemixMessage {
    Table(TrackEvent),
    TogglePlaylist(String),
    SelectAll,
    DeselectAll,
    PlayAll,
    TogglePlayback,
    SearchInputChanged(String),
    ToggleFilter,
    /// Abre o cierra el panel lateral para elegir las playlists.
    TogglePicker,
}

#[derive(Debug, Clone)]
pub enum RemixExtra {}

pub type RemixOutMessage = TrackListOutMessage<RemixExtra>;

pub struct RemixView {
    pub list: TrackViewState,
    /// Playlists habilitadas para la mezcla (se guardan en los ajustes).
    pub enabled: Vec<String>,
    /// Panel lateral de playlists abierto.
    picker_open: bool,
    /// Campo de filtro visible (lupa activa).
    filter_open: bool,
}

impl RemixView {
    pub fn new(enabled: Vec<String>) -> Self {
        let mut list = TrackViewState::new();
        list.rows_offset = ROWS_OFFSET;
        Self { list, enabled, picker_open: false, filter_open: false }
    }

    /// Canciones de la mezcla: una de cada playlist habilitada por vuelta, en el orden
    /// del sidebar y de cada playlist, sin repetidas.
    pub fn source<'a>(&self, catalog: &'a CatalogStore) -> Vec<&'a Track> {
        let pools: Vec<Vec<&'a Track>> = self.enabled_in_order(catalog).into_iter().map(|id| catalog.tracks_for_playlist(id)).collect();
        interleave_unique(&pools, |track| track.id.clone())
    }

    pub fn is_picker_open(&self) -> bool {
        self.picker_open
    }

    /// Cierra el panel de playlists; `true` si estaba abierto.
    pub fn close_picker(&mut self) -> bool {
        std::mem::take(&mut self.picker_open)
    }

    /// Playlists habilitadas que siguen existiendo, en el orden del sidebar.
    fn enabled_in_order<'a>(&self, catalog: &'a CatalogStore) -> Vec<&'a str> {
        catalog
            .playlists_metadata()
            .iter()
            .filter(|(id, _, _)| self.enabled.contains(id))
            .map(|(id, _, _)| id.as_str())
            .collect()
    }

    /// Color del header: el más vivo de las playlists habilitadas.
    pub fn base_color(&self, catalog: &CatalogStore) -> Color {
        cover_palette::most_vivid(self.enabled_in_order(catalog).into_iter().map(cover_palette::playlist_color))
            .unwrap_or_else(|| cover_palette::fallback_color("remix"))
    }

    /// Portadas grandes que pide el collage del header.
    pub fn collage_playlist_ids<'a>(&self, catalog: &'a CatalogStore) -> Vec<&'a str> {
        self.enabled_in_order(catalog).into_iter().take(MAX_TILES).collect()
    }

    /// Ctrl+F: muestra el filtro de la página y le da foco con el texto seleccionado.
    pub fn open_filter(&mut self) -> Task<RemixMessage> {
        self.filter_open = true;
        let id = Id::new(FILTER_INPUT_ID);
        Task::batch([iced::widget::operation::focus(id.clone()), iced::widget::operation::select_all(id)])
    }

    pub fn update(
        &mut self,
        message: RemixMessage,
        rendered_tracks: &[&Track],
        playlists: &[(String, String)],
        catalog: &CatalogStore,
    ) -> (Task<RemixMessage>, RemixOutMessage) {
        let mut task = Task::none();
        let out = match message {
            RemixMessage::Table(event) => match self.list.process_event(event, rendered_tracks) {
                ListAction::PlayContext(id) => RemixOutMessage::RequestPlayContext { start_track_id: id },
                ListAction::SortChanged(key) => RemixOutMessage::RequestChangeSort(key),
                ListAction::OpenArtist(id) => RemixOutMessage::RequestOpenArtist(id),
                ListAction::OpenAlbum(id) => RemixOutMessage::RequestOpenAlbum(id),
                ListAction::TogglePlayback => RemixOutMessage::RequestTogglePlayback,
                ListAction::None => RemixOutMessage::Idle,
                ListAction::OpenContextMenu { anchor_id, selected_ids } => {
                    let member_of = catalog.playlists_containing_track(&anchor_id);
                    let is_downloaded = catalog.track_by_id(&anchor_id).is_some_and(|t| t.file_path.is_some());
                    let items = TrackContextMenuBuilder::new(catalog.is_liked(&anchor_id))
                        .with_playlists(playlists, None, &member_of)
                        .with_tools(is_downloaded)
                        .build();
                    RemixOutMessage::ContextMenuRightClicked { track_id: anchor_id, items, selected_ids }
                }
            },
            RemixMessage::TogglePlaylist(playlist_id) => {
                if let Some(index) = self.enabled.iter().position(|id| id == &playlist_id) {
                    self.enabled.remove(index);
                } else {
                    self.enabled.push(playlist_id);
                }
                self.list.invalidate_cache();
                RemixOutMessage::Idle
            }
            RemixMessage::SelectAll => {
                self.enabled = catalog.playlists_metadata().iter().map(|(id, _, _)| id.clone()).collect();
                self.list.invalidate_cache();
                RemixOutMessage::Idle
            }
            RemixMessage::DeselectAll => {
                self.enabled.clear();
                self.list.invalidate_cache();
                RemixOutMessage::Idle
            }
            RemixMessage::PlayAll => RemixOutMessage::RequestPlayAll,
            RemixMessage::TogglePlayback => RemixOutMessage::RequestTogglePlayback,
            RemixMessage::SearchInputChanged(query) => {
                self.list.apply_search_filter(query.clone());
                RemixOutMessage::RequestSearch(query)
            }
            RemixMessage::ToggleFilter => {
                self.filter_open = !self.filter_open;
                if self.filter_open {
                    task = iced::widget::operation::focus(Id::new(FILTER_INPUT_ID));
                    RemixOutMessage::Idle
                } else if !self.list.search_filter.is_empty() {
                    // Al cerrar la lupa no queda un filtro escondido.
                    self.list.apply_search_filter(String::new());
                    RemixOutMessage::RequestSearch(String::new())
                } else {
                    RemixOutMessage::Idle
                }
            }
            RemixMessage::TogglePicker => {
                self.picker_open = !self.picker_open;
                RemixOutMessage::Idle
            }
        };

        (task, out)
    }

    pub fn view<'a>(
        &'a self,
        rendered_tracks: Vec<&'a Track>,
        thumbnails: &'a AsyncThumbnail,
        collage: Vec<CollageTile>,
        now_playing_id: Option<String>,
        is_playing: bool,
        catalog: &'a CatalogStore,
    ) -> Element<'a, RemixMessage> {
        let base = self.base_color(catalog);
        let enabled_count = self.enabled_in_order(catalog).len();
        let header = playlist_header(
            PlaylistHeaderData {
                name: "Remix",
                kicker: Some("REMIX"),
                description: match enabled_count {
                    0 => None,
                    1 => Some("Mezcla de 1 playlist".to_string()),
                    count => Some(format!("Mezcla de {count} playlists")),
                },
                track_count: rendered_tracks.len(),
                total_duration_seconds: rendered_tracks.iter().map(|t| t.duration_seconds as i64).sum(),
                tint: Some(cover_palette::header_tint(base)),
                tint_end: Some(cover_palette::header_tint_end(base)),
                lyrics_count: None,
                edge_to_edge: true,
            },
            HeaderCover::Collage(collage),
            None,
            None,
            None,
            false,
        );

        let is_current = contains_now_playing(&rendered_tracks, now_playing_id.as_deref());
        let toolbar = self.view_action_bar(is_current, is_current && is_playing);
        let band = cover_palette::band_tint(base);

        if rendered_tracks.is_empty() {
            let message = if enabled_count == 0 {
                "Elige una o más playlists con el botón + para mezclarlas."
            } else if self.list.search_filter.trim().is_empty() {
                "Las playlists elegidas están vacías."
            } else {
                "Nada en la mezcla coincide con el filtro."
            };
            return empty_page(header, toolbar, message, band);
        }

        TrackBuilder::new(
            rendered_tracks,
            &self.list.scroll,
            thumbnails,
            &self.list.tracks_selection.selected_ids,
            SCROLL_ID,
        )
            .leading(header, EDGE_HEADER_HEIGHT)
            .toolbar(toolbar, ACTION_BAR_HEIGHT)
            .band(band)
            .content_padding_x(CONTENT_PADDING_X)
            .index_sortable()
            .sort(self.list.active_sort_key, self.list.sort_direction_asc)
            .playing(now_playing_id, is_playing)
            .icon_hovered(self.list.playing_icon_hovered)
            .on_event(RemixMessage::Table)
            .build()
    }

    /// Reproducir; a la derecha el filtro (lupa) y el panel de playlists (+).
    fn view_action_bar(&self, is_current: bool, is_playing: bool) -> Element<'_, RemixMessage> {
        let play_message = if is_current { RemixMessage::TogglePlayback } else { RemixMessage::PlayAll };

        let mut right = Vec::new();
        if self.filter_open {
            right.push(filter_input("Filtrar la mezcla…", &self.list.search_filter, RemixMessage::SearchInputChanged, FILTER_INPUT_ID));
        }
        right.push(icon_toggle(Icon::Search, self.filter_open, RemixMessage::ToggleFilter));
        right.push(icon_toggle(Icon::Add, self.picker_open, RemixMessage::TogglePicker));

        action_bar(vec![play_button(is_playing, play_message)], right)
    }

    /// Panel lateral: una fila por playlist con su casilla, portada, nombre y canciones.
    pub fn view_picker<'a>(&'a self, catalog: &'a CatalogStore, covers: &'a CoverManager) -> Element<'a, RemixMessage> {
        let selected = self.enabled_in_order(catalog).len();
        let title = row![
            column![
                text("Playlists del remix").font(SF_PRO).size(typography::TEXT_16).color(theme().content.primary),
                text(match selected {
                    0 => "Ninguna elegida".to_string(),
                    1 => "1 elegida".to_string(),
                    count => format!("{count} elegidas"),
                })
                    .font(SF_PRO)
                    .size(typography::TEXT_11)
                    .color(theme().content.muted),
            ]
                .spacing(spacing::SP_2),
            space().width(Length::Fill),
            button(text("Cerrar").font(SF_PRO).size(typography::TEXT_12).color(theme().content.secondary))
                .padding(Padding { top: spacing::SP_4, bottom: spacing::SP_4, left: spacing::SP_10, right: spacing::SP_10 })
                .style(button_style::pill(false))
                .on_press(RemixMessage::TogglePicker),
        ]
            .align_y(Alignment::Center);

        let total = catalog.playlists_metadata().len();
        let bulk_actions = row![
            bulk_button("Seleccionar todo", (selected < total).then_some(RemixMessage::SelectAll)),
            bulk_button("Deseleccionar todo", (selected > 0).then_some(RemixMessage::DeselectAll)),
        ]
            .spacing(spacing::SP_8);

        let rows = catalog.playlists_metadata().iter().map(|(id, name, _)| {
            let (track_count, duration) = catalog.playlist_track_stats(id);
            let tile = CollageTile {
                handle: covers.get(&CoverVariant::Small.key(id)).cloned(),
                color: cover_palette::header_tint(cover_palette::playlist_color(id)),
            };
            picker_row(name, track_count, duration, tile, self.enabled.contains(id), RemixMessage::TogglePlaylist(id.clone()))
        });

        container(
            column![
                title,
                bulk_actions,
                scrollable(column(rows).spacing(spacing::SP_4)).height(Length::Fill).style(scrollable_style::discreet),
            ]
                .spacing(spacing::SP_12),
        )
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(spacing::SP_16)
            .style(container_style::queue_panel)
            .into()
    }
}

/// Botón de "todo / nada" del panel; sin `on_press` cuando no cambiaría nada.
fn bulk_button<'a>(label: &'a str, on_press: Option<RemixMessage>) -> Element<'a, RemixMessage> {
    let color = if on_press.is_some() { theme().content.primary } else { theme().content.muted };
    let button = button(text(label).font(SF_PRO).size(typography::TEXT_12).color(color))
        .padding(Padding { top: spacing::SP_4, bottom: spacing::SP_4, left: spacing::SP_10, right: spacing::SP_10 })
        .style(button_style::pill(false));
    match on_press {
        Some(message) => button.on_press(message).into(),
        None => button.into(),
    }
}

/// Fila del panel: casilla, portada, nombre y "N canciones · duración"; toda la fila marca o desmarca.
fn picker_row<'a>(
    name: &'a str,
    track_count: usize,
    duration_seconds: i64,
    tile: CollageTile,
    is_checked: bool,
    on_toggle: RemixMessage,
) -> Element<'a, RemixMessage> {
    let info = column![
        single_line_text(name, SF_PRO, typography::TEXT_13, theme().content.primary, Length::Fill),
        single_line_text(
            format!("{} · {}", format_track_count(track_count), format_total_duration(duration_seconds)),
            SF_PRO,
            typography::TEXT_11,
            theme().content.muted,
            Length::Fill,
        ),
    ]
        .spacing(spacing::SP_2)
        .width(Length::Fill);

    let content = row![
        checkbox(is_checked),
        cover_tile(tile, PICKER_COVER_SIZE, PICKER_COVER_SIZE, Radius::new(PICKER_COVER_RADIUS)),
        info,
    ]
        .spacing(spacing::SP_12)
        .align_y(Alignment::Center);

    button(content)
        .width(Length::Fill)
        .padding(Padding { top: spacing::SP_6, bottom: spacing::SP_6, left: spacing::SP_8, right: spacing::SP_8 })
        .style(button_style::card_hover(radii::R_8))
        .on_press(on_toggle)
        .into()
}

/// Casilla cuadrada: con borde vacía, rellena con el acento y un check marcada.
fn checkbox<'a>(is_checked: bool) -> Element<'a, RemixMessage> {
    let mark: Element<'a, RemixMessage> = if is_checked {
        icons::icon(Icon::Check, typography::TEXT_11).color(theme().content.on_accent).into()
    } else {
        space().into()
    };
    container(mark)
        .width(Length::Fixed(CHECKBOX_SIZE))
        .height(Length::Fixed(CHECKBOX_SIZE))
        .center_x(Length::Fixed(CHECKBOX_SIZE))
        .center_y(Length::Fixed(CHECKBOX_SIZE))
        .style(move |_theme: &Theme| container::Style {
            background: is_checked.then(|| theme().accent.primary.into()),
            border: rounded(radii::R_5)
                .color(if is_checked { theme().accent.primary } else { theme().content.muted })
                .width(1.5),
            ..Default::default()
        })
        .into()
}

/// Una de cada lista por vuelta, saltando las repetidas (según `key`).
fn interleave_unique<T: Clone, K: Eq + std::hash::Hash>(pools: &[Vec<T>], key: impl Fn(&T) -> K) -> Vec<T> {
    let mut seen = HashSet::new();
    let longest = pools.iter().map(Vec::len).max().unwrap_or(0);
    (0..longest)
        .flat_map(|index| pools.iter().filter_map(move |pool| pool.get(index)))
        .filter(|item| seen.insert(key(item)))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    #[test]
    fn intercala_las_playlists_sin_repetir() {
        let pools = vec![ids(&["a1", "a2", "a3"]), ids(&["b1", "a2"])];
        assert_eq!(interleave_unique(&pools, |id| id.clone()), ids(&["a1", "b1", "a2", "a3"]));
    }
}
