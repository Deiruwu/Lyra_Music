//! Ajustes (se abre con el avatar de la barra superior): perfil, color de
//! acento, la transición entre canciones y el servidor de música.

use std::path::PathBuf;

use iced::border::rounded;
use iced::widget::image::Handle;
use iced::widget::{button, column, container, row, scrollable, slider, space, text, text_input, toggler};
use iced::{Alignment, Element, Length, Padding, Task, Theme};

use crate::local_server::{self, ServerState};
use crate::settings::{ServerMode, ServerSettings};
use crate::ui::accent_picker::{AccentMessage, AccentPicker};
use crate::ui::profile;
use crate::ui::styles::button as button_style;
use crate::ui::utils::cover_picker::pick_image;
use crate::ui::utils::image::load_crop_preview;
use crate::ui::widgets::cover_crop_editor::{CoverCropEditor, CropEditorMessage, CropEditorOutcome};
use crate::ui::assets::fonts::SF_PRO;
use crate::ui::assets::{radii, spacing, typography};
use crate::ui::styles::scrollable as scrollable_style;
use crate::ui::styles::slider as slider_style;
use crate::ui::styles::text_input as text_input_style;
use crate::ui::theme::theme;

const CROSSFADE_MIN_SECONDS: f32 = 1.0;
const CROSSFADE_MAX_SECONDS: f32 = 12.0;
const CONTENT_MAX_WIDTH: f32 = 760.0;
const SLIDER_WIDTH: f32 = 260.0;
const DESCRIPTION_WIDTH: f32 = 360.0;
const SERVER_DETAIL_HEIGHT: f32 = 34.0;
const PORT_INPUT_WIDTH: f32 = 90.0;
const PROFILE_AVATAR_SIZE: f32 = 88.0;
const NAME_INPUT_WIDTH: f32 = 260.0;
const CROP_PREVIEW_MAX_SIDE: u32 = 1024;

#[derive(Debug, Clone)]
pub enum SettingsMessage {
    Accent(AccentMessage),
    CrossfadeToggled(bool),
    CrossfadeSecondsChanged(f32),
    RemoteServerToggled(bool),
    ServerHostChanged(String),
    ServerPortChanged(String),
    ProfileNameChanged(String),
    PickPhoto,
    PhotoPicked(Option<PathBuf>),
    PhotoPreviewLoaded(PathBuf, Result<(Vec<u8>, u32, u32), String>),
    PhotoCrop(CropEditorMessage),
    PhotoSaved(Result<Vec<u8>, String>),
    RemovePhoto,
}

pub struct SettingsView {
    accent: AccentPicker,
    crossfade_enabled: bool,
    /// Se recuerda aunque la transición esté apagada.
    crossfade_seconds: f32,
    server: ServerSettings,
    /// Texto del campo de puerto, que puede no ser un número válido mientras se escribe.
    port_input: String,
    profile_name: String,
    photo: Option<Handle>,
    photo_crop: Option<CoverCropEditor>,
}

impl SettingsView {
    pub fn new(crossfade_enabled: bool, crossfade_seconds: f32, server: ServerSettings, profile_name: Option<String>) -> Self {
        Self {
            accent: AccentPicker::default(),
            crossfade_enabled,
            crossfade_seconds: crossfade_seconds.clamp(CROSSFADE_MIN_SECONDS, CROSSFADE_MAX_SECONDS),
            port_input: server.port.to_string(),
            server,
            profile_name: profile_name.unwrap_or_default(),
            photo: profile::load_photo(),
            photo_crop: None,
        }
    }

    /// Al abrir: el selector toma el acento vigente.
    pub fn opened(&mut self) {
        self.accent.sync_with_theme();
    }

    /// Aplica el mensaje; si cambió la transición, devuelve también su duración efectiva (0 = apagada).
    pub fn update(&mut self, message: SettingsMessage) -> (Task<SettingsMessage>, Option<f32>) {
        match message {
            SettingsMessage::CrossfadeToggled(enabled) => {
                self.crossfade_enabled = enabled;
                (Task::none(), Some(self.effective_crossfade()))
            }
            SettingsMessage::CrossfadeSecondsChanged(seconds) => {
                self.crossfade_seconds = seconds;
                (Task::none(), Some(self.effective_crossfade()))
            }
            other => (self.update_other(other), None),
        }
    }

    fn update_other(&mut self, message: SettingsMessage) -> Task<SettingsMessage> {
        match message {
            SettingsMessage::Accent(message) => {
                self.accent.update(message);
            }
            SettingsMessage::CrossfadeToggled(_) | SettingsMessage::CrossfadeSecondsChanged(_) => {}
            SettingsMessage::ProfileNameChanged(name) => self.profile_name = name,
            SettingsMessage::PickPhoto => {
                return Task::perform(pick_image("Elegir foto de perfil"), SettingsMessage::PhotoPicked);
            }
            SettingsMessage::PhotoPicked(Some(path)) => {
                return Task::perform(
                    {
                        let path = path.clone();
                        async move {
                            tokio::task::spawn_blocking(move || load_crop_preview(&path, CROP_PREVIEW_MAX_SIDE))
                                .await
                                .unwrap_or_else(|e| Err(e.to_string()))
                        }
                    },
                    move |result| SettingsMessage::PhotoPreviewLoaded(path.clone(), result),
                );
            }
            SettingsMessage::PhotoPicked(None) => {}
            SettingsMessage::PhotoPreviewLoaded(path, Ok((preview, width, height))) => {
                self.photo_crop = Some(
                    CoverCropEditor::new("profile".to_string(), path, preview, width, height).with_title("Recortar foto de perfil"),
                );
            }
            SettingsMessage::PhotoPreviewLoaded(path, Err(e)) => {
                eprintln!("No se pudo abrir la foto {}: {e}", path.display());
            }
            SettingsMessage::PhotoCrop(message) => {
                let Some(editor) = &mut self.photo_crop else { return Task::none() };
                match editor.update(message) {
                    CropEditorOutcome::Editing => {}
                    CropEditorOutcome::Cancel => self.photo_crop = None,
                    CropEditorOutcome::Save { source_path, region, .. } => {
                        self.photo_crop = None;
                        return Task::perform(
                            async move {
                                tokio::task::spawn_blocking(move || profile::save_photo(&source_path, region))
                                    .await
                                    .unwrap_or_else(|e| Err(e.to_string()))
                            },
                            SettingsMessage::PhotoSaved,
                        );
                    }
                }
            }
            SettingsMessage::PhotoSaved(Ok(bytes)) => self.photo = Some(Handle::from_bytes(bytes)),
            SettingsMessage::PhotoSaved(Err(e)) => eprintln!("No se pudo guardar la foto de perfil: {e}"),
            SettingsMessage::RemovePhoto => match profile::remove_photo() {
                Ok(()) => self.photo = None,
                Err(e) => eprintln!("No se pudo quitar la foto de perfil: {e}"),
            },
            SettingsMessage::RemoteServerToggled(remote) => {
                self.server.mode = if remote { ServerMode::Remote } else { ServerMode::Local };
            }
            SettingsMessage::ServerHostChanged(host) => {
                self.server.host = host.trim().to_string();
            }
            SettingsMessage::ServerPortChanged(port) => {
                if let Ok(parsed) = port.parse() {
                    self.server.port = parsed;
                }
                self.port_input = port;
            }
        }
        Task::none()
    }

    /// Nombre de perfil (`None` si está vacío).
    pub fn profile_name(&self) -> Option<String> {
        let name = self.profile_name.trim();
        (!name.is_empty()).then(|| name.to_string())
    }

    /// Avatar de la barra superior.
    pub fn avatar<'a, Message: 'a>(&self, size: f32) -> Element<'a, Message> {
        profile::avatar(self.photo.as_ref(), &self.profile_name, size)
    }

    /// Editor de recorte de la foto, si está abierto.
    pub fn view_photo_crop(&self) -> Option<Element<'_, SettingsMessage>> {
        self.photo_crop.as_ref().map(|editor| editor.view().map(SettingsMessage::PhotoCrop))
    }

    /// Cierra el editor de recorte; `true` si estaba abierto.
    pub fn cancel_photo_crop(&mut self) -> bool {
        self.photo_crop.take().is_some()
    }

    pub fn server(&self) -> &ServerSettings {
        &self.server
    }

    pub fn crossfade_enabled(&self) -> bool {
        self.crossfade_enabled
    }

    pub fn crossfade_seconds(&self) -> f32 {
        self.crossfade_seconds
    }

    /// Duración que usa el motor (0 si está apagada).
    pub fn effective_crossfade(&self) -> f32 {
        if self.crossfade_enabled { self.crossfade_seconds } else { 0.0 }
    }

    pub fn view(&self) -> Element<'_, SettingsMessage> {
        let profile = section("Perfil", self.view_profile());

        let appearance = section(
            "Apariencia",
            row![
                setting_label(
                    "Color de acento",
                    "El tono de botones, selección y resaltados. Los demás tonos de acento se derivan de él.",
                ),
                space().width(Length::Fill),
                self.accent.view().map(SettingsMessage::Accent),
            ]
                .align_y(Alignment::Start)
                .into(),
        );

        let crossfade_switch = toggler(self.crossfade_enabled)
            .on_toggle(SettingsMessage::CrossfadeToggled)
            .size(22.0)
            .style(switch_style);
        let duration_color = if self.crossfade_enabled { theme().content.primary } else { theme().content.muted };
        let duration = row![
            text("Duración").font(SF_PRO).size(typography::TEXT_13).color(duration_color),
            space().width(Length::Fill),
            slider(CROSSFADE_MIN_SECONDS..=CROSSFADE_MAX_SECONDS, self.crossfade_seconds, SettingsMessage::CrossfadeSecondsChanged)
                .step(1.0)
                .width(Length::Fixed(SLIDER_WIDTH))
                .style(slider_style::track),
            text(format!("{:.0} s", self.crossfade_seconds))
                .font(SF_PRO)
                .size(typography::TEXT_13)
                .color(duration_color)
                .width(Length::Fixed(40.0))
                .align_x(Alignment::End),
        ]
            .spacing(spacing::SP_12)
            .align_y(Alignment::Center);

        let playback = section(
            "Reproducción",
            column![
                row![
                    setting_label(
                        "Transición entre canciones",
                        "Funde el final de cada canción con el inicio de la siguiente de la cola.",
                    ),
                    space().width(Length::Fill),
                    crossfade_switch,
                ]
                    .align_y(Alignment::Center),
                duration,
            ]
                .spacing(spacing::SP_16)
                .into(),
        );

        let remote = self.server.mode == ServerMode::Remote;
        let server = section(
            "Servidor",
            column![
                row![
                    setting_label(
                        "Servidor remoto",
                        "Conecta con un track_manager ya desplegado en vez de iniciar uno local. Se aplica al reiniciar atelier.",
                    ),
                    space().width(Length::Fill),
                    toggler(remote)
                        .on_toggle(SettingsMessage::RemoteServerToggled)
                        .size(22.0)
                        .style(switch_style),
                ]
                    .align_y(Alignment::Center),
                container(if remote { self.view_remote_address() } else { view_local_status() })
                    .height(Length::Fixed(SERVER_DETAIL_HEIGHT))
                    .align_y(Alignment::Center),
            ]
                .spacing(spacing::SP_16)
                .into(),
        );

        let content = column![
            text("Ajustes").font(SF_PRO).size(typography::TEXT_28).color(theme().content.primary),
            profile,
            appearance,
            playback,
            server,
        ]
            .spacing(spacing::SP_24)
            .max_width(CONTENT_MAX_WIDTH);

        scrollable(container(content).width(Length::Fill).padding(Padding { top: spacing::SP_8, bottom: spacing::SP_32, ..Padding::ZERO }))
            .height(Length::Fill)
            .style(scrollable_style::discreet)
            .into()
    }
}

impl SettingsView {
    /// Foto grande, nombre y botones para cambiar o quitar la foto.
    fn view_profile(&self) -> Element<'_, SettingsMessage> {
        let action = |label: &'static str, message| {
            button(text(label).font(SF_PRO).size(typography::TEXT_13).color(theme().content.primary))
                .style(button_style::context_menu_item)
                .padding(Padding { top: spacing::SP_6, bottom: spacing::SP_6, left: spacing::SP_12, right: spacing::SP_12 })
                .on_press(message)
        };
        let change_label = if self.photo.is_some() { "Cambiar foto" } else { "Elegir foto" };
        let mut actions = row![action(change_label, SettingsMessage::PickPhoto)].spacing(spacing::SP_8);
        if self.photo.is_some() {
            actions = actions.push(action("Quitar foto", SettingsMessage::RemovePhoto));
        }

        let name = text_input("Tu nombre", &self.profile_name)
            .on_input(SettingsMessage::ProfileNameChanged)
            .font(SF_PRO)
            .size(typography::TEXT_16)
            .padding(Padding { top: spacing::SP_6, bottom: spacing::SP_6, left: spacing::SP_10, right: spacing::SP_10 })
            .style(text_input_style::field)
            .width(Length::Fixed(NAME_INPUT_WIDTH));

        row![
            profile::avatar(self.photo.as_ref(), &self.profile_name, PROFILE_AVATAR_SIZE),
            column![name, actions].spacing(spacing::SP_12),
        ]
            .spacing(spacing::SP_20)
            .align_y(Alignment::Center)
            .into()
    }

    /// Campos de host y puerto del servidor remoto.
    fn view_remote_address(&self) -> Element<'_, SettingsMessage> {
        row![
            text_input("192.168.1.10", &self.server.host)
                .on_input(SettingsMessage::ServerHostChanged)
                .font(SF_PRO)
                .size(typography::TEXT_13)
                .padding(Padding { top: spacing::SP_6, bottom: spacing::SP_6, left: spacing::SP_10, right: spacing::SP_10 })
                .style(text_input_style::field)
                .width(Length::Fill),
            text_input("7878", &self.port_input)
                .on_input(SettingsMessage::ServerPortChanged)
                .font(SF_PRO)
                .size(typography::TEXT_13)
                .padding(Padding { top: spacing::SP_6, bottom: spacing::SP_6, left: spacing::SP_10, right: spacing::SP_10 })
                .style(text_input_style::field)
                .width(Length::Fixed(PORT_INPUT_WIDTH)),
        ]
            .spacing(spacing::SP_10)
            .align_y(Alignment::Center)
            .into()
    }
}

/// Estado del track_manager local: instalación y arranque.
fn view_local_status<'a>() -> Element<'a, SettingsMessage> {
    let (message, color) = match local_server::state() {
        ServerState::Starting => ("Iniciando servidor local…".to_string(), theme().content.muted),
        ServerState::Failed(reason) => (reason, theme().status.error),
        ServerState::Ready if local_server::is_installed() => (
            format!("Servidor local activo · {}", local_server::server_root().display()),
            theme().content.secondary,
        ),
        ServerState::Ready => (
            "Servidor local no instalado: corre scripts/setup-local-server.sh".to_string(),
            theme().content.muted,
        ),
    };
    text(message).font(SF_PRO).size(typography::TEXT_12).color(color).into()
}

/// Título de sección y su tarjeta.
fn section<'a>(title: &'a str, body: Element<'a, SettingsMessage>) -> Element<'a, SettingsMessage> {
    column![
        text(title).font(SF_PRO).size(typography::TEXT_13).color(theme().content.muted),
        container(body).width(Length::Fill).padding(spacing::SP_20).style(|_theme: &Theme| container::Style {
            background: Some(theme().surface.sunken.into()),
            border: rounded(radii::R_12),
            ..Default::default()
        }),
    ]
        .spacing(spacing::SP_8)
        .into()
}

/// Nombre del ajuste y su explicación.
fn setting_label<'a>(name: &'a str, description: &'a str) -> Element<'a, SettingsMessage> {
    column![
        text(name).font(SF_PRO).size(typography::TEXT_14).color(theme().content.primary),
        text(description).font(SF_PRO).size(typography::TEXT_12).color(theme().content.muted),
    ]
        .spacing(spacing::SP_4)
        .width(Length::Fixed(DESCRIPTION_WIDTH))
        .into()
}

/// Interruptor con el acento cuando está encendido.
fn switch_style(_theme: &Theme, status: toggler::Status) -> toggler::Style {
    let (is_on, hovered) = match status {
        toggler::Status::Active { is_toggled } => (is_toggled, false),
        toggler::Status::Hovered { is_toggled } => (is_toggled, true),
        toggler::Status::Disabled { is_toggled } => (is_toggled, false),
    };
    let background = match (is_on, hovered) {
        (true, false) => theme().accent.primary,
        (true, true) => theme().accent.hover,
        (false, false) => theme().surface.control,
        (false, true) => theme().overlay.control_hover,
    };
    toggler::Style {
        background: background.into(),
        background_border_width: 0.0,
        background_border_color: iced::Color::TRANSPARENT,
        foreground: if is_on { theme().content.on_accent } else { theme().content.secondary }.into(),
        foreground_border_width: 0.0,
        foreground_border_color: iced::Color::TRANSPARENT,
        text_color: None,
        border_radius: None,
        padding_ratio: 0.12,
    }
}
