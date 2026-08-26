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

use iced::{Alignment, Element, Length, Padding};
use iced::widget::{button, column, container, mouse_area, row, text};
use crate::ui::assets::fonts::SF_PRO;
use crate::ui::styles::button as button_style;
use crate::ui::styles::container as container_style;
use crate::ui::assets::{spacing, typography};
use crate::ui::theme::theme;

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
            .size(typography::TEXT_14)
            .color(theme().content.primary);

        let confirm_btn = button(
            text("Confirmar").font(SF_PRO).size(typography::TEXT_13).color(theme().content.primary),
        )
            .style(button_style::context_menu_item)
            .padding(Padding { top: spacing::SP_8, bottom: spacing::SP_8, left: spacing::SP_18, right: spacing::SP_18 })
            .on_press(confirm_msg);

        let cancel_btn = button(
            text("Cancelar").font(SF_PRO).size(typography::TEXT_13).color(theme().content.secondary),
        )
            .style(button_style::context_menu_item)
            .padding(Padding { top: spacing::SP_8, bottom: spacing::SP_8, left: spacing::SP_18, right: spacing::SP_18 })
            .on_press(cancel_msg.clone());

        let card = container(
            column![
                container(prompt_text)
                    .width(Length::Fixed(260.0))
                    .align_x(Alignment::Center)
                    .padding(Padding { top: spacing::SP_4, bottom: spacing::SP_16, left: spacing::SP_4, right: spacing::SP_4 }),
                row![confirm_btn, cancel_btn].spacing(spacing::SP_10).align_y(Alignment::Center),
            ]
                .align_x(Alignment::Center)
                .spacing(spacing::SP_4),
        )
            .padding(spacing::SP_24)
            .style(container_style::context_menu);

        let backdrop = mouse_area(
            container(card)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::Center)
                .align_y(Alignment::Center)
                .style(|_| container::Style {
                    background: Some(theme().overlay.scrim_strong.into()),
                    ..Default::default()
                }),
        )
            .on_press(cancel_msg);

        Some(backdrop.into())
    }
}