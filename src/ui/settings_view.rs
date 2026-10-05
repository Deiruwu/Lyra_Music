//! Ajustes (se abre con la tuerca de la barra superior): color de acento, la
//! transición entre canciones y el servidor de música.

use iced::border::rounded;
use iced::widget::{column, container, row, scrollable, slider, space, text, text_input, toggler};
use iced::{Alignment, Element, Length, Padding, Theme};

use crate::local_server::{self, ServerState};
use crate::settings::{ServerMode, ServerSettings};
use crate::ui::accent_picker::{AccentMessage, AccentPicker};
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

#[derive(Debug, Clone)]
pub enum SettingsMessage {
    Accent(AccentMessage),
    CrossfadeToggled(bool),
    CrossfadeSecondsChanged(f32),
    RemoteServerToggled(bool),
    ServerHostChanged(String),
    ServerPortChanged(String),
}

pub struct SettingsView {
    accent: AccentPicker,
    crossfade_enabled: bool,
    /// Se recuerda aunque la transición esté apagada.
    crossfade_seconds: f32,
    server: ServerSettings,
    /// Texto del campo de puerto, que puede no ser un número válido mientras se escribe.
    port_input: String,
}

impl SettingsView {
    pub fn new(crossfade_enabled: bool, crossfade_seconds: f32, server: ServerSettings) -> Self {
        Self {
            accent: AccentPicker::default(),
            crossfade_enabled,
            crossfade_seconds: crossfade_seconds.clamp(CROSSFADE_MIN_SECONDS, CROSSFADE_MAX_SECONDS),
            port_input: server.port.to_string(),
            server,
        }
    }

    /// Al abrir: el selector toma el acento vigente.
    pub fn opened(&mut self) {
        self.accent.sync_with_theme();
    }

    /// Aplica el mensaje; si cambió la transición, devuelve su duración efectiva (0 = apagada).
    pub fn update(&mut self, message: SettingsMessage) -> Option<f32> {
        match message {
            SettingsMessage::Accent(message) => {
                self.accent.update(message);
                None
            }
            SettingsMessage::CrossfadeToggled(enabled) => {
                self.crossfade_enabled = enabled;
                Some(self.effective_crossfade())
            }
            SettingsMessage::CrossfadeSecondsChanged(seconds) => {
                self.crossfade_seconds = seconds;
                Some(self.effective_crossfade())
            }
            SettingsMessage::RemoteServerToggled(remote) => {
                self.server.mode = if remote { ServerMode::Remote } else { ServerMode::Local };
                None
            }
            SettingsMessage::ServerHostChanged(host) => {
                self.server.host = host.trim().to_string();
                None
            }
            SettingsMessage::ServerPortChanged(port) => {
                if let Ok(parsed) = port.parse() {
                    self.server.port = parsed;
                }
                self.port_input = port;
                None
            }
        }
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
