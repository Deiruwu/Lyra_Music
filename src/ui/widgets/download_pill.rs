//! Píldora flotante que muestra una descarga en curso (o su desenlace).
//!
//! Dos "kinds" de píldora comparten el mismo widget: la de progreso de
//! descarga (`Requested → Downloading → Finished/Failed`) y los pop-ups
//! aparte (análisis, letra, metadatos) — ver `DownloadPillEntry::key` en
//! `download_feature.rs` para cómo conviven sin pisarse en la misma lista.

use iced::widget::{column, container, progress_bar, row};
use iced::{Alignment, Color, Length, Padding};

use crate::ui::assets::fonts::SF_PRO;
use crate::ui::assets::{radii, spacing, typography};
use crate::ui::theme::theme;
use crate::ui::utils::thumbnail_cache::ThumbnailCache;
use crate::ui::widgets::async_thumbnail::{async_thumbnail, ThumbnailState};
use crate::ui::widgets::single_line_text::single_line_text;

pub const PILL_HEIGHT: f32 = 46.0;
const PILL_WIDTH: f32 = 280.0;

/// Crecimiento del pill del pop-up de análisis: "un pelín más grande".
const SUCCESS_GROWTH: f32 = 6.0;

const THUMB_SIZE: f32 = 32.0;
const THUMB_RADIUS: f32 = radii::R_6;

/// Cuánto del color de estado se mezcla sobre `surface.panel` para el fondo
/// de las píldoras terminales (ni tenue como un borde ni un verde/rojo
/// sólido y plano).
const STATUS_TINT_AMOUNT: f32 = 0.4;

#[derive(Debug, Clone, PartialEq)]
pub enum PillPhase {
    /// yt-dlp arrancó, sin progreso todavía.
    Requested,
    /// Progreso real de descarga.
    Downloading,
    /// Audio bajado y persistido en DB (todavía sin bpm/key).
    Finished,
    /// Análisis de BPM/key terminado — el pop-up aparte.
    Analyzed,
    /// yt-dlp o el insert a DB fallaron. El mensaje real solo se loggea por
    /// terminal (ver `DownloadFeature::update`); acá nunca se muestra.
    Failed,
    /// El análisis de BPM/key falló.
    AnalyzeFailed,
    /// Se guardó una letra nueva.
    LyricsFound,
    /// LRCLIB no tuvo letra para el track.
    LyricsNotFound,
    /// Metadatos reescritos desde YT Music.
    MetadataUpdated,
    /// No se pudieron actualizar los metadatos.
    MetadataFailed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DownloadPillEntry {
    /// Clave de la entry en la lista (única): para la píldora de descarga es
    /// el id del track; para el pop-up de análisis lleva un sufijo, así
    /// ambas pueden coexistir un instante sin que una pise a la otra.
    pub key: String,
    /// Id real del track — el que hay que usar para mirar la miniatura en
    /// `ThumbnailCache` (esa sí está cacheada por id de track, sin sufijo).
    pub id: String,
    pub title: String,
    pub thumbnail_url: Option<String>,
    pub downloaded_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
    pub speed_bytes_per_sec: Option<f64>,
    /// Solo se llena en la fase `Analyzed`, para mostrarlo en el pop-up.
    pub bpm: Option<i32>,
    pub phase: PillPhase,
}

fn format_speed(bytes_per_sec: f64) -> String {
    if bytes_per_sec >= 1024.0 * 1024.0 {
        format!("{:.1} MB/s", bytes_per_sec / (1024.0 * 1024.0))
    } else {
        format!("{:.0} KB/s", bytes_per_sec / 1024.0)
    }
}

/// Mezcla `accent` sobre `base` en proporción `amount` (0.0 = `base` puro,
/// 1.0 = `accent` puro). Da un fondo opaco teñido en vez de un borde tenue o
/// un color de estado plano que no combina con el resto de la paleta.
fn tint(base: Color, accent: Color, amount: f32) -> Color {
    Color::from_rgb(
        base.r + (accent.r - base.r) * amount,
        base.g + (accent.g - base.g) * amount,
        base.b + (accent.b - base.b) * amount,
    )
}

pub fn download_pill<'a, Message: Clone + 'a>(
    entry: &DownloadPillEntry,
    thumbnails: &ThumbnailCache,
) -> iced::Element<'a, Message> {
    let t = theme();

    let thumb_state = match thumbnails.peek_color(&entry.id) {
        Some(handle) => ThumbnailState::Loaded(handle),
        None => ThumbnailState::Loading,
    };
    let thumb_view = async_thumbnail(thumb_state, THUMB_SIZE, THUMB_RADIUS);

    let subtitle = match &entry.phase {
        PillPhase::Requested => "En cola…".to_string(),
        PillPhase::Downloading => match (entry.downloaded_bytes, entry.total_bytes) {
            (Some(down), Some(tot)) if tot > 0 => {
                let pct = (down as f64 / tot as f64 * 100.0).clamp(0.0, 100.0);
                match entry.speed_bytes_per_sec {
                    Some(speed) => format!("{:.0}% · {}", pct, format_speed(speed)),
                    None => format!("{:.0}%", pct),
                }
            }
            _ => match entry.speed_bytes_per_sec {
                Some(speed) => format!("Descargando · {}", format_speed(speed)),
                None => "Descargando…".to_string(),
            },
        },
        PillPhase::Finished => "Completado".to_string(),
        PillPhase::Analyzed => match entry.bpm {
            Some(bpm) => format!("Análisis listo · {} BPM", bpm),
            None => "Análisis listo".to_string(),
        },
        PillPhase::Failed => "No se pudo descargar".to_string(),
        PillPhase::AnalyzeFailed => "No se pudo analizar".to_string(),
        PillPhase::LyricsFound => "Letra encontrada".to_string(),
        PillPhase::LyricsNotFound => "No se encontró letra".to_string(),
        PillPhase::MetadataUpdated => "Metadatos actualizados".to_string(),
        PillPhase::MetadataFailed => "Falló al actualizar metadatos".to_string(),
    };

    let progress: Option<f32> = match (&entry.phase, entry.downloaded_bytes, entry.total_bytes) {
        (PillPhase::Downloading, Some(down), Some(tot)) if tot > 0 => {
            Some((down as f64 / tot as f64).clamp(0.0, 1.0) as f32)
        }
        _ => None,
    };

    let text_width = Length::Fixed(PILL_WIDTH - spacing::SP_40 - spacing::SP_20 - THUMB_SIZE);

    let title_view = single_line_text(entry.title.clone(), SF_PRO, typography::TEXT_13, t.content.primary, text_width);
    let subtitle_view = single_line_text(subtitle, SF_PRO, typography::TEXT_12, t.content.muted, text_width);

    let mut text_column = column![title_view, subtitle_view].spacing(spacing::SP_2);

    if let Some(fraction) = progress {
        text_column = text_column.push(
            progress_bar(0.0..=1.0, fraction)
                .girth(Length::Fixed(3.0))
                .style(move |_theme| progress_bar::Style {
                    background: theme().overlay.resting.into(),
                    bar: theme().accent.primary.into(),
                    border: iced::border::rounded(1.5),
                }),
        );
    }

    let content = row![thumb_view, text_column]
        .spacing(spacing::SP_10)
        .align_y(Alignment::Center)
        .width(Length::Fill);

    let (width, height) = match entry.phase {
        PillPhase::Analyzed | PillPhase::LyricsFound | PillPhase::MetadataUpdated => {
            (PILL_WIDTH + SUCCESS_GROWTH, PILL_HEIGHT + SUCCESS_GROWTH)
        }
        _ => (PILL_WIDTH, PILL_HEIGHT),
    };

    let background = match entry.phase {
        PillPhase::Finished | PillPhase::Analyzed | PillPhase::LyricsFound | PillPhase::MetadataUpdated => {
            tint(t.surface.panel, t.status.cached, STATUS_TINT_AMOUNT)
        }
        PillPhase::Failed | PillPhase::AnalyzeFailed | PillPhase::MetadataFailed => {
            tint(t.surface.panel, t.status.error, STATUS_TINT_AMOUNT)
        }
        PillPhase::Requested | PillPhase::Downloading | PillPhase::LyricsNotFound => t.surface.panel,
    };

    container(content)
        .width(Length::Fixed(width))
        .height(Length::Fixed(height))
        .padding(Padding {
            top: spacing::SP_0,
            right: spacing::SP_16,
            bottom: spacing::SP_0,
            left: spacing::SP_16,
        })
        .align_y(Alignment::Center)
        .style(move |_theme| container::Style {
            background: Some(background.into()),
            border: iced::border::rounded(height / 2.0),
            shadow: theme().elevation.shadow,
            ..Default::default()
        })
        .into()
}
