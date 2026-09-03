use iced::{border, Alignment, Element, Length, Padding, Task, Theme};
use iced::widget::{button, column, container, row, rule, text, text_input};
use crate::model::Track;
use crate::ui::widgets::icon_toggle::IconToggle;
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

#[derive(Debug, Clone, PartialEq)]
pub enum SearchFilter {
    Songs,
    Videos,
}

#[derive(Debug, Clone)]
pub enum SearchMessage {
    ToggleOpen,
    /// Cierra la isla vía ESC (`main.rs`) — a diferencia de `ToggleOpen`,
    /// también limpia el texto y los resultados en vez de solo ocultarla.
    Close,
    InputChanged(String),
    Submit,
    TrackClicked(Track),
    FilterChanged(SearchFilter),
    Tick,
}

#[derive(Debug, Clone)]
pub enum SearchOutMessage {
    Idle,
    RequestSearch(String, SearchFilter),
    RequestDownloadAndPlay(Track),
}

pub struct SearchInput {
    pub input_value: String,
    pub filter: SearchFilter,
    pub thumb_offset: f32,
    /// La isla está escondida por defecto — el botón de lupa en la topbar
    /// la revela/oculta. No se resetea `input_value`/resultados al
    /// cerrarla: reabrir mantiene la última búsqueda.
    pub is_open: bool,
}

impl Default for SearchInput {
    fn default() -> Self {
        Self {
            input_value: String::new(),
            filter: SearchFilter::Songs,
            thumb_offset: 0.0,
            is_open: false,
        }
    }
}

impl SearchInput {
    pub fn update(&mut self, msg: SearchMessage) -> (Task<SearchMessage>, SearchOutMessage) {
        match msg {
            SearchMessage::ToggleOpen => {
                self.is_open = !self.is_open;
                (Task::none(), SearchOutMessage::Idle)
            }

            SearchMessage::Close => {
                self.is_open = false;
                self.input_value.clear();
                (Task::none(), SearchOutMessage::RequestSearch(String::new(), self.filter.clone()))
            }

            SearchMessage::Tick => {
                let target = if self.filter == SearchFilter::Videos { 32.0 } else { 0.0 };
                let diff = target - self.thumb_offset;

                if diff.abs() > 0.5 {
                    self.thumb_offset += diff * 0.3;
                } else {
                    self.thumb_offset = target;
                }
                (Task::none(), SearchOutMessage::Idle)
            }

            SearchMessage::InputChanged(value) => {
                self.input_value = value;
                if self.input_value.is_empty() {
                    (Task::none(), SearchOutMessage::RequestSearch(String::new(), self.filter.clone()))
                } else {
                    (Task::none(), SearchOutMessage::Idle)
                }
            }

            SearchMessage::FilterChanged(filter) => {
                self.filter = filter.clone();
                if !self.input_value.is_empty() {
                    return (Task::none(), SearchOutMessage::RequestSearch(self.input_value.clone(), filter));
                }
                (Task::none(), SearchOutMessage::Idle)
            }

            SearchMessage::Submit => {
                if self.input_value.is_empty() {
                    (Task::none(), SearchOutMessage::Idle)
                } else {
                    (Task::none(), SearchOutMessage::RequestSearch(self.input_value.clone(), self.filter.clone()))
                }
            }

            SearchMessage::TrackClicked(track) => {
                self.input_value.clear();
                (Task::none(), SearchOutMessage::RequestDownloadAndPlay(track))
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

    /// Isla flotante (input + toggle Songs/Videos + resultados), superpuesta
    /// sobre el resto de la app — `None` mientras está cerrada, para que
    /// `main.rs` ni siquiera monte la capa de overlay/dismiss.
    pub fn view_overlay<'a>(
        &'a self,
        is_searching: bool,
        results: &'a [Track],
        thumbnails: &'a ThumbnailCache,
    ) -> Option<Element<'a, SearchMessage>> {
        if !self.is_open {
            return None;
        }

        let is_videos = self.filter == SearchFilter::Videos;

        let input_row = row![
            text_input("Buscar canción, álbum, artista...", &self.input_value)
                .on_input(SearchMessage::InputChanged)
                .on_submit(SearchMessage::Submit)
                .padding([spacing::SP_6, spacing::SP_0])
                .style(text_input_style::island)
                .width(Length::Fill),

            IconToggle::new(
                is_videos,
                self.thumb_offset,
                |next_state| {
                    SearchMessage::FilterChanged(if next_state {
                        SearchFilter::Videos
                    } else {
                        SearchFilter::Songs
                    })
                }
            ).build(),
        ]
            .spacing(spacing::SP_10)
            .align_y(Alignment::Center);

        let mut body = column![input_row].spacing(spacing::SP_12);

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
                let mut results_column = column![].spacing(spacing::SP_8);
                for track in results {
                    let thumbnail = thumbnails.peek_for_render(track);
                    results_column = results_column.push(
                        track_row(
                            track,
                            thumbnail,
                            SearchMessage::TrackClicked(track.clone()),
                        )
                    );
                }
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

        let positioned_island = container(island)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::Center)
            .padding(Padding { top: ISLAND_TOP_OFFSET, ..Default::default() });

        Some(positioned_island.into())
    }
}
