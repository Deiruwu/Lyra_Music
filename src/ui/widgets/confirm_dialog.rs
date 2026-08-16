//! # ConfirmDialog — popup de confirmación modal, centrado en pantalla
//!
//! Independiente de `ContextMenu`: se usa cuando una acción (p. ej.
//! eliminar) necesita que el usuario confirme antes de ejecutarse.
//! A diferencia del menú contextual, este NO se ancla al punto de
//! click; siempre se muestra centrado sobre toda la vista, con un
//! fondo oscurecido detrás.
//!
//! ## Cómo se consume
//!
//! ```ignore
//! // 1. Un campo en tu vista:
//! confirm_dialog: ConfirmDialog<Track>,
//!
//! // 2. Cuando el usuario pide una acción que requiere confirmación
//! //    (p. ej. desde el ContextMenuAction::Delete), en vez de
//! //    ejecutarla de inmediato, la guardas pendiente:
//! self.confirm_dialog.request(track, "¿Eliminar esta canción del catálogo?");
//!
//! // 3. En tu view(), si hay una confirmación pendiente, la pintas
//! //    encima de todo lo demás:
//! if let Some(dialog) = self.confirm_dialog.view(
//!     MyMsg::ConfirmDialogConfirm,
//!     MyMsg::ConfirmDialogCancel,
//! ) {
//!     stack![tu_contenido, dialog].into()
//! } else {
//!     tu_contenido.into()
//! }
//!
//! // 4. En tu update():
//! MyMsg::ConfirmDialogConfirm => {
//!     if let Some(track) = self.confirm_dialog.take_confirmed() {
//!         // ejecutar la acción real con `track`
//!     }
//! }
//! MyMsg::ConfirmDialogCancel => self.confirm_dialog.cancel(),
//! ```

use iced::{Alignment, Color, Element, Length, Padding};
use iced::widget::{button, column, container, mouse_area, row, text};
use crate::ui::assets::fonts::SF_PRO;
use crate::ui::styles::styles::{context_menu_container, context_menu_item};

#[derive(Debug, Clone)]
pub struct ConfirmDialog<Item> {
    pending: Option<(Item, String)>, // Cambiado de &'static str a String
}

impl<Item> Default for ConfirmDialog<Item> {
    fn default() -> Self {
        Self { pending: None }
    }
}

impl<Item: Clone> ConfirmDialog<Item> {
    pub fn new() -> Self {
        Self { pending: None }
    }

    pub fn request(&mut self, item: Item, prompt: impl Into<String>) {
        self.pending = Some((item, prompt.into()));
    }

    pub fn cancel(&mut self) {
        self.pending = None;
    }

    /// Consume la confirmación pendiente (si la hay) y regresa el ítem
    /// asociado. Después de llamar esto el diálogo queda cerrado.
    pub fn take_confirmed(&mut self) -> Option<Item> {
        self.pending.take().map(|(item, _)| item)
    }

    pub fn is_open(&self) -> bool {
        self.pending.is_some()
    }

    pub fn view<'a, Msg: Clone + 'a>(
        &self,
        confirm_msg: Msg,
        cancel_msg: Msg,
    ) -> Option<Element<'a, Msg>> {
        let (_, prompt) = self.pending.as_ref()?;

        let prompt_text = text(prompt.clone())
            .font(SF_PRO)
            .size(14)
            .color(Color::WHITE);

        let confirm_btn = button(
            text("Confirmar").font(SF_PRO).size(13).color(Color::WHITE),
        )
            .style(context_menu_item)
            .padding(Padding { top: 8.0, bottom: 8.0, left: 18.0, right: 18.0 })
            .on_press(confirm_msg);

        let cancel_btn = button(
            text("Cancelar").font(SF_PRO).size(13).color(Color::from_rgb(0.7, 0.7, 0.75)),
        )
            .style(context_menu_item)
            .padding(Padding { top: 8.0, bottom: 8.0, left: 18.0, right: 18.0 })
            .on_press(cancel_msg.clone());

        let card = container(
            column![
                container(prompt_text)
                    .width(Length::Fixed(260.0))
                    .align_x(Alignment::Center)
                    .padding(Padding { top: 4.0, bottom: 16.0, left: 4.0, right: 4.0 }),
                row![confirm_btn, cancel_btn].spacing(10).align_y(Alignment::Center),
            ]
                .align_x(Alignment::Center)
                .spacing(4),
        )
            .padding(24)
            .style(context_menu_container);

        let backdrop = mouse_area(
            container(card)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::Center)
                .align_y(Alignment::Center)
                .style(|_| container::Style {
                    background: Some(Color { r: 0.0, g: 0.0, b: 0.0, a: 0.55 }.into()),
                    ..Default::default()
                }),
        )
            .on_press(cancel_msg);

        Some(backdrop.into())
    }
}