use std::sync::Arc;

use iced::border::rounded;
use iced::widget::image::Handle;
use iced::widget::scrollable::Viewport;
use iced::widget::{button, column, container, image, responsive, row, rule, scrollable, space, stack, text, Id};
use iced::{Alignment, ContentFit, Element, Length, Padding, Task, Theme};

use crate::db::followed_artist_manager::FollowedArtistManager;
use crate::microservices::client::MicroserviceClient;
use crate::model::{AlbumSummary, AlbumType, ArtistDto, ArtistProfileDto, Track};
use crate::ui::styles::button as button_style;
use crate::ui::assets::fonts::SF_PRO;
use crate::ui::assets::icons::{self, Icon};
use crate::ui::utils::async_thumbnail::{thumb_key, AsyncThumbnail};
use crate::ui::utils::gallery_thumbnail::{GalleryThumbnail, Treatment};
use crate::ui::utils::virtual_list::ScrollTracker;
use crate::ui::views::catalog_store::CatalogStore;
use crate::ui::widgets::selection_state::{stepped_index, DoubleClickDetector, SelectionStep};
use crate::ui::widgets::track_row::truncate;
use crate::ui::widgets::track_row_simple::{track_row_with_thumbnail, THUMBNAIL_ROW_HEIGHT};
use crate::utils::formatting::format_views;
use crate::ui::assets::{radii, spacing, typography};
use crate::ui::theme::theme;
use crate::ui::styles::scrollable as scrollable_style;

const TOP_SONGS_COUNT: usize = 5;
const CARD_THUMBNAIL_SIZE: f32 = 176.0;
const CARD_HOVER_PADDING: f32 = CARD_THUMBNAIL_SIZE * 0.06;
const CARD_HOVER_PADDING_BOTTOM: f32 = CARD_HOVER_PADDING * 1.8;
const CARD_RADIUS: f32 = 8.0;
const CARD_UNIT_WIDTH: f32 = CARD_THUMBNAIL_SIZE + 2.0 * CARD_HOVER_PADDING;
const CARD_SPACING: f32 = 12.0;
const CARD_NAME_MAX_CHARS: usize = 34;
const CARD_NAME_CHARS_PER_LINE: usize = 20;
const CARD_NAME_LINE_HEIGHT: f32 = 18.0;
const CARD_SUBTITLE_LINE_HEIGHT: f32 = 16.0;
const CARD_TEXT_BLOCK_HEIGHT: f32 = 2.0 * CARD_NAME_LINE_HEIGHT + 4.0 + CARD_SUBTITLE_LINE_HEIGHT;
const ARROW_SIZE: f32 = 34.0;
const GALLERY_MAX_SIDE: u32 = 500;
const BANNER_SOURCE_ASPECT_RATIO: f32 = 853.0 / 2048.0;
const BANNER_TOP_CROP_FRACTION: f32 = 0.55;
/// Tope del lado mayor del banner. El banner se pinta a todo el ancho del
/// contenido (`banner_height = size.width * BANNER_ASPECT_RATIO`), así que
/// el tope tiene que cubrir ventanas anchas sin que se vea blando: a 1920
/// de ancho de contenido la imagen cacheada calza 1:1. Por encima de eso
/// degrada suave, que para un banner de fondo es aceptable.
const BANNER_MAX_SIDE: u32 = 1920;
const BANNER_ASPECT_RATIO: f32 = BANNER_SOURCE_ASPECT_RATIO * BANNER_TOP_CROP_FRACTION;
const CONTENT_PADDING_X: f32 = spacing::SP_24;
const SECTION_SPACING: f32 = spacing::SP_28;
const SONGS_HEADER_HEIGHT: f32 = 30.0;
const SONGS_HEADER_SPACING: f32 = spacing::SP_12;
const SONG_ROW_SPACING: f32 = spacing::SP_4;

/// Qué lista de canciones muestra la sección de canciones del artista.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtistSongsMode {
    /// Top de YT Music (lo que trae el fetch del artista).
    Top,
    /// Canciones del artista que están en Me gusta.
    Liked,
    /// Canciones del artista descargadas en el catálogo.
    Downloaded,
}

enum ArtistViewData {
    Loading,
    Loaded(ArtistDto),
    Error(String),
}

pub struct ArtistView {
    artist_id: String,
    data: ArtistViewData,
    is_followed: bool,
    thumbnails: AsyncThumbnail,
    gallery: GalleryThumbnail,
    albums_page: usize,
    singles_page: usize,
    related_page: usize,
    /// Scroll de esta vista — `pub` para que `LibraryBrowserFeature` lo
    /// guarde/restaure al navegar entre artistas/álbumes (ver
    /// `stash_active_route_scroll`).
    pub scroll: ScrollTracker,
    icon_hovered: bool,
    songs_mode: ArtistSongsMode,
    selected_id: Option<String>,
    clicks: DoubleClickDetector,
}

#[derive(Debug, Clone)]
pub enum ArtistMessage {
    Loaded(Result<ArtistDto, String>),
    FollowStatusLoaded(bool),
    ThumbnailLoaded(String, Vec<u8>),
    GalleryLoaded(String, Vec<u8>),
    AlbumsPrevPage,
    AlbumsNextPage,
    SinglesPrevPage,
    SinglesNextPage,
    RelatedPrevPage,
    RelatedNextPage,
    AlbumCardPressed(String),
    RelatedArtistPressed(String),
    /// Click en una fila: selecciona; doble click reproduce.
    SongClicked(String),
    SongRightClicked(String),
    SongArtistPressed(String),
    SongAlbumPressed(String),
    SongTogglePlayback,
    SongIconHover(bool),
    /// Botones "Me gusta"/"Descargadas": vuelve a `Top` si ya estaba activo.
    SongsModeToggled(ArtistSongsMode),
    FollowPressed,
    Scrolled(Viewport),
}

#[derive(Debug, Clone)]
pub enum ArtistOutMessage {
    Idle,
    OpenAlbum(String),
    PlaySong(String),
    TrackRightClicked(String),
    OpenTrackArtist(String),
    RequestTogglePlayback,
    ToggleFollow(String, String, Option<String>),
}

impl ArtistView {
    pub fn new(
        client: MicroserviceClient,
        artist_id: String,
        followed_artist_manager: Arc<FollowedArtistManager>,
    ) -> (Self, Task<ArtistMessage>) {
        let view = Self {
            artist_id: artist_id.clone(),
            data: ArtistViewData::Loading,
            is_followed: false,
            thumbnails: AsyncThumbnail::new(128),
            gallery: GalleryThumbnail::new(),
            albums_page: 0,
            singles_page: 0,
            related_page: 0,
            scroll: ScrollTracker::default(),
            icon_hovered: false,
            songs_mode: ArtistSongsMode::Top,
            selected_id: None,
            clicks: DoubleClickDetector::default(),
        };

        let load_task = Task::perform(
            async move { client.artist(&artist_id, Some(TOP_SONGS_COUNT)).await.map_err(|e| e.to_string()) },
            ArtistMessage::Loaded,
        );

        let follow_status_task = Task::perform(
            {
                let artist_id = view.artist_id.clone();
                async move { followed_artist_manager.is_followed(&artist_id).await.unwrap_or(false) }
            },
            ArtistMessage::FollowStatusLoaded,
        );

        (view, Task::batch([load_task, follow_status_task]))
    }

    pub fn update(&mut self, message: ArtistMessage, catalog: &CatalogStore) -> (Task<ArtistMessage>, ArtistOutMessage) {
        let mut out = ArtistOutMessage::Idle;

        match message {
            ArtistMessage::Loaded(Ok(dto)) => self.data = ArtistViewData::Loaded(dto),
            ArtistMessage::Loaded(Err(error)) => self.data = ArtistViewData::Error(error),
            ArtistMessage::FollowStatusLoaded(is_followed) => self.is_followed = is_followed,
            ArtistMessage::ThumbnailLoaded(key, bytes) => self.thumbnails.on_loaded(key, bytes),
            ArtistMessage::GalleryLoaded(key, bytes) => self.gallery.on_loaded(key, bytes),
            ArtistMessage::AlbumsPrevPage => self.albums_page = self.albums_page.saturating_sub(1),
            ArtistMessage::AlbumsNextPage => self.albums_page += 1,
            ArtistMessage::SinglesPrevPage => self.singles_page = self.singles_page.saturating_sub(1),
            ArtistMessage::SinglesNextPage => self.singles_page += 1,
            ArtistMessage::RelatedPrevPage => self.related_page = self.related_page.saturating_sub(1),
            ArtistMessage::RelatedNextPage => self.related_page += 1,
            ArtistMessage::AlbumCardPressed(id) => out = ArtistOutMessage::OpenAlbum(id),
            ArtistMessage::RelatedArtistPressed(id) => out = ArtistOutMessage::OpenTrackArtist(id),
            ArtistMessage::SongClicked(id) => {
                if self.clicks.register(&id) {
                    out = ArtistOutMessage::PlaySong(id);
                } else {
                    self.selected_id = Some(id);
                }
            }
            ArtistMessage::SongRightClicked(id) => {
                self.selected_id = Some(id.clone());
                out = ArtistOutMessage::TrackRightClicked(id);
            }
            ArtistMessage::SongArtistPressed(id) => out = ArtistOutMessage::OpenTrackArtist(id),
            ArtistMessage::SongAlbumPressed(id) => out = ArtistOutMessage::OpenAlbum(id),
            ArtistMessage::SongTogglePlayback => out = ArtistOutMessage::RequestTogglePlayback,
            ArtistMessage::SongIconHover(hovered) => self.icon_hovered = hovered,
            ArtistMessage::SongsModeToggled(mode) => {
                self.songs_mode = if self.songs_mode == mode { ArtistSongsMode::Top } else { mode };
                self.selected_id = None;
            }
            ArtistMessage::FollowPressed => {
                self.is_followed = !self.is_followed;
                if let ArtistViewData::Loaded(artist) = &self.data {
                    out = ArtistOutMessage::ToggleFollow(artist.id.clone(), artist.name.clone(), artist.banner.clone());
                }
            }
            ArtistMessage::Scrolled(viewport) => self.scroll.update(viewport),
        }

        let sync_task = self.thumbnails.sync(&self.thumbnail_targets(catalog), ArtistMessage::ThumbnailLoaded);
        let gallery_task = self.gallery.sync(&self.gallery_targets(), ArtistMessage::GalleryLoaded);

        (Task::batch([sync_task, gallery_task]), out)
    }

    pub fn view<'a>(&'a self, now_playing_id: Option<String>, is_playing: bool, catalog: &'a CatalogStore) -> Element<'a, ArtistMessage> {
        match &self.data {
            ArtistViewData::Loading => status_message("Cargando artista…"),
            ArtistViewData::Error(error) => status_message(error),
            ArtistViewData::Loaded(artist) => {
                let (albums, singles_and_eps) = partition_albums(artist);

                let mut children: Vec<Element<'a, ArtistMessage>> =
                    vec![self.view_header(artist), self.view_songs(catalog, now_playing_id, is_playing)];

                if !albums.is_empty() {
                    children.push(self.view_album_section(
                        "Álbumes",
                        albums,
                        self.albums_page,
                        ArtistMessage::AlbumsPrevPage,
                        ArtistMessage::AlbumsNextPage,
                    ));
                }

                if !singles_and_eps.is_empty() {
                    children.push(self.view_album_section(
                        "Singles y EPs",
                        singles_and_eps,
                        self.singles_page,
                        ArtistMessage::SinglesPrevPage,
                        ArtistMessage::SinglesNextPage,
                    ));
                }

                if !artist.related.is_empty() {
                    children.push(self.view_carousel_section(
                        "A los fans también les gusta",
                        artist.related.iter().collect(),
                        self.related_page,
                        ArtistMessage::RelatedPrevPage,
                        ArtistMessage::RelatedNextPage,
                        |related| self.view_related_artist_card(related),
                    ));
                }

                scrollable(
                    column(children)
                        .spacing(SECTION_SPACING)
                        .padding(Padding { top: spacing::SP_0, right: CONTENT_PADDING_X, bottom: spacing::SP_32, left: CONTENT_PADDING_X }),
                )
                .width(Length::Fill)
                .style(scrollable_style::discreet)
                .id(Id::new("artist_view_scroll"))
                .on_scroll(ArtistMessage::Scrolled)
                .into()
            }
        }
    }

    /// Banner de fondo (relación de aspecto fija 2048×853) con el nombre del artista superpuesto.
    fn view_header<'a>(&'a self, artist: &'a ArtistDto) -> Element<'a, ArtistMessage> {
        responsive(move |size| {
            let banner_height = size.width * BANNER_ASPECT_RATIO;

            let background: Element<'a, ArtistMessage> = match self.gallery.get(&banner_key(&artist.id)) {
                Some(handle) => banner_image(handle.clone(), banner_height),
                None => banner_placeholder(banner_height),
            };

            let name = text(artist.name.as_str()).font(SF_PRO).size(typography::TEXT_44).color(theme().content.primary);

            let views: Element<'a, ArtistMessage> = match artist.views {
                Some(views) => text(format!("{} reproducciones", format_views(views)))
                    .font(SF_PRO)
                    .size(typography::TEXT_14)
                    .color(theme().content.on_banner)
                    .into(),
                None => space().into(),
            };

            let overlay = row![name, space().width(Length::Fill), views, space().width(12), follow_button(self.is_followed)]
                .align_y(Alignment::End)
                .width(Length::Fill);

            let name = container(overlay)
                .width(Length::Fill)
                .height(Length::Fixed(banner_height))
                .align_y(Alignment::End)
                .padding(spacing::SP_24);

            container(stack![background, name])
                .width(Length::Fill)
                .height(Length::Fixed(banner_height))
                .clip(true)
                .into()
        })
        .into()
    }

    /// Sección de canciones: título + botones Me gusta/Descargadas y la lista
    /// del modo activo (fila simple con número + thumbnail).
    fn view_songs<'a>(&'a self, catalog: &'a CatalogStore, now_playing_id: Option<String>, is_playing: bool) -> Element<'a, ArtistMessage> {
        let mode = self.effective_mode(catalog);
        let liked_count = self.songs_for(ArtistSongsMode::Liked, catalog).len();
        let downloaded_count = self.songs_for(ArtistSongsMode::Downloaded, catalog).len();

        let title = match mode {
            ArtistSongsMode::Top => "Top canciones",
            ArtistSongsMode::Liked => "Canciones que te gustan",
            ArtistSongsMode::Downloaded => "Canciones descargadas",
        };

        let mut header = row![section_title(title), space().width(Length::Fill)]
            .spacing(spacing::SP_8)
            .align_y(Alignment::Center)
            .height(Length::Fixed(SONGS_HEADER_HEIGHT));

        if liked_count > 0 {
            header = header.push(pill_button(
                format!("Me gusta · {liked_count}"),
                mode == ArtistSongsMode::Liked,
                ArtistMessage::SongsModeToggled(ArtistSongsMode::Liked),
            ));
        }
        if downloaded_count > 0 {
            header = header.push(pill_button(
                format!("Descargadas · {downloaded_count}"),
                mode == ArtistSongsMode::Downloaded,
                ArtistMessage::SongsModeToggled(ArtistSongsMode::Downloaded),
            ));
        }

        let rows: Vec<Element<'a, ArtistMessage>> = self
            .songs_for(mode, catalog)
            .into_iter()
            .enumerate()
            .map(|(index, track)| {
                let thumbnail = self.thumbnails.get(&thumb_key(track)).cloned();
                let is_playing_row = now_playing_id.as_deref() == Some(track.id.as_str());
                let is_selected = self.selected_id.as_deref() == Some(track.id.as_str());
                track_row_with_thumbnail(
                    index + 1,
                    track,
                    thumbnail,
                    is_selected,
                    ArtistMessage::SongClicked(track.id.clone()),
                    ArtistMessage::SongRightClicked(track.id.clone()),
                    ArtistMessage::SongArtistPressed,
                    ArtistMessage::SongAlbumPressed,
                    is_playing_row,
                    is_playing,
                    self.icon_hovered,
                    ArtistMessage::SongTogglePlayback,
                    ArtistMessage::SongIconHover(true),
                    ArtistMessage::SongIconHover(false),
                )
            })
            .collect();

        column![header, column(rows).spacing(SONG_ROW_SPACING)]
            .spacing(SONGS_HEADER_SPACING)
            .into()
    }

    /// Carrusel paginado de álbumes/singles/EPs. Nunca se llama con `items` vacío.
    fn view_album_section<'a>(
        &'a self,
        title: &'static str,
        items: Vec<&'a AlbumSummary>,
        page: usize,
        on_prev: ArtistMessage,
        on_next: ArtistMessage,
    ) -> Element<'a, ArtistMessage> {
        self.view_carousel_section(title, items, page, on_prev, on_next, |item| self.view_album_card(item))
    }

    /// Título + separador + flechas, y una fila paginada de tarjetas de `CARD_UNIT_WIDTH`.
    fn view_carousel_section<'a, T: 'a>(
        &'a self,
        title: &'static str,
        items: Vec<&'a T>,
        page: usize,
        on_prev: ArtistMessage,
        on_next: ArtistMessage,
        card: impl Fn(&'a T) -> Element<'a, ArtistMessage> + 'a,
    ) -> Element<'a, ArtistMessage> {
        responsive(move |size| {
            let per_page = albums_per_page(size.width);
            let total_pages = items.len().div_ceil(per_page);
            let page = page.min(total_pages.saturating_sub(1));

            let divider = rule::horizontal(1.0).style(|_theme: &Theme| rule::Style {
                color: theme().border.subtle,
                radius: radii::R_NONE.into(),
                fill_mode: rule::FillMode::Full,
                snap: false,
            });

            let header = row![
                section_title(title),
                divider,
                Self::carousel_arrow(Icon::LeftArrow, (page > 0).then_some(on_prev.clone())),
                Self::carousel_arrow(Icon::RightArrow, (page + 1 < total_pages).then_some(on_next.clone())),
            ]
            .spacing(spacing::SP_16)
            .align_y(Alignment::Center);

            let start = page * per_page;
            let end = ((page + 1) * per_page).min(items.len());
            let visible_count = end - start;

            let leftover = (size.width - visible_count as f32 * CARD_UNIT_WIDTH).max(0.0);
            let gap = if visible_count > 1 && leftover < CARD_UNIT_WIDTH {
                leftover / (visible_count - 1) as f32
            } else {
                CARD_SPACING
            };

            let cards: Vec<Element<'a, ArtistMessage>> = items[start..end]
                .iter()
                .map(|item| card(item))
                .collect();

            column![header, row(cards).spacing(gap)].spacing(spacing::SP_12).into()
        })
        .into()
    }

    /// Tarjeta individual: thumbnail grande arriba, nombre a 2 líneas fijas, "Año · Tipo" debajo. Clic → abre el álbum.
    fn view_album_card<'a>(&'a self, item: &'a AlbumSummary) -> Element<'a, ArtistMessage> {
        let thumbnail: Element<'a, ArtistMessage> = match self.gallery.get(&item.id) {
            Some(handle) => image(handle.clone())
                .width(Length::Fixed(CARD_THUMBNAIL_SIZE))
                .height(Length::Fixed(CARD_THUMBNAIL_SIZE))
                .content_fit(ContentFit::Cover)
                .border_radius(CARD_RADIUS)
                .into(),
            None => container(space())
                .width(Length::Fixed(CARD_THUMBNAIL_SIZE))
                .height(Length::Fixed(CARD_THUMBNAIL_SIZE))
                .style(|_theme: &Theme| container::Style {
                    background: Some(theme().surface.sunken.into()),
                    border: rounded(CARD_RADIUS),
                    ..Default::default()
                })
                .into(),
        };

        let display_name = truncate(item.name.as_str(), CARD_NAME_MAX_CHARS);
        let name_lines = if display_name.chars().count() > CARD_NAME_CHARS_PER_LINE { 2.0 } else { 1.0 };

        let name = container(text(display_name).font(SF_PRO).size(typography::TEXT_14).color(theme().content.primary))
            .width(Length::Fixed(CARD_THUMBNAIL_SIZE))
            .height(Length::Fixed(name_lines * CARD_NAME_LINE_HEIGHT))
            .clip(true);

        let subtitle = text(format!(
            "{} · {}",
            item.year.as_deref().unwrap_or("—"),
            item.album_type.label(),
        ))
        .font(SF_PRO)
        .size(typography::TEXT_12)
        .color(theme().content.muted)
        .width(Length::Fixed(CARD_THUMBNAIL_SIZE))
        .height(Length::Fixed(CARD_SUBTITLE_LINE_HEIGHT));

        let text_block = column![name, subtitle]
            .spacing(spacing::SP_4)
            .width(Length::Fixed(CARD_THUMBNAIL_SIZE))
            .height(Length::Fixed(CARD_TEXT_BLOCK_HEIGHT));
        let content = column![thumbnail, text_block].spacing(spacing::SP_6);

        button(content)
            .padding(Padding {
                top: CARD_HOVER_PADDING,
                right: CARD_HOVER_PADDING,
                bottom: CARD_HOVER_PADDING_BOTTOM,
                left: CARD_HOVER_PADDING,
            })
            .style(button_style::card_hover(CARD_RADIUS))
            .on_press(ArtistMessage::AlbumCardPressed(item.id.clone()))
            .into()
    }

    /// Tarjeta circular de artista relacionado: foto y nombre centrado. Clic → abre el artista.
    fn view_related_artist_card<'a>(&'a self, artist: &'a ArtistProfileDto) -> Element<'a, ArtistMessage> {
        let photo: Element<'a, ArtistMessage> = match self.gallery.get(&related_key(&artist.id)) {
            Some(handle) => image(handle.clone())
                .width(Length::Fixed(CARD_THUMBNAIL_SIZE))
                .height(Length::Fixed(CARD_THUMBNAIL_SIZE))
                .content_fit(ContentFit::Cover)
                .border_radius(CARD_THUMBNAIL_SIZE / 2.0)
                .into(),
            None => container(space())
                .width(Length::Fixed(CARD_THUMBNAIL_SIZE))
                .height(Length::Fixed(CARD_THUMBNAIL_SIZE))
                .style(|_theme: &Theme| container::Style {
                    background: Some(theme().surface.sunken.into()),
                    border: rounded(CARD_THUMBNAIL_SIZE / 2.0),
                    ..Default::default()
                })
                .into(),
        };

        let name = text(truncate(artist.name.as_str(), CARD_NAME_CHARS_PER_LINE))
            .font(SF_PRO)
            .size(typography::TEXT_14)
            .color(theme().content.primary)
            .width(Length::Fixed(CARD_THUMBNAIL_SIZE))
            .height(Length::Fixed(CARD_NAME_LINE_HEIGHT))
            .align_x(Alignment::Center);

        button(column![photo, name].spacing(spacing::SP_8).align_x(Alignment::Center))
            .padding(Padding {
                top: CARD_HOVER_PADDING,
                right: CARD_HOVER_PADDING,
                bottom: CARD_HOVER_PADDING_BOTTOM,
                left: CARD_HOVER_PADDING,
            })
            .style(button_style::card_hover(CARD_RADIUS))
            .on_press(ArtistMessage::RelatedArtistPressed(artist.id.clone()))
            .into()
    }

    /// Botón circular con flecha centrada; sin `on_press` cuando `on_press` es `None`.
    fn carousel_arrow(icon: Icon, on_press: Option<ArtistMessage>) -> Element<'static, ArtistMessage> {
        let glyph = container(
            icons::icon(icon, typography::TEXT_14),
        )
        .width(Length::Fixed(ARROW_SIZE))
        .height(Length::Fixed(ARROW_SIZE))
        .align_x(Alignment::Center)
        .align_y(Alignment::Center);

        let enabled = on_press.is_some();

        let mut arrow_button = button(glyph)
            .padding(spacing::SP_0)
            .style(button_style::carousel_arrow(enabled, ARROW_SIZE));

        if let Some(message) = on_press {
            arrow_button = arrow_button.on_press(message);
        }

        arrow_button.into()
    }

    /// Id del artista mostrado — para cachear/restaurar el scroll por id
    /// (ver `LibraryBrowserFeature::stash_active_route_scroll`).
    pub(crate) fn artist_id(&self) -> &str {
        &self.artist_id
    }

    /// Modo pedido, o `Top` si su lista quedó vacía (p. ej. se quitó el último like).
    fn effective_mode(&self, catalog: &CatalogStore) -> ArtistSongsMode {
        match self.songs_mode {
            ArtistSongsMode::Top => ArtistSongsMode::Top,
            mode if self.songs_for(mode, catalog).is_empty() => ArtistSongsMode::Top,
            mode => mode,
        }
    }

    /// Canciones de un modo: el top del fetch, o las del catálogo local de este artista.
    fn songs_for<'a>(&'a self, mode: ArtistSongsMode, catalog: &'a CatalogStore) -> Vec<&'a Track> {
        let ArtistViewData::Loaded(artist) = &self.data else { return Vec::new() };

        let by_artist = |track: &&Track| track.artists.iter().any(|a| a.id.as_deref() == Some(artist.id.as_str()));

        match mode {
            ArtistSongsMode::Top => artist.songs.iter().take(TOP_SONGS_COUNT).collect(),
            ArtistSongsMode::Liked => catalog
                .tracks_for_playlist(catalog.system_playlist_id())
                .into_iter()
                .filter(by_artist)
                .collect(),
            ArtistSongsMode::Downloaded => catalog
                .all_tracks()
                .iter()
                .filter(|t| t.file_path.is_some())
                .filter(by_artist)
                .collect(),
        }
    }

    /// Canciones que se ven ahora mismo, para armar `play_context`.
    pub(crate) fn shown_songs(&self, catalog: &CatalogStore) -> Vec<Track> {
        self.songs_for(self.effective_mode(catalog), catalog).into_iter().cloned().collect()
    }

    /// Busca una canción de la lista visible por id (para armar el menú contextual).
    pub(crate) fn find_song<'a>(&'a self, id: &str, catalog: &'a CatalogStore) -> Option<&'a Track> {
        self.songs_for(self.effective_mode(catalog), catalog).into_iter().find(|t| t.id == id)
    }

    /// Mueve la selección (filas o páginas) y scrollea para mantenerla a la vista.
    pub(crate) fn move_selection(&mut self, step: SelectionStep, catalog: &CatalogStore) -> Task<ArtistMessage> {
        let songs = self.songs_for(self.effective_mode(catalog), catalog);
        if songs.is_empty() {
            return Task::none();
        }

        let row_pitch = THUMBNAIL_ROW_HEIGHT + SONG_ROW_SPACING;
        let current = self.selected_id.as_deref().and_then(|id| songs.iter().position(|t| t.id == id));
        let index = stepped_index(current, step.rows(self.scroll.rows_per_page(row_pitch)), songs.len());
        let song_count = songs.len();
        self.selected_id = Some(songs[index].id.clone());

        let banner_height = (self.scroll.viewport_width - 2.0 * CONTENT_PADDING_X).max(0.0) * BANNER_ASPECT_RATIO;
        let list_top = banner_height + SECTION_SPACING + SONGS_HEADER_HEIGHT + SONGS_HEADER_SPACING;
        let row_top = list_top + index as f32 * row_pitch;

        if step.is_page() {
            let moved_rows = index as f32 - current.unwrap_or(index) as f32;
            let content_height = list_top + song_count as f32 * row_pitch + self.scroll.viewport_height;
            self.scroll.page_and_reveal(moved_rows * row_pitch, content_height, row_top, THUMBNAIL_ROW_HEIGHT, "artist_view_scroll")
        } else {
            self.scroll.reveal(row_top, THUMBNAIL_ROW_HEIGHT, "artist_view_scroll")
        }
    }

    /// Id de la canción seleccionada, si sigue en la lista visible.
    pub(crate) fn selected_song_id(&self, catalog: &CatalogStore) -> Option<&str> {
        let id = self.selected_id.as_deref()?;
        self.find_song(id, catalog).map(|_| id)
    }

    /// Reemplaza en el lugar el track cuyo id matchea, en el top de
    /// canciones (mismo `Track` recién descargado/analizado en otra parte
    /// de la app, vía `LibraryBrowserFeature::patch_track`) — sin esto, esta
    /// vista sigue mostrando/reproduciendo el stub congelado que trajo el
    /// fetch inicial del artista, aunque `CatalogStore` ya tenga el dato
    /// fresco.
    pub(crate) fn patch_track(&mut self, track: &Track) {
        if let ArtistViewData::Loaded(artist) = &mut self.data
            && let Some(existing) = artist.songs.iter_mut().find(|t| t.id == track.id) {
                *existing = track.clone();
            }
    }

    /// URLs de thumbnails cuadrados a mantener vivos: canciones de la lista visible.
    fn thumbnail_targets(&self, catalog: &CatalogStore) -> Vec<(String, String)> {
        self.songs_for(self.effective_mode(catalog), catalog)
            .into_iter()
            .filter_map(|track| track.thumbnail_small.clone().map(|url| (thumb_key(track), url)))
            .collect()
    }

    /// Banner (recortado a su mitad superior), portadas de la discografía y fotos de relacionados (con tope de tamaño).
    fn gallery_targets(&self) -> Vec<(String, String, Treatment)> {
        let ArtistViewData::Loaded(artist) = &self.data else { return Vec::new() };

        let mut targets = Vec::new();

        if let Some(url) = &artist.banner {
            targets.push((banner_key(&artist.id), url.clone(), Treatment::TopCrop(BANNER_TOP_CROP_FRACTION, BANNER_MAX_SIDE)));
        }

        for item in &artist.albums {
            if let Some(url) = &item.thumbnail_large {
                targets.push((item.id.clone(), url.clone(), Treatment::MaxSide(GALLERY_MAX_SIDE)));
            }
        }

        for related in &artist.related {
            if let Some(url) = related.thumbnail_large.as_ref().or(related.thumbnail_small.as_ref()) {
                targets.push((related_key(&related.id), url.clone(), Treatment::MaxSide(GALLERY_MAX_SIDE)));
            }
        }

        targets
    }
}

/// Botón chico "Seguir"/"Siguiendo" junto al nombre del artista en el banner.
fn follow_button(is_followed: bool) -> Element<'static, ArtistMessage> {
    let label = if is_followed { "Siguiendo" } else { "Seguir" };
    pill_button(label.to_string(), is_followed, ArtistMessage::FollowPressed)
}

/// Píldora de texto con estado activo/inactivo (seguir, filtros de canciones).
fn pill_button(label: String, is_active: bool, on_press: ArtistMessage) -> Element<'static, ArtistMessage> {
    button(text(label).font(SF_PRO).size(typography::TEXT_13).color(theme().content.primary))
        .padding(Padding { top: spacing::SP_6, right: spacing::SP_14, bottom: spacing::SP_6, left: spacing::SP_14 })
        .style(button_style::pill(is_active))
        .on_press(on_press)
        .into()
}

/// Cuántas tarjetas de `CARD_UNIT_WIDTH` (+ `CARD_SPACING` entre ellas) caben en `width`.
fn albums_per_page(width: f32) -> usize {
    let unit = CARD_UNIT_WIDTH + CARD_SPACING;
    (((width + CARD_SPACING) / unit).floor() as usize).max(1)
}

fn related_key(artist_id: &str) -> String {
    format!("related_artist:{artist_id}")
}

fn banner_key(artist_id: &str) -> String {
    format!("artist_banner:{artist_id}")
}

/// Separa la discografía del artista en (álbumes, singles + EPs).
fn partition_albums(artist: &ArtistDto) -> (Vec<&AlbumSummary>, Vec<&AlbumSummary>) {
    artist.albums.iter().partition(|item| item.album_type == AlbumType::Album)
}

fn banner_image<'a, Message: 'a>(handle: Handle, height: f32) -> Element<'a, Message> {
    container(
        image(handle)
            .width(Length::Fill)
            .height(Length::Fill)
            .content_fit(ContentFit::Cover)
            .opacity(0.8),
    )
    .width(Length::Fill)
    .height(Length::Fixed(height))
    .clip(true)
    .style(|_theme: &Theme| container::Style {
        background: Some(theme().content.on_accent.into()),
        ..Default::default()
    })
    .into()
}

fn banner_placeholder<'a, Message: 'a>(height: f32) -> Element<'a, Message> {
    container(space())
        .width(Length::Fill)
        .height(Length::Fixed(height))
        .style(|_theme: &Theme| container::Style {
            background: Some(theme().surface.sunken.into()),
            ..Default::default()
        })
        .into()
}

fn status_message(message: &str) -> Element<'_, ArtistMessage> {
    container(text(message.to_string()).font(SF_PRO).size(typography::TEXT_14).color(theme().content.muted))
        .width(Length::Fill)
        .height(Length::Fixed(200.0))
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .into()
}

fn section_title<'a, Message: 'a>(title: &'a str) -> Element<'a, Message> {
    text(title).font(SF_PRO).size(typography::TEXT_18).color(theme().content.primary).into()
}
