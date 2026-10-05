//! Panel lateral para armar una playlist: un solo listado que, mientras escribes,
//! muestra tu biblioteca y, al pulsar Enter, resultados de YouTube (volver a
//! escribir regresa a la biblioteca). "Agregar" suma la canción; las de YouTube
//! se descargan primero (las pide el coordinator, que tiene el cliente).

use std::collections::HashSet;

use iced::widget::{button, column, container, row, scrollable, space, text, text_input, Id};
use iced::{Alignment, Element, Length, Padding, Task};

use crate::model::Track;
use crate::ui::assets::fonts::SF_PRO;
use crate::ui::assets::icons::{self, Icon};
use crate::ui::assets::{spacing, typography};
use crate::ui::styles::container as container_style;
use crate::ui::styles::button as button_style;
use crate::ui::styles::scrollable as scrollable_style;
use crate::ui::styles::text_input as text_input_style;
use crate::ui::theme::theme;
use crate::ui::utils::async_thumbnail::{thumb_key, AsyncThumbnail};
use crate::ui::views::catalog_store::CatalogStore;
use crate::ui::views::states_view::filter_tracks;
use crate::ui::widgets::single_line_text::single_line_text;
use crate::ui::widgets::track_row::track_thumbnail_sized;

const INPUT_ID: &str = "playlist_adder_input";
const LOCAL_RESULTS: usize = 30;
const ROW_THUMBNAIL_SIZE: f32 = 40.0;
const ACTION_WIDTH: f32 = 96.0;

enum RemoteResults {
    Idle,
    Searching,
    Ready(Vec<Track>),
    Failed(String),
}

#[derive(Debug, Clone)]
pub enum AdderMessage {
    /// Escribir filtra tu biblioteca (y deja de mostrar lo de YouTube).
    QueryChanged(String),
    /// Enter: busca la consulta en YouTube.
    Submit,
    RemoteLoaded(Result<Vec<Track>, String>),
    AddLocal(String),
    AddRemote(Track),
    Close,
}

pub enum AdderOutMessage {
    Idle,
    /// Buscar en YouTube; el resultado vuelve como `RemoteLoaded`.
    Search(String),
    AddTrack(String),
    DownloadAndAdd(Track),
    Close,
}

pub struct PlaylistAdder {
    query: String,
    /// Resultados de YouTube; `Idle` = se muestra la biblioteca.
    remote: RemoteResults,
    /// Canciones de YouTube que se están descargando para agregarse.
    downloading: HashSet<String>,
}

impl PlaylistAdder {
    pub fn new() -> (Self, Task<AdderMessage>) {
        let adder = Self { query: String::new(), remote: RemoteResults::Idle, downloading: HashSet::new() };
        (adder, iced::widget::operation::focus(Id::new(INPUT_ID)))
    }

    pub fn update(&mut self, message: AdderMessage) -> AdderOutMessage {
        match message {
            AdderMessage::QueryChanged(query) => {
                self.query = query;
                self.remote = RemoteResults::Idle;
                AdderOutMessage::Idle
            }
            AdderMessage::Submit => {
                let query = self.query.trim().to_string();
                if query.is_empty() {
                    return AdderOutMessage::Idle;
                }
                self.remote = RemoteResults::Searching;
                AdderOutMessage::Search(query)
            }
            AdderMessage::RemoteLoaded(result) => {
                self.remote = match result {
                    Ok(tracks) => RemoteResults::Ready(tracks),
                    Err(e) => RemoteResults::Failed(e),
                };
                AdderOutMessage::Idle
            }
            AdderMessage::AddLocal(track_id) => AdderOutMessage::AddTrack(track_id),
            AdderMessage::AddRemote(track) => {
                self.downloading.insert(track.id.clone());
                AdderOutMessage::DownloadAndAdd(track)
            }
            AdderMessage::Close => AdderOutMessage::Close,
        }
    }

    /// Terminó (bien o mal) la descarga de una canción pedida desde el panel.
    pub fn download_finished(&mut self, track_id: &str) {
        self.downloading.remove(track_id);
    }

    /// Miniaturas de lo que muestra el panel.
    pub fn thumbnail_targets(&self, catalog: &CatalogStore) -> Vec<(String, String)> {
        let tracks: Vec<&Track> = match &self.remote {
            RemoteResults::Ready(tracks) => tracks.iter().collect(),
            RemoteResults::Idle => self.local_matches(catalog),
            _ => Vec::new(),
        };
        tracks
            .into_iter()
            .filter_map(|t| t.thumbnail_small.clone().map(|url| (thumb_key(t), url)))
            .collect()
    }

    pub fn view<'a>(
        &'a self,
        playlist_id: &'a str,
        catalog: &'a CatalogStore,
        thumbnails: &'a AsyncThumbnail,
    ) -> Element<'a, AdderMessage> {
        let title = row![
            text("Agregar canciones").font(SF_PRO).size(typography::TEXT_16).color(theme().content.primary),
            space().width(Length::Fill),
            button(text("Cerrar").font(SF_PRO).size(typography::TEXT_12).color(theme().content.secondary))
                .padding(Padding { top: spacing::SP_4, bottom: spacing::SP_4, left: spacing::SP_10, right: spacing::SP_10 })
                .style(button_style::pill(false))
                .on_press(AdderMessage::Close),
        ]
            .align_y(Alignment::Center);

        let input = text_input("Buscar canciones…", &self.query)
            .id(Id::new(INPUT_ID))
            .on_input(AdderMessage::QueryChanged)
            .on_submit(AdderMessage::Submit)
            .font(SF_PRO)
            .size(typography::TEXT_13)
            .padding(Padding { top: spacing::SP_8, bottom: spacing::SP_8, left: spacing::SP_12, right: spacing::SP_12 })
            .style(text_input_style::field);

        let in_playlist = |id: &str| catalog.is_track_in_playlist(playlist_id, id);

        let (source_label, body): (&str, Element<'a, AdderMessage>) = match &self.remote {
            RemoteResults::Idle => {
                let body = if self.query.trim().is_empty() {
                    hint("Escribe para buscar entre tus canciones; Enter busca en YouTube.")
                } else {
                    let matches = self.local_matches(catalog);
                    if matches.is_empty() {
                        hint("Nada en tu biblioteca. Pulsa Enter para buscar en YouTube.")
                    } else {
                        rows(matches.into_iter().map(|track| {
                            let action = if in_playlist(&track.id) { RowAction::Added } else { RowAction::Add(AdderMessage::AddLocal(track.id.clone())) };
                            result_row(track, thumbnails.get(&thumb_key(track)).cloned(), action)
                        }))
                    }
                };
                ("En tu biblioteca · Enter busca en YouTube", body)
            }
            RemoteResults::Searching => ("En YouTube", hint("Buscando…")),
            RemoteResults::Failed(e) => ("En YouTube", hint_owned(format!("No se pudo buscar: {e}"))),
            RemoteResults::Ready(tracks) if tracks.is_empty() => ("En YouTube · escribe para volver a tu biblioteca", hint("Sin resultados.")),
            RemoteResults::Ready(tracks) => (
                "En YouTube · escribe para volver a tu biblioteca",
                rows(tracks.iter().map(|track| {
                    let action = if in_playlist(&track.id) {
                        RowAction::Added
                    } else if self.downloading.contains(&track.id) {
                        RowAction::Downloading
                    } else {
                        RowAction::Add(AdderMessage::AddRemote(track.clone()))
                    };
                    result_row(track, thumbnails.get(&thumb_key(track)).cloned(), action)
                })),
            ),
        };

        container(
            column![
                title,
                input,
                text(source_label).font(SF_PRO).size(typography::TEXT_11).color(theme().content.muted),
                scrollable(body).height(Length::Fill).style(scrollable_style::discreet),
            ]
                .spacing(spacing::SP_12),
        )
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(spacing::SP_16)
            .style(container_style::queue_panel)
            .into()
    }

    fn local_matches<'a>(&self, catalog: &'a CatalogStore) -> Vec<&'a Track> {
        if self.query.trim().is_empty() {
            return Vec::new();
        }
        let library = catalog.explorer_tracks();
        filter_tracks(&library, &self.query).into_iter().take(LOCAL_RESULTS).collect()
    }
}

enum RowAction {
    Add(AdderMessage),
    Added,
    Downloading,
}

fn rows<'a>(items: impl Iterator<Item = Element<'a, AdderMessage>>) -> Element<'a, AdderMessage> {
    column(items).spacing(spacing::SP_4).into()
}

/// Miniatura, título/artistas y la acción a la derecha ("+", "Agregada" o "Descargando…").
fn result_row<'a>(track: &'a Track, thumbnail: Option<iced::widget::image::Handle>, action: RowAction) -> Element<'a, AdderMessage> {
    let info = column![
        single_line_text(track.title.as_str(), SF_PRO, typography::TEXT_13, theme().content.primary, Length::Fill),
        single_line_text(track.format_artists(), SF_PRO, typography::TEXT_11, theme().content.muted, Length::Fill),
    ]
        .spacing(spacing::SP_2)
        .width(Length::Fill);

    let trailing: Element<'a, AdderMessage> = match action {
        RowAction::Add(message) => button(
            row![icons::icon(Icon::Add, typography::TEXT_13), text("Agregar").font(SF_PRO).size(typography::TEXT_12)]
                .spacing(spacing::SP_4)
                .align_y(Alignment::Center),
        )
            .padding(Padding { top: spacing::SP_4, bottom: spacing::SP_4, left: spacing::SP_10, right: spacing::SP_10 })
            .style(button_style::pill(false))
            .on_press(message)
            .into(),
        RowAction::Added => text("Agregada").font(SF_PRO).size(typography::TEXT_12).color(theme().content.muted).into(),
        RowAction::Downloading => text("Descargando…").font(SF_PRO).size(typography::TEXT_12).color(theme().content.muted).into(),
    };

    row![
        track_thumbnail_sized(thumbnail, ROW_THUMBNAIL_SIZE),
        info,
        container(trailing).width(Length::Fixed(ACTION_WIDTH)).align_x(Alignment::End),
    ]
        .spacing(spacing::SP_10)
        .align_y(Alignment::Center)
        .into()
}

fn hint<'a>(message: &'a str) -> Element<'a, AdderMessage> {
    column![space().height(spacing::SP_4), text(message).font(SF_PRO).size(typography::TEXT_12).color(theme().content.muted)].into()
}

fn hint_owned<'a>(message: String) -> Element<'a, AdderMessage> {
    text(message).font(SF_PRO).size(typography::TEXT_12).color(theme().content.muted).into()
}
