use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::Arc;

use iced::border::rounded;
use iced::widget::{button, column, container, image, mouse_area, pin, responsive, row, rule, scrollable, space, text, text_input, Id};
use iced::{Alignment, ContentFit, Element, Length, Padding, Point, Task, Theme};

use crate::db::artist_tag_manager::ArtistTagManager;
use crate::model::{ArtistTag, FollowedArtist};
use crate::ui::assets::fonts::SF_PRO;
use crate::ui::assets::icons::Icon;
use crate::ui::assets::{radii, spacing, typography};
use crate::ui::styles::button as button_style;
use crate::ui::styles::scrollable as scrollable_style;
use crate::ui::styles::text_input as text_input_style;
use crate::ui::theme::theme;
use crate::ui::utils::async_thumbnail::AsyncThumbnail;
use crate::ui::utils::image::square_image_url;
use crate::ui::views::catalog_store::CatalogStore;
use crate::ui::views::view_data::{NavId, ViewData};
use crate::ui::widgets::confirm_dialog::ConfirmDialog;
use crate::ui::widgets::context_menu::{ContextMenu, ContextMenuEvent, ContextMenuItem};
use crate::ui::widgets::drag_pill::drag_pill;
use crate::ui::widgets::track_row::truncate;

pub const VIEW_DATA: ViewData = ViewData::new(NavId::Artists, Icon::Artists, "Artistas");

const PHOTO_SIZE: f32 = 132.0;
/// Lado de la foto cuadrada que se pide al CDN.
const PHOTO_REQUEST_SIDE: u32 = 400;
const CARD_HOVER_PADDING: f32 = 8.0;
const CARD_NAME_MAX_CHARS: usize = 18;
const CARD_NAME_LINE_HEIGHT: f32 = 18.0;
const CARD_SPACING: f32 = 12.0;
const HEADER_HEIGHT: f32 = 44.0;
const SECTION_TITLE_HEIGHT: f32 = 30.0;
const NEW_TAG_INPUT_WIDTH: f32 = 260.0;
const NEW_TAG_INPUT_ID: &str = "artists_new_tag_input";
const RENAME_INPUT_ID: &str = "artists_rename_input";
/// Lado de la foto en la tarjeta que sigue al cursor al arrastrar.
const GHOST_PHOTO_SIZE: f32 = 84.0;
/// Movimiento mínimo antes de que apretar sobre una tarjeta cuente como arrastre.
const DRAG_THRESHOLD_PX: f32 = 5.0;
/// Opacidad de la tarjeta original mientras se arrastra su fantasma.
const DRAGGED_CARD_OPACITY: f32 = 0.35;
/// Cuánto hay que moverse hacia una sección vecina para que la etiqueta arrastrada cambie de lugar.
const TAG_SWAP_HYSTERESIS_PX: f32 = 16.0;
const ARROW_SIZE: f32 = 30.0;
const ADD_TO_TAG_SUBMENU_ID: usize = 0;
const MOVE_TO_TAG_SUBMENU_ID: usize = 1;

/// Etiquetas en orden y pares `(tag_id, artist_id)`, tal como salen de SQLite.
type TagSnapshot = (Vec<ArtistTag>, Vec<(String, String)>);

/// Sobre qué se abrió el menú contextual.
#[derive(Debug, Clone, PartialEq)]
pub enum ArtistMenuTarget {
    /// Tarjeta de artista; `tag_id` es la sección donde se hizo clic (`None` = "Artistas seguidos").
    Artist { artist_id: String, tag_id: Option<String> },
    Tag(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ArtistMenuAction {
    AddToTag(String),
    /// Saca al artista de la sección donde se hizo clic y lo mete en esta etiqueta.
    MoveToTag(String),
    RemoveFromTag(String),
    NewTagWithArtist,
    RenameTag,
    MoveTagUp,
    MoveTagDown,
    DeleteTag,
}

#[derive(Debug, Clone)]
pub enum ArtistsMessage {
    Loaded(Result<TagSnapshot, String>),
    /// Resultado de escribir un cambio en SQLite; si falla se recarga todo.
    Persisted(Result<(), String>),
    ThumbnailLoaded(String, Vec<u8>),
    ArtistClicked(String),
    ArtistRightClicked { artist_id: String, tag_id: Option<String> },
    TagRightClicked(String),
    MenuEvent(ContextMenuEvent<ArtistMenuTarget>),
    MenuAction(ArtistMenuAction, ArtistMenuTarget),
    ShowNewTagInput,
    NewTagNameChanged(String),
    SubmitNewTag,
    StartRename(String),
    RenameChanged(String),
    SubmitRename,
    ConfirmDeleteTag,
    CancelDeleteTag,

    // ─── Arrastrar artistas entre etiquetas ─────────────────────
    CardHovered { artist_id: String, tag_id: Option<String> },
    CardUnhovered { artist_id: String, tag_id: Option<String> },
    /// Sección bajo el cursor (`None` = "Artistas seguidos").
    SectionHovered(Option<String>),
    SectionUnhovered(Option<String>),
    GlobalPressed,
    GlobalReleased,

    // ─── Contraer / carrusel ────────────────────────────────────
    ToggleSection(Option<String>),
    SectionPrevPage(Option<String>),
    SectionNextPage(Option<String>),
    /// Cursor sobre el título de una etiqueta (desde ahí se arrastra para reordenar).
    TagTitleHovered(String),
    TagTitleUnhovered(String),
}

pub enum ArtistsOutMessage {
    Idle,
    OpenArtist(String),
}

/// Artista arrastrado desde la sección `from` (`None` = "Artistas seguidos").
struct ArtistDrag {
    artist_id: String,
    from: Option<String>,
    start: Point,
    /// Pasa a `true` cuando el cursor supera `DRAG_THRESHOLD_PX`.
    active: bool,
    /// Posición donde caería dentro de su propia etiqueta (vista previa del reordenamiento).
    target: Option<usize>,
}

/// Etiqueta arrastrada por su título para reordenar las secciones.
struct TagDrag {
    tag_id: String,
    start: Point,
    active: bool,
    /// Orden de vista previa (ids de etiqueta).
    order: Vec<String>,
    /// Altura del cursor en el último cambio de lugar; evita que dos secciones de distinto alto se intercambien sin parar.
    anchor_y: f32,
}

/// Nombre en edición de una etiqueta nueva; `artist_id` se agrega al crearla.
struct NewTagDraft {
    name: String,
    artist_id: Option<String>,
}

/// Artistas seguidos organizados en etiquetas definidas por el usuario.
pub struct ArtistsView {
    manager: Arc<ArtistTagManager>,
    tags: Vec<ArtistTag>,
    /// Miembros por etiqueta, en orden de alta.
    members: HashMap<String, Vec<String>>,
    thumbnails: AsyncThumbnail,
    new_tag: Option<NewTagDraft>,
    rename: Option<(String, String)>,
    menu: ContextMenu<ArtistMenuTarget>,
    menu_items: Vec<ContextMenuItem<ArtistMenuAction>>,
    delete_dialog: ConfirmDialog<String>,
    cursor: Point,
    hovered_card: Option<(String, Option<String>)>,
    hovered_section: Option<Option<String>>,
    drag: Option<ArtistDrag>,
    hovered_tag_title: Option<String>,
    tag_drag: Option<TagDrag>,
    /// Secciones expandidas a grilla (por defecto se ven como carrusel); `None` = "Artistas seguidos".
    expanded: HashSet<Option<String>>,
    /// Página del carrusel de cada sección contraída.
    pages: HashMap<Option<String>, usize>,
}

impl ArtistsView {
    pub fn new(manager: Arc<ArtistTagManager>) -> (Self, Task<ArtistsMessage>) {
        let view = Self {
            manager,
            tags: Vec::new(),
            members: HashMap::new(),
            thumbnails: AsyncThumbnail::new(PHOTO_REQUEST_SIDE),
            new_tag: None,
            rename: None,
            menu: ContextMenu::new(),
            menu_items: Vec::new(),
            delete_dialog: ConfirmDialog::new(),
            cursor: Point::ORIGIN,
            hovered_card: None,
            hovered_section: None,
            drag: None,
            hovered_tag_title: None,
            tag_drag: None,
            expanded: HashSet::new(),
            pages: HashMap::new(),
        };
        let task = view.reload();
        (view, task)
    }

    pub fn update(&mut self, message: ArtistsMessage) -> (Task<ArtistsMessage>, ArtistsOutMessage) {
        let mut out = ArtistsOutMessage::Idle;

        let task = match message {
            ArtistsMessage::Loaded(Ok((tags, members))) => {
                self.tags = tags;
                self.members.clear();
                for (tag_id, artist_id) in members {
                    self.members.entry(tag_id).or_default().push(artist_id);
                }
                Task::none()
            }
            ArtistsMessage::Loaded(Err(e)) => {
                eprintln!("[ARTISTAS] No se pudieron cargar las etiquetas: {e}");
                Task::none()
            }
            ArtistsMessage::Persisted(Ok(())) => Task::none(),
            ArtistsMessage::Persisted(Err(e)) => {
                eprintln!("[ARTISTAS] No se pudo guardar el cambio: {e}");
                self.reload()
            }
            ArtistsMessage::ThumbnailLoaded(key, bytes) => {
                self.thumbnails.on_loaded(key, bytes);
                Task::none()
            }
            ArtistsMessage::ArtistClicked(id) => {
                // La tarjeta deja de pintarse sin recibir el `on_exit`.
                self.clear_hover();
                out = ArtistsOutMessage::OpenArtist(id);
                Task::none()
            }
            ArtistsMessage::ArtistRightClicked { artist_id, tag_id } => {
                self.menu_items = self.artist_menu_items(&artist_id, tag_id.as_deref());
                self.menu.handle(ContextMenuEvent::RightClicked(ArtistMenuTarget::Artist { artist_id, tag_id }));
                Task::none()
            }
            ArtistsMessage::TagRightClicked(tag_id) => {
                self.menu_items = self.tag_menu_items(&tag_id);
                self.menu.handle(ContextMenuEvent::RightClicked(ArtistMenuTarget::Tag(tag_id)));
                Task::none()
            }
            ArtistsMessage::MenuEvent(event) => {
                if matches!(event, ContextMenuEvent::Dismissed) {
                    self.menu_items.clear();
                }
                self.menu.handle(event);
                Task::none()
            }
            ArtistsMessage::MenuAction(action, target) => {
                self.menu.handle(ContextMenuEvent::Dismissed);
                self.menu_items.clear();
                self.apply_menu_action(action, target)
            }
            ArtistsMessage::ShowNewTagInput => self.open_new_tag_input(None),
            ArtistsMessage::NewTagNameChanged(name) => {
                if let Some(draft) = &mut self.new_tag {
                    draft.name = name;
                }
                Task::none()
            }
            ArtistsMessage::SubmitNewTag => self.create_tag(),
            ArtistsMessage::StartRename(tag_id) => self.start_rename(&tag_id),
            ArtistsMessage::RenameChanged(name) => {
                if let Some((_, draft)) = &mut self.rename {
                    *draft = name;
                }
                Task::none()
            }
            ArtistsMessage::SubmitRename => self.submit_rename(),
            ArtistsMessage::ConfirmDeleteTag => match self.delete_dialog.take_confirmed() {
                Some(tag_id) => {
                    self.tags.retain(|t| t.id != tag_id);
                    self.members.remove(&tag_id);
                    self.persist(move |m| async move { m.delete_tag(&tag_id).await })
                }
                None => Task::none(),
            },
            ArtistsMessage::CancelDeleteTag => {
                self.delete_dialog.cancel();
                Task::none()
            }
            ArtistsMessage::CardHovered { artist_id, tag_id } => {
                // Sobre otra tarjeta de su misma etiqueta: ahí caería al soltar.
                if let Some(drag) = self.drag.as_mut().filter(|drag| drag.active)
                    && drag.from.is_some()
                    && drag.from == tag_id
                    && drag.artist_id != artist_id
                {
                    drag.target = self.members
                        .get(tag_id.as_deref().unwrap_or_default())
                        .and_then(|ids| ids.iter().position(|id| id == &artist_id));
                }
                self.hovered_card = Some((artist_id, tag_id));
                Task::none()
            }
            ArtistsMessage::CardUnhovered { artist_id, tag_id } => {
                if self.hovered_card.as_ref() == Some(&(artist_id, tag_id)) {
                    self.hovered_card = None;
                }
                Task::none()
            }
            ArtistsMessage::SectionHovered(section) => {
                if let Some(drag) = self.drag.as_mut().filter(|drag| drag.from != section) {
                    drag.target = None;
                }
                self.hovered_section = Some(section);
                self.update_tag_drag();
                Task::none()
            }
            ArtistsMessage::SectionUnhovered(section) => {
                if self.hovered_section.as_ref() == Some(&section) {
                    self.hovered_section = None;
                }
                Task::none()
            }
            ArtistsMessage::GlobalPressed => {
                self.drag = self.hovered_card.clone().map(|(artist_id, from)| ArtistDrag {
                    artist_id,
                    from,
                    start: self.cursor,
                    active: false,
                    target: None,
                });
                let is_renaming = |tag_id: &str| self.rename.as_ref().is_some_and(|(id, _)| id == tag_id);
                self.tag_drag = self.hovered_tag_title
                    .clone()
                    .filter(|tag_id| self.drag.is_none() && !is_renaming(tag_id))
                    .map(|tag_id| TagDrag {
                        tag_id,
                        start: self.cursor,
                        active: false,
                        order: self.tags.iter().map(|t| t.id.clone()).collect(),
                        anchor_y: self.cursor.y,
                    });
                Task::none()
            }
            ArtistsMessage::GlobalReleased => {
                let tag_task = match self.tag_drag.take() {
                    Some(drag) if drag.active => self.commit_tag_order(drag.order),
                    _ => Task::none(),
                };
                let artist_task = match self.drag.take() {
                    Some(drag) if drag.active => match self.hovered_section.clone() {
                        Some(to) => self.drop_artist(drag, to),
                        None => Task::none(),
                    },
                    _ => Task::none(),
                };
                Task::batch([tag_task, artist_task])
            }
            ArtistsMessage::TagTitleHovered(tag_id) => {
                self.hovered_tag_title = Some(tag_id);
                Task::none()
            }
            ArtistsMessage::TagTitleUnhovered(tag_id) => {
                if self.hovered_tag_title.as_deref() == Some(tag_id.as_str()) {
                    self.hovered_tag_title = None;
                }
                Task::none()
            }
            ArtistsMessage::ToggleSection(section) => {
                if !self.expanded.remove(&section) {
                    self.expanded.insert(section);
                }
                Task::none()
            }
            ArtistsMessage::SectionPrevPage(section) => {
                let page = self.pages.entry(section).or_default();
                *page = page.saturating_sub(1);
                Task::none()
            }
            ArtistsMessage::SectionNextPage(section) => {
                *self.pages.entry(section).or_default() += 1;
                Task::none()
            }
        };

        (task, out)
    }

    /// Mantiene vivas las fotos de los artistas seguidos (todos se muestran en la vista).
    pub fn sync<'a>(&mut self, followed: impl Iterator<Item = &'a FollowedArtist>) -> Task<ArtistsMessage> {
        let targets: Vec<(String, String)> = followed
            .filter_map(|artist| {
                let url = artist.photo_url.as_deref()?;
                Some((photo_key(&artist.artist_id), square_image_url(url, PHOTO_REQUEST_SIDE)))
            })
            .collect();
        self.thumbnails.sync(&targets, ArtistsMessage::ThumbnailLoaded)
    }

    /// Cursor global de la ventana: ancla del menú y posición del arrastre.
    pub fn set_cursor(&mut self, position: Point) {
        self.menu.handle(ContextMenuEvent::MouseMoved(position));
        self.cursor = position;
        if let Some(drag) = &mut self.drag
            && !drag.active
            && position.distance(drag.start) > DRAG_THRESHOLD_PX
        {
            drag.active = true;
        }
        if let Some(drag) = &mut self.tag_drag
            && !drag.active
            && position.distance(drag.start) > DRAG_THRESHOLD_PX
        {
            drag.active = true;
        }
        self.update_tag_drag();
    }

    /// Mueve la etiqueta arrastrada al lugar de la sección bajo el cursor, si el cursor avanzó hacia ella.
    fn update_tag_drag(&mut self) {
        let cursor_y = self.cursor.y;
        let Some(Some(hovered)) = self.hovered_section.clone() else { return };
        let Some(drag) = self.tag_drag.as_mut().filter(|drag| drag.active && drag.tag_id != hovered) else { return };
        let (Some(from), Some(to)) = (
            drag.order.iter().position(|id| id == &drag.tag_id),
            drag.order.iter().position(|id| id == &hovered),
        ) else {
            return;
        };

        let moved = cursor_y - drag.anchor_y;
        let toward_target = if to > from { moved > TAG_SWAP_HYSTERESIS_PX } else { moved < -TAG_SWAP_HYSTERESIS_PX };
        if toward_target {
            let id = drag.order.remove(from);
            drag.order.insert(to, id);
            drag.anchor_y = cursor_y;
        }
    }

    fn active_tag_drag(&self) -> Option<&TagDrag> {
        self.tag_drag.as_ref().filter(|drag| drag.active)
    }

    /// Etiquetas en el orden a pintar: con la vista previa si se está arrastrando una.
    fn display_tags(&self) -> Vec<&ArtistTag> {
        match self.active_tag_drag() {
            Some(drag) => drag.order.iter().filter_map(|id| self.tags.iter().find(|t| &t.id == id)).collect(),
            None => self.tags.iter().collect(),
        }
    }

    /// Olvida qué tarjeta/sección está bajo el cursor (al dejar de mostrarse la vista).
    pub fn clear_hover(&mut self) {
        self.hovered_card = None;
        self.hovered_section = None;
        self.drag = None;
        self.hovered_tag_title = None;
        self.tag_drag = None;
    }

    fn active_drag(&self) -> Option<&ArtistDrag> {
        self.drag.as_ref().filter(|drag| drag.active)
    }

    /// Suelta un artista en la sección `to`:
    /// - en su misma etiqueta, lo reordena;
    /// - en otra etiqueta, lo mueve (o lo copia si viene de "Artistas seguidos")
    ///   a la posición de la tarjeta bajo el cursor, o al final;
    /// - en "Artistas seguidos", lo saca de la etiqueta de origen.
    fn drop_artist(&mut self, drag: ArtistDrag, to: Option<String>) -> Task<ArtistsMessage> {
        let ArtistDrag { artist_id, from, target, .. } = drag;

        match (from, to) {
            (Some(from), Some(to)) if from == to => match target {
                Some(index) => {
                    let order = self.ordered_members_with(&to, &artist_id, index);
                    self.members.insert(to.clone(), order.clone());
                    self.persist(move |m| async move { m.set_member_order(&to, &order).await })
                }
                None => Task::none(),
            },
            (from, Some(to)) => {
                let index = self.hovered_card
                    .as_ref()
                    .filter(|(_, tag)| tag.as_deref() == Some(to.as_str()))
                    .and_then(|(hovered, _)| self.members.get(&to)?.iter().position(|id| id == hovered));

                if let Some(from) = &from
                    && let Some(ids) = self.members.get_mut(from)
                {
                    ids.retain(|id| id != &artist_id);
                }
                let ids = self.members.entry(to.clone()).or_default();
                ids.retain(|id| id != &artist_id);
                ids.insert(index.unwrap_or(ids.len()).min(ids.len()), artist_id.clone());
                let order = ids.clone();

                self.persist(move |m| async move {
                    m.add_member(&to, &artist_id).await?;
                    if let Some(from) = from {
                        m.remove_member(&from, &artist_id).await?;
                    }
                    m.set_member_order(&to, &order).await
                })
            }
            (Some(from), None) => self.apply_menu_action(
                ArtistMenuAction::RemoveFromTag(from.clone()),
                ArtistMenuTarget::Artist { artist_id, tag_id: Some(from) },
            ),
            (None, None) => Task::none(),
        }
    }

    /// Miembros de la etiqueta con `artist_id` movido a `index`.
    fn ordered_members_with(&self, tag_id: &str, artist_id: &str, index: usize) -> Vec<String> {
        let mut ids = self.members.get(tag_id).cloned().unwrap_or_default();
        if let Some(current) = ids.iter().position(|id| id == artist_id) {
            let moved = ids.remove(current);
            ids.insert(index.min(ids.len()), moved);
        }
        ids
    }

    /// Miembros en el orden a pintar: con la vista previa del reordenamiento si se está arrastrando dentro de la etiqueta.
    fn display_members(&self, tag_id: &str) -> Vec<String> {
        match self.active_drag() {
            Some(drag) if drag.from.as_deref() == Some(tag_id) => match drag.target {
                Some(index) => self.ordered_members_with(tag_id, &drag.artist_id, index),
                None => self.members.get(tag_id).cloned().unwrap_or_default(),
            },
            _ => self.members.get(tag_id).cloned().unwrap_or_default(),
        }
    }

    pub fn set_viewport(&mut self, size: iced::Size) {
        self.menu.handle(ContextMenuEvent::ViewportResized(size));
    }

    /// Cierra lo que esté abierto (input, renombre, diálogo, arrastre); `true` si había algo.
    pub fn cancel_edit(&mut self) -> bool {
        let was_open = self.new_tag.is_some()
            || self.rename.is_some()
            || self.delete_dialog.is_open()
            || self.active_drag().is_some()
            || self.active_tag_drag().is_some();
        self.new_tag = None;
        self.rename = None;
        self.delete_dialog.cancel();
        self.drag = None;
        self.tag_drag = None;
        was_open
    }

    pub fn view<'a>(&'a self, catalog: &'a CatalogStore) -> Element<'a, ArtistsMessage> {
        let mut followed: Vec<&FollowedArtist> = catalog.followed_artists().collect();
        followed.sort_by_key(|artist| std::cmp::Reverse(artist.followed_at));

        let mut children: Vec<Element<'a, ArtistsMessage>> = vec![self.view_header()];

        if followed.is_empty() {
            children.push(muted_text("Todavía no sigues a ningún artista. Abre la página de un artista y pulsa «Seguir» para organizarlo aquí."));
        } else {
            for tag in self.display_tags() {
                let artists: Vec<&FollowedArtist> = self
                    .display_members(&tag.id)
                    .iter()
                    .filter_map(|id| catalog.followed_artist(id))
                    .collect();
                children.push(self.view_section(
                    Some(tag.id.clone()),
                    artists,
                    "Sin artistas todavía. Arrastra un artista aquí o usa el clic derecho.",
                ));
            }

            children.push(self.view_section(None, followed, ""));
        }

        scrollable(
            column(children)
                .spacing(spacing::SP_28)
                .padding(Padding { top: spacing::SP_8, right: spacing::SP_24, bottom: spacing::SP_32, left: spacing::SP_24 }),
        )
            .width(Length::Fill)
            .height(Length::Fill)
            .style(scrollable_style::discreet)
            .id(Id::new("artists_view_scroll"))
            .into()
    }

    /// Fantasma del arrastre, menú contextual y diálogo de borrado, para apilar a nivel ventana.
    pub fn view_overlays<'a>(&'a self, catalog: &'a CatalogStore) -> Vec<Element<'a, ArtistsMessage>> {
        let mut layers = Vec::new();

        if let Some(ghost) = self.active_drag().and_then(|drag| self.view_drag_ghost(drag, catalog)) {
            layers.push(ghost);
        }
        if let Some(ghost) = self.active_tag_drag().and_then(|drag| self.view_tag_ghost(drag)) {
            layers.push(ghost);
        }

        let open_target = self.menu.open_id();
        if let Some((anchor, target)) = self.menu.render_target(|_| open_target) {
            layers.push(self.menu.view(
                anchor,
                self.menu_items.clone(),
                target,
                ArtistsMessage::MenuAction,
                ArtistsMessage::MenuEvent(ContextMenuEvent::Dismissed),
                |sub| ArtistsMessage::MenuEvent(ContextMenuEvent::SubmenuHovered(sub)),
            ));
        }

        if let Some(dialog) = self.delete_dialog.view(ArtistsMessage::ConfirmDeleteTag, ArtistsMessage::CancelDeleteTag) {
            layers.push(dialog);
        }

        layers
    }

    fn view_header(&self) -> Element<'_, ArtistsMessage> {
        let title = text("Artistas").font(SF_PRO).size(typography::TEXT_28).color(theme().content.primary);

        let action: Element<'_, ArtistsMessage> = match &self.new_tag {
            Some(draft) => text_input("Nombre de la etiqueta (Enter para crear)", &draft.name)
                .id(Id::new(NEW_TAG_INPUT_ID))
                .on_input(ArtistsMessage::NewTagNameChanged)
                .on_submit(ArtistsMessage::SubmitNewTag)
                .font(SF_PRO)
                .size(typography::TEXT_13)
                .padding(Padding { top: spacing::SP_6, bottom: spacing::SP_6, left: spacing::SP_10, right: spacing::SP_10 })
                .style(text_input_style::field)
                .width(Length::Fixed(NEW_TAG_INPUT_WIDTH))
                .into(),
            None => button(
                row![
                    crate::ui::assets::icons::icon(Icon::Add, typography::TEXT_13),
                    text("Nueva etiqueta").font(SF_PRO).size(typography::TEXT_13).color(theme().content.primary),
                ]
                    .spacing(spacing::SP_6)
                    .align_y(Alignment::Center),
            )
                .padding(Padding { top: spacing::SP_6, bottom: spacing::SP_6, left: spacing::SP_14, right: spacing::SP_14 })
                .style(button_style::pill(false))
                .on_press(ArtistsMessage::ShowNewTagInput)
                .into(),
        };

        row![title, space().width(Length::Fill), action]
            .align_y(Alignment::Center)
            .height(Length::Fixed(HEADER_HEIGHT))
            .into()
    }

    /// Sección de artistas (`None` = "Artistas seguidos", con todos los que sigues).
    /// Contraída es un carrusel de una fila con flechas; expandida, la grilla
    /// completa donde se arrastra para reordenar o mover a otra etiqueta.
    fn view_section<'a>(&'a self, section: Option<String>, artists: Vec<&'a FollowedArtist>, empty_text: &'a str) -> Element<'a, ArtistsMessage> {
        let is_expanded = self.expanded.contains(&section);
        let page = self.pages.get(&section).copied().unwrap_or(0);
        let count = artists.len();
        let key = section.clone();

        let content = responsive(move |size| {
            let per_page = cards_per_page(size.width);
            let total_pages = count.div_ceil(per_page).max(1);
            let page = page.min(total_pages - 1);

            let title = row![self.section_name(key.as_deref()), section_count(count), section_divider()]
                .spacing(spacing::SP_10)
                .align_y(Alignment::Center)
                .width(Length::Fill);
            let title: Element<'a, ArtistsMessage> = match &key {
                Some(tag_id) => mouse_area(title)
                    .interaction(iced::mouse::Interaction::Grab)
                    .on_enter(ArtistsMessage::TagTitleHovered(tag_id.clone()))
                    .on_exit(ArtistsMessage::TagTitleUnhovered(tag_id.clone()))
                    .into(),
                None => title.into(),
            };
            let mut header = row![title]
                .spacing(spacing::SP_10)
                .align_y(Alignment::Center)
                .height(Length::Fixed(SECTION_TITLE_HEIGHT));
            if !is_expanded && total_pages > 1 {
                header = header
                    .push(carousel_arrow(Icon::LeftArrow, (page > 0).then(|| ArtistsMessage::SectionPrevPage(key.clone()))))
                    .push(carousel_arrow(Icon::RightArrow, (page + 1 < total_pages).then(|| ArtistsMessage::SectionNextPage(key.clone()))));
            }
            if count > 0 {
                header = header.push(expand_toggle(is_expanded, ArtistsMessage::ToggleSection(key.clone())));
            }

            let body: Element<'a, ArtistsMessage> = if artists.is_empty() {
                muted_text(empty_text)
            } else if is_expanded {
                let cards: Vec<Element<'a, ArtistsMessage>> = artists.iter().map(|artist| self.view_card(artist, key.clone())).collect();
                row(cards).spacing(CARD_SPACING).wrap().vertical_spacing(CARD_SPACING).into()
            } else {
                let start = page * per_page;
                let end = (start + per_page).min(count);
                let cards: Vec<Element<'a, ArtistsMessage>> = artists[start..end].iter().map(|artist| self.view_card(artist, key.clone())).collect();
                row(cards).spacing(CARD_SPACING).into()
            };

            column![header, body].spacing(spacing::SP_12).into()
        });

        self.drop_zone(section, content.into())
    }

    /// Nombre de la sección: la etiqueta (clic derecho = menú, doble clic = renombrar) o "Artistas seguidos".
    fn section_name<'a>(&'a self, tag_id: Option<&str>) -> Element<'a, ArtistsMessage> {
        let Some(tag) = tag_id.and_then(|id| self.tags.iter().find(|t| t.id == id)) else {
            return row![
                crate::ui::assets::icons::icon(Icon::Artists, typography::TEXT_14).color(theme().content.secondary),
                text("Artistas seguidos").font(SF_PRO).size(typography::TEXT_18).color(theme().content.primary),
            ]
                .spacing(spacing::SP_8)
                .align_y(Alignment::Center)
                .into();
        };

        match &self.rename {
            Some((id, draft)) if id == &tag.id => text_input("Nombre de la etiqueta", draft)
                .id(Id::new(RENAME_INPUT_ID))
                .on_input(ArtistsMessage::RenameChanged)
                .on_submit(ArtistsMessage::SubmitRename)
                .font(SF_PRO)
                .size(typography::TEXT_16)
                .padding(Padding { top: spacing::SP_2, bottom: spacing::SP_2, left: spacing::SP_8, right: spacing::SP_8 })
                .style(text_input_style::field)
                .width(Length::Fixed(NEW_TAG_INPUT_WIDTH))
                .into(),
            _ => mouse_area(
                row![
                    crate::ui::assets::icons::icon(Icon::Tag, typography::TEXT_14).color(theme().accent.primary),
                    text(tag.name.as_str()).font(SF_PRO).size(typography::TEXT_18).color(theme().content.primary),
                ]
                    .spacing(spacing::SP_8)
                    .align_y(Alignment::Center),
            )
                .on_right_press(ArtistsMessage::TagRightClicked(tag.id.clone()))
                .on_double_click(ArtistsMessage::StartRename(tag.id.clone()))
                .into(),
        }
    }

    /// Envuelve una sección para saber si el cursor está encima y resaltarla como destino al arrastrar.
    fn drop_zone<'a>(&'a self, section: Option<String>, content: Element<'a, ArtistsMessage>) -> Element<'a, ArtistsMessage> {
        let is_target = (self.active_drag().is_some_and(|drag| drag.from != section)
            && self.hovered_section.as_ref() == Some(&section))
            || self.active_tag_drag().is_some_and(|drag| section.as_deref() == Some(drag.tag_id.as_str()));

        let zone = container(content)
            .width(Length::Fill)
            .padding(spacing::SP_8)
            .style(move |_theme: &Theme| container::Style {
                background: is_target.then(|| theme().overlay.hover.into()),
                border: rounded(radii::R_12)
                    .color(if is_target { theme().accent.primary } else { iced::Color::TRANSPARENT })
                    .width(1.0),
                ..Default::default()
            });

        mouse_area(zone)
            .on_enter(ArtistsMessage::SectionHovered(section.clone()))
            .on_exit(ArtistsMessage::SectionUnhovered(section))
            .into()
    }

    /// Tarjeta circular: foto y nombre. Clic abre el artista, clic derecho abre el
    /// menú de etiquetas, y arrastrarla la lleva a otra sección.
    fn view_card<'a>(&'a self, artist: &'a FollowedArtist, tag_id: Option<String>) -> Element<'a, ArtistsMessage> {
        let drag = self.active_drag();
        let is_dragged = drag.is_some_and(|drag| drag.artist_id == artist.artist_id && drag.from == tag_id);
        let opacity = if is_dragged { DRAGGED_CARD_OPACITY } else { 1.0 };

        let content = column![
            self.photo(&artist.artist_id, PHOTO_SIZE, opacity),
            text(truncate(artist.name.as_str(), CARD_NAME_MAX_CHARS))
                .font(SF_PRO)
                .size(typography::TEXT_13)
                .color(if is_dragged { theme().content.muted } else { theme().content.primary })
                .width(Length::Fixed(PHOTO_SIZE))
                .height(Length::Fixed(CARD_NAME_LINE_HEIGHT))
                .align_x(Alignment::Center),
        ]
            .spacing(spacing::SP_8)
            .align_x(Alignment::Center);

        // Durante un arrastre las tarjetas no se resaltan ni abren el artista al soltar.
        let card = button(content).padding(CARD_HOVER_PADDING);
        let card = if drag.is_some() {
            card.style(button_style::inert)
        } else {
            card.style(button_style::card_hover(radii::R_12)).on_press(ArtistsMessage::ArtistClicked(artist.artist_id.clone()))
        };

        mouse_area(card)
            .on_right_press(ArtistsMessage::ArtistRightClicked { artist_id: artist.artist_id.clone(), tag_id: tag_id.clone() })
            .on_enter(ArtistsMessage::CardHovered { artist_id: artist.artist_id.clone(), tag_id: tag_id.clone() })
            .on_exit(ArtistsMessage::CardUnhovered { artist_id: artist.artist_id.clone(), tag_id })
            .into()
    }

    /// Foto circular del artista (o un círculo vacío mientras carga).
    fn photo<'a>(&'a self, artist_id: &str, size: f32, opacity: f32) -> Element<'a, ArtistsMessage> {
        match self.thumbnails.get(&photo_key(artist_id)) {
            Some(handle) => image(handle.clone())
                .width(Length::Fixed(size))
                .height(Length::Fixed(size))
                .content_fit(ContentFit::Cover)
                .border_radius(size / 2.0)
                .opacity(opacity)
                .into(),
            None => container(space())
                .width(Length::Fixed(size))
                .height(Length::Fixed(size))
                .style(move |_theme: &Theme| container::Style {
                    background: Some(theme().surface.sunken.into()),
                    border: rounded(size / 2.0),
                    ..Default::default()
                })
                .into(),
        }
    }

    /// Tarjeta que sigue al cursor mientras se arrastra un artista.
    fn view_drag_ghost<'a>(&'a self, drag: &ArtistDrag, catalog: &'a CatalogStore) -> Option<Element<'a, ArtistsMessage>> {
        let artist = catalog.followed_artist(&drag.artist_id)?;
        let card = container(
            column![
                self.photo(&artist.artist_id, GHOST_PHOTO_SIZE, 1.0),
                text(truncate(artist.name.as_str(), CARD_NAME_MAX_CHARS))
                    .font(SF_PRO)
                    .size(typography::TEXT_12)
                    .color(theme().content.primary)
                    .width(Length::Fixed(GHOST_PHOTO_SIZE + 2.0 * CARD_HOVER_PADDING))
                    .align_x(Alignment::Center),
            ]
                .spacing(spacing::SP_6)
                .align_x(Alignment::Center),
        )
            .padding(CARD_HOVER_PADDING)
            .style(|_theme: &Theme| container::Style {
                background: Some(theme().overlay.hover.into()),
                border: rounded(radii::R_12).color(theme().border.subtle).width(1.0),
                ..Default::default()
            });

        let half = GHOST_PHOTO_SIZE / 2.0 + CARD_HOVER_PADDING;
        Some(pin(card).x(self.cursor.x - half).y(self.cursor.y - half).into())
    }

    /// Píldora con el nombre de la etiqueta que sigue al cursor mientras se reordena.
    fn view_tag_ghost<'a>(&'a self, drag: &TagDrag) -> Option<Element<'a, ArtistsMessage>> {
        let tag = self.tags.iter().find(|t| t.id == drag.tag_id)?;
        Some(drag_pill(Icon::Tag, tag.name.as_str(), self.cursor))
    }

    fn artist_menu_items(&self, artist_id: &str, tag_id: Option<&str>) -> Vec<ContextMenuItem<ArtistMenuAction>> {
        let is_member = |tag: &ArtistTag| self.members.get(&tag.id).is_some_and(|ids| ids.iter().any(|id| id == artist_id));

        let other_tags = |to_action: fn(String) -> ArtistMenuAction| -> Vec<ContextMenuItem<ArtistMenuAction>> {
            self.tags
                .iter()
                .filter(|tag| !is_member(tag))
                .map(|tag| ContextMenuItem::new(tag.name.clone(), to_action(tag.id.clone())).icon(Icon::Tag))
                .collect()
        };

        let mut add_children = other_tags(ArtistMenuAction::AddToTag);
        add_children.push(ContextMenuItem::new("Nueva etiqueta…", ArtistMenuAction::NewTagWithArtist).icon(Icon::Add));

        let mut items = vec![ContextMenuItem::submenu("Agregar a etiqueta", ADD_TO_TAG_SUBMENU_ID, add_children).icon(Icon::Tag)];

        if let Some(tag) = tag_id.and_then(|id| self.tags.iter().find(|t| t.id == id)) {
            let move_children = other_tags(ArtistMenuAction::MoveToTag);
            if !move_children.is_empty() {
                items.push(ContextMenuItem::submenu("Mover a etiqueta", MOVE_TO_TAG_SUBMENU_ID, move_children).icon(Icon::RightArrow));
            }
            items.push(
                ContextMenuItem::new(format!("Quitar de «{}»", tag.name), ArtistMenuAction::RemoveFromTag(tag.id.clone()))
                    .icon(Icon::DeleteOpen),
            );
        }

        items
    }

    fn tag_menu_items(&self, tag_id: &str) -> Vec<ContextMenuItem<ArtistMenuAction>> {
        let index = self.tags.iter().position(|t| t.id == tag_id).unwrap_or(0);

        let mut items = vec![ContextMenuItem::new("Renombrar", ArtistMenuAction::RenameTag).icon(Icon::Rename)];
        if index > 0 {
            items.push(ContextMenuItem::new("Subir", ArtistMenuAction::MoveTagUp).icon(Icon::ExpandLess));
        }
        if index + 1 < self.tags.len() {
            items.push(ContextMenuItem::new("Bajar", ArtistMenuAction::MoveTagDown).icon(Icon::ExpandMore));
        }
        items.push(ContextMenuItem::new("Eliminar etiqueta", ArtistMenuAction::DeleteTag).icon(Icon::Delete));
        items
    }

    fn apply_menu_action(&mut self, action: ArtistMenuAction, target: ArtistMenuTarget) -> Task<ArtistsMessage> {
        match (action, target) {
            (ArtistMenuAction::AddToTag(tag_id), ArtistMenuTarget::Artist { artist_id, .. }) => self.add_member(tag_id, artist_id),
            (ArtistMenuAction::MoveToTag(to), ArtistMenuTarget::Artist { artist_id, tag_id: Some(from) }) => {
                if let Some(ids) = self.members.get_mut(&from) {
                    ids.retain(|id| id != &artist_id);
                }
                let ids = self.members.entry(to.clone()).or_default();
                if !ids.contains(&artist_id) {
                    ids.push(artist_id.clone());
                }
                self.persist(move |m| async move {
                    m.add_member(&to, &artist_id).await?;
                    m.remove_member(&from, &artist_id).await
                })
            }
            (ArtistMenuAction::RemoveFromTag(tag_id), ArtistMenuTarget::Artist { artist_id, .. }) => {
                if let Some(ids) = self.members.get_mut(&tag_id) {
                    ids.retain(|id| id != &artist_id);
                }
                self.persist(move |m| async move { m.remove_member(&tag_id, &artist_id).await })
            }
            (ArtistMenuAction::NewTagWithArtist, ArtistMenuTarget::Artist { artist_id, .. }) => self.open_new_tag_input(Some(artist_id)),
            (ArtistMenuAction::RenameTag, ArtistMenuTarget::Tag(tag_id)) => self.start_rename(&tag_id),
            (ArtistMenuAction::MoveTagUp, ArtistMenuTarget::Tag(tag_id)) => self.move_tag(&tag_id, -1),
            (ArtistMenuAction::MoveTagDown, ArtistMenuTarget::Tag(tag_id)) => self.move_tag(&tag_id, 1),
            (ArtistMenuAction::DeleteTag, ArtistMenuTarget::Tag(tag_id)) => {
                let name = self.tags.iter().find(|t| t.id == tag_id).map(|t| t.name.clone()).unwrap_or_default();
                self.delete_dialog.request(tag_id, format!("¿Eliminar la etiqueta «{name}»? Los artistas siguen en tu lista."));
                Task::none()
            }
            _ => Task::none(),
        }
    }

    fn add_member(&mut self, tag_id: String, artist_id: String) -> Task<ArtistsMessage> {
        let ids = self.members.entry(tag_id.clone()).or_default();
        if ids.contains(&artist_id) {
            return Task::none();
        }
        ids.push(artist_id.clone());
        self.persist(move |m| async move { m.add_member(&tag_id, &artist_id).await })
    }

    fn open_new_tag_input(&mut self, artist_id: Option<String>) -> Task<ArtistsMessage> {
        self.rename = None;
        self.new_tag = Some(NewTagDraft { name: String::new(), artist_id });
        iced::widget::operation::focus(Id::new(NEW_TAG_INPUT_ID))
    }

    fn create_tag(&mut self) -> Task<ArtistsMessage> {
        let Some(draft) = self.new_tag.take() else { return Task::none() };
        let name = draft.name.trim().to_string();
        if name.is_empty() {
            return Task::none();
        }

        let tag = ArtistTag {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            position: self.tags.last().map_or(0.0, |t| t.position + 1.0),
        };
        self.tags.push(tag.clone());

        if let Some(artist_id) = &draft.artist_id {
            self.members.entry(tag.id.clone()).or_default().push(artist_id.clone());
        }

        // El alta del miembro va después de crear la etiqueta (la FK la necesita).
        self.persist(move |m| async move {
            m.create_tag(&tag).await?;
            match draft.artist_id {
                Some(artist_id) => m.add_member(&tag.id, &artist_id).await,
                None => Ok(()),
            }
        })
    }

    fn start_rename(&mut self, tag_id: &str) -> Task<ArtistsMessage> {
        let Some(tag) = self.tags.iter().find(|t| t.id == tag_id) else { return Task::none() };
        self.new_tag = None;
        self.rename = Some((tag.id.clone(), tag.name.clone()));
        let id = Id::new(RENAME_INPUT_ID);
        Task::batch([iced::widget::operation::focus(id.clone()), iced::widget::operation::select_all(id)])
    }

    fn submit_rename(&mut self) -> Task<ArtistsMessage> {
        let Some((tag_id, draft)) = self.rename.take() else { return Task::none() };
        let name = draft.trim().to_string();
        let Some(tag) = self.tags.iter_mut().find(|t| t.id == tag_id) else { return Task::none() };
        if name.is_empty() || name == tag.name {
            return Task::none();
        }
        tag.name = name.clone();
        self.persist(move |m| async move { m.rename_tag(&tag_id, &name).await })
    }

    /// Intercambia la etiqueta con su vecina (`delta` = -1 sube, 1 baja).
    fn move_tag(&mut self, tag_id: &str, delta: isize) -> Task<ArtistsMessage> {
        let Some(index) = self.tags.iter().position(|t| t.id == tag_id) else { return Task::none() };
        let Some(other) = index.checked_add_signed(delta).filter(|&i| i < self.tags.len()) else { return Task::none() };

        let mut order: Vec<String> = self.tags.iter().map(|t| t.id.clone()).collect();
        order.swap(index, other);
        self.commit_tag_order(order)
    }

    /// Aplica y guarda un nuevo orden de etiquetas (no hace nada si no cambió).
    fn commit_tag_order(&mut self, order: Vec<String>) -> Task<ArtistsMessage> {
        if self.tags.iter().map(|t| &t.id).eq(order.iter()) {
            return Task::none();
        }
        self.tags.sort_by_key(|tag| order.iter().position(|id| id == &tag.id).unwrap_or(usize::MAX));
        for (position, tag) in self.tags.iter_mut().enumerate() {
            tag.position = position as f64;
        }
        let positions: Vec<(String, f64)> = self.tags.iter().map(|t| (t.id.clone(), t.position)).collect();
        self.persist(move |m| async move { m.set_tag_positions(&positions).await })
    }

    /// Relee etiquetas y miembros de SQLite (dejar de seguir a un artista lo saca de sus etiquetas).
    pub fn reload(&self) -> Task<ArtistsMessage> {
        let manager = Arc::clone(&self.manager);
        Task::perform(
            async move { manager.load().await.map_err(|e| e.to_string()) },
            ArtistsMessage::Loaded,
        )
    }

    /// Corre una escritura en SQLite y reporta el resultado como `Persisted`.
    fn persist<F, Fut>(&self, op: F) -> Task<ArtistsMessage>
    where
        F: FnOnce(Arc<ArtistTagManager>) -> Fut + Send + 'static,
        Fut: Future<Output = Result<(), sqlx::Error>> + Send + 'static,
    {
        let manager = Arc::clone(&self.manager);
        Task::perform(async move { op(manager).await.map_err(|e| e.to_string()) }, ArtistsMessage::Persisted)
    }
}

fn photo_key(artist_id: &str) -> String {
    format!("artists_view:{artist_id}")
}

fn section_count<'a>(count: usize) -> Element<'a, ArtistsMessage> {
    text(count.to_string()).font(SF_PRO).size(typography::TEXT_13).color(theme().content.muted).into()
}

/// Botón circular con flecha del carrusel; sin `on_press` en el extremo.
fn carousel_arrow<'a>(icon: Icon, on_press: Option<ArtistsMessage>) -> Element<'a, ArtistsMessage> {
    let glyph = container(crate::ui::assets::icons::icon(icon, typography::TEXT_13))
        .width(Length::Fixed(ARROW_SIZE))
        .height(Length::Fixed(ARROW_SIZE))
        .align_x(Alignment::Center)
        .align_y(Alignment::Center);
    let enabled = on_press.is_some();
    let arrow = button(glyph).padding(spacing::SP_0).style(button_style::carousel_arrow(enabled, ARROW_SIZE));
    match on_press {
        Some(message) => arrow.on_press(message).into(),
        None => arrow.into(),
    }
}

/// Línea tenue que ocupa el espacio libre del título de la sección.
fn section_divider<'a>() -> Element<'a, ArtistsMessage> {
    rule::horizontal(1.0)
        .style(|_theme: &Theme| rule::Style {
            color: theme().border.subtle,
            radius: radii::R_NONE.into(),
            fill_mode: rule::FillMode::Full,
            snap: false,
        })
        .into()
}

/// Expande la sección a grilla o la vuelve a contraer a carrusel.
fn expand_toggle<'a>(is_expanded: bool, on_press: ArtistsMessage) -> Element<'a, ArtistsMessage> {
    let icon = if is_expanded { Icon::ExpandLess } else { Icon::ExpandMore };
    button(
        container(crate::ui::assets::icons::icon(icon, typography::TEXT_16))
            .width(Length::Fixed(ARROW_SIZE))
            .height(Length::Fixed(ARROW_SIZE))
            .align_x(Alignment::Center)
            .align_y(Alignment::Center),
    )
        .padding(spacing::SP_0)
        .style(button_style::minimal)
        .on_press(on_press)
        .into()
}

/// Cuántas tarjetas (más el espacio entre ellas) caben en `width`.
fn cards_per_page(width: f32) -> usize {
    let unit = PHOTO_SIZE + 2.0 * CARD_HOVER_PADDING + CARD_SPACING;
    (((width + CARD_SPACING) / unit).floor() as usize).max(1)
}

fn muted_text<'a>(message: &'a str) -> Element<'a, ArtistsMessage> {
    text(message).font(SF_PRO).size(typography::TEXT_13).color(theme().content.muted).into()
}
