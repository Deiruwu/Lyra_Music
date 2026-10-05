use iced::{border, Alignment, Element, Font, Length, Padding, Task, Theme};
use iced::widget::{button, column, container, mouse_area, opaque, row, rule, text, text_input, Id};
use serde::{Deserialize, Serialize};
use crate::model::{AlbumSearchResult, ArtistProfileDto, SearchItem, Track};
use crate::ui::widgets::async_thumbnail::{async_thumbnail, ThumbnailState};
use crate::ui::widgets::single_line_text::single_line_text;
use crate::ui::widgets::track_row::track_row;
use crate::ui::utils::thumbnail_cache::ThumbnailCache;
use crate::ui::assets::icons::{self, Icon};
use crate::ui::assets::{radii, spacing, typography};
use crate::ui::theme::theme;
use crate::ui::styles::button as button_style;
use crate::ui::styles::text_input as text_input_style;

/// Ancho fijo de la isla flotante (input + toggle + resultados).
const ISLAND_WIDTH: f32 = 640.0;

/// Separación entre el borde superior de la ventana y la isla — la deja
/// justo debajo de la topbar (sidebar toggle + botón de lupa).
const ISLAND_TOP_OFFSET: f32 = 70.0;

/// Lado de la miniatura de cada resultado (igual que `track_row`).
const RESULT_THUMBNAIL_SIZE: f32 = 60.0;
const RESULT_INFO_WIDTH: f32 = 420.0;

const SEARCH_INPUT_ID: &str = "search_island_input";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SearchFilter {
    #[default]
    All,
    Songs,
    Videos,
    Albums,
    Artists,
}

impl SearchFilter {
    /// Orden de los chips en la isla.
    pub const ALL: [SearchFilter; 5] = [Self::All, Self::Songs, Self::Videos, Self::Albums, Self::Artists];

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "Todo",
            Self::Songs => "Canciones",
            Self::Videos => "Videos",
            Self::Albums => "Álbumes",
            Self::Artists => "Artistas",
        }
    }

    /// Valor de `filter` que espera la acción `search_items` del track_manager.
    pub fn as_param(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Songs => "songs",
            Self::Videos => "videos",
            Self::Albums => "albums",
            Self::Artists => "artists",
        }
    }
}

/// Clave de miniatura de un álbum en los resultados de búsqueda.
pub fn album_thumb_key(album_id: &str) -> String {
    format!("search_album:{album_id}")
}

/// Clave de miniatura de un artista en los resultados de búsqueda.
pub fn artist_thumb_key(artist_id: &str) -> String {
    format!("search_artist:{artist_id}")
}

#[derive(Debug, Clone)]
pub enum SearchMessage {
    ToggleOpen,
    /// Ctrl+F: abre la isla (si estaba cerrada) y le da foco al input.
    Open,
    /// Clic fuera de la isla: la oculta sin limpiar texto ni resultados.
    Dismiss,
    /// Cierra la isla vía ESC (`main.rs`) — a diferencia de `ToggleOpen`,
    /// también limpia el texto y los resultados en vez de solo ocultarla.
    Close,
    InputChanged(String),
    Submit,
    TrackClicked(Track),
    TrackRightClicked(Track),
    AlbumClicked(String),
    ArtistClicked(String),
    FilterChanged(SearchFilter),
}

#[derive(Debug, Clone)]
pub enum SearchOutMessage {
    Idle,
    RequestSearch(String, SearchFilter),
    RequestDownloadAndPlay(Track),
    RequestContextMenu(Track),
    RequestOpenAlbum(String),
    RequestOpenArtist(String),
}

#[derive(Default)]
pub struct SearchInput {
    pub input_value: String,
    pub filter: SearchFilter,
    /// La isla está escondida por defecto — el botón de lupa en la topbar
    /// la revela/oculta. No se resetea `input_value`/resultados al
    /// cerrarla: reabrir mantiene la última búsqueda.
    pub is_open: bool,
}


impl SearchInput {
    pub fn update(&mut self, msg: SearchMessage) -> (Task<SearchMessage>, SearchOutMessage) {
        match msg {
            SearchMessage::ToggleOpen => {
                self.is_open = !self.is_open;
                if !self.is_open {
                    return (Task::none(), SearchOutMessage::Idle);
                }
                // Al abrir, el input queda listo para escribir (con lo anterior seleccionado).
                let id = Id::new(SEARCH_INPUT_ID);
                let task = Task::batch([
                    iced::widget::operation::focus(id.clone()),
                    iced::widget::operation::select_all(id),
                ]);
                (task, SearchOutMessage::Idle)
            }

            SearchMessage::Open => {
                self.is_open = true;
                let id = Id::new(SEARCH_INPUT_ID);
                let task = Task::batch([
                    iced::widget::operation::focus(id.clone()),
                    iced::widget::operation::select_all(id),
                ]);
                (task, SearchOutMessage::Idle)
            }

            SearchMessage::Dismiss => {
                self.is_open = false;
                (Task::none(), SearchOutMessage::Idle)
            }

            SearchMessage::Close => {
                self.is_open = false;
                self.input_value.clear();
                (Task::none(), SearchOutMessage::RequestSearch(String::new(), self.filter))
            }

            SearchMessage::InputChanged(value) => {
                self.input_value = value;
                if self.input_value.is_empty() {
                    (Task::none(), SearchOutMessage::RequestSearch(String::new(), self.filter))
                } else {
                    (Task::none(), SearchOutMessage::Idle)
                }
            }

            SearchMessage::FilterChanged(filter) => {
                self.filter = filter;
                if !self.input_value.is_empty() {
                    return (Task::none(), SearchOutMessage::RequestSearch(self.input_value.clone(), filter));
                }
                (Task::none(), SearchOutMessage::Idle)
            }

            SearchMessage::Submit => {
                if self.input_value.is_empty() {
                    (Task::none(), SearchOutMessage::Idle)
                } else {
                    (Task::none(), SearchOutMessage::RequestSearch(self.input_value.clone(), self.filter))
                }
            }

            SearchMessage::TrackClicked(track) => {
                self.input_value.clear();
                (Task::none(), SearchOutMessage::RequestDownloadAndPlay(track))
            }

            SearchMessage::TrackRightClicked(track) => (Task::none(), SearchOutMessage::RequestContextMenu(track)),

            SearchMessage::AlbumClicked(album_id) => {
                self.is_open = false;
                (Task::none(), SearchOutMessage::RequestOpenAlbum(album_id))
            }

            SearchMessage::ArtistClicked(artist_id) => {
                self.is_open = false;
                (Task::none(), SearchOutMessage::RequestOpenArtist(artist_id))
            }
        }
    }

    /// Botón de lupa que vive en la topbar y abre/cierra la isla. Mismo
    /// estilo `minimal` (sin fondo) que `SidebarFeatureV2::view_toggle` —
    /// el espacio para "respirar" lo aporta `main.rs` alrededor, no un
    /// fondo propio.
    pub fn view_toggle(&self) -> Element<'_, SearchMessage> {
        let btn = button(icons::icon(Icon::Search, typography::TEXT_18))
            .style(button_style::minimal)
            .on_press(SearchMessage::ToggleOpen)
            .padding(spacing::SP_8);

        container(btn)
            .width(Length::Fixed(60.0))
            .align_x(Alignment::End)
            .align_y(Alignment::Center)
            .into()
    }

    /// Isla flotante (input + chips de filtro + resultados), superpuesta
    /// sobre el resto de la app — `None` mientras está cerrada, para que
    /// `main.rs` ni siquiera monte la capa de overlay/dismiss.
    pub fn view_overlay<'a>(
        &'a self,
        is_searching: bool,
        results: &'a [SearchItem],
        thumbnails: &'a ThumbnailCache,
    ) -> Option<Element<'a, SearchMessage>> {
        if !self.is_open {
            return None;
        }

        let input = text_input("Buscar canción, álbum, artista...", &self.input_value)
            .on_input(SearchMessage::InputChanged)
            .on_submit(SearchMessage::Submit)
            .id(Id::new(SEARCH_INPUT_ID))
            .padding([spacing::SP_6, spacing::SP_0])
            .style(text_input_style::island)
            .width(Length::Fill);

        let filter_chips = row(SearchFilter::ALL.iter().map(|&filter| {
            button(text(filter.label()).size(typography::TEXT_12).color(theme().content.primary))
                .padding(Padding { top: spacing::SP_4, bottom: spacing::SP_4, left: spacing::SP_12, right: spacing::SP_12 })
                .style(button_style::pill(filter == self.filter))
                .on_press(SearchMessage::FilterChanged(filter))
                .into()
        }))
            .spacing(spacing::SP_6);

        let mut body = column![input, filter_chips].spacing(spacing::SP_12);

        let show_results_area = is_searching || !results.is_empty();

        if show_results_area {
            body = body.push(
                rule::horizontal(1.0).style(|_theme: &Theme| rule::Style {
                    color: theme().border.subtle,
                    radius: radii::R_NONE.into(),
                    fill_mode: rule::FillMode::Full,
                    snap: false,
                }),
            );

            if is_searching {
                body = body.push(text("Buscando...").size(typography::TEXT_14).color(theme().content.muted));
            } else {
                let results_column = column(results.iter().map(|item| match item {
                    SearchItem::Track(track) => mouse_area(track_row(
                        track,
                        thumbnails.peek_for_render(track),
                        SearchMessage::TrackClicked(track.clone()),
                    ))
                        .on_right_press(SearchMessage::TrackRightClicked(track.clone()))
                        .into(),
                    SearchItem::Album(album) => album_result_row(album, thumbnails),
                    SearchItem::Artist(artist) => artist_result_row(artist, thumbnails),
                }))
                    .spacing(spacing::SP_8);
                body = body.push(results_column);
            }
        }

        let island = container(body)
            .width(Length::Fixed(ISLAND_WIDTH))
            .padding(spacing::SP_16)
            .style(|_theme: &Theme| container::Style {
                background: Some(theme().surface.panel.into()),
                border: border::rounded(radii::R_20),
                shadow: theme().elevation.shadow,
                ..Default::default()
            });

        // `opaque` absorbe los clics dentro de la isla; el resto de la
        // ventana es el fondo que la cierra.
        let positioned_island = container(opaque(island))
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::Center)
            .padding(Padding { top: ISLAND_TOP_OFFSET, ..Default::default() });

        Some(mouse_area(positioned_island).on_press(SearchMessage::Dismiss).into())
    }
}

/// Resultado de álbum: portada + nombre + "Tipo · artistas · año". Click abre el álbum.
fn album_result_row<'a>(album: &'a AlbumSearchResult, thumbnails: &ThumbnailCache) -> Element<'a, SearchMessage> {
    let thumbnail = thumbnail_or_placeholder(thumbnails.peek_color(&album_thumb_key(&album.id)), 6.0);

    let artists = album.artists.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ");
    let subtitle = [album.album_type.as_deref().unwrap_or("Álbum"), artists.as_str(), album.year.as_deref().unwrap_or("")]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");

    result_row(thumbnail, &album.name, subtitle, SearchMessage::AlbumClicked(album.id.clone()))
}

/// Resultado de artista: foto circular + nombre. Click abre el artista.
fn artist_result_row<'a>(artist: &'a ArtistProfileDto, thumbnails: &ThumbnailCache) -> Element<'a, SearchMessage> {
    let thumbnail = thumbnail_or_placeholder(thumbnails.peek_color(&artist_thumb_key(&artist.id)), RESULT_THUMBNAIL_SIZE / 2.0);
    result_row(thumbnail, &artist.name, "Artista".to_string(), SearchMessage::ArtistClicked(artist.id.clone()))
}

fn thumbnail_or_placeholder<'a>(handle: Option<iced::widget::image::Handle>, radius: f32) -> Element<'a, SearchMessage> {
    let state = match handle {
        Some(handle) => ThumbnailState::Loaded(handle),
        None => ThumbnailState::Loading,
    };
    async_thumbnail(state, RESULT_THUMBNAIL_SIZE, radius)
}

/// Fila navegable de resultado (miniatura + título + subtítulo), mismo alto que `track_row`.
fn result_row<'a>(thumbnail: Element<'a, SearchMessage>, title: &'a str, subtitle: String, on_press: SearchMessage) -> Element<'a, SearchMessage> {
    let width = Length::Fixed(RESULT_INFO_WIDTH);
    let info = column![
        single_line_text(title, Font::default(), typography::TEXT_14, theme().content.primary, width),
        single_line_text(subtitle, Font::default(), typography::TEXT_11, theme().content.muted, width),
    ]
        .spacing(spacing::SP_2);

    button(row![thumbnail, info].spacing(spacing::SP_10).align_y(Alignment::Center))
        .width(Length::Fill)
        .on_press(on_press)
        .style(button_style::transparent)
        .into()
}
