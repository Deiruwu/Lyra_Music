use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::animation::{Animation, Easing};
use iced::widget::{column, container, mouse_area, scrollable, text};
use iced::{Color, Element, Length, Padding, Task};

use crate::JETBRAINS_MONO;
use crate::model::audio_tech::PlayableTrack;
use super::lrc_parser::{parse_lrc, SyncedLyrics};

/// Cuánto se desplaza verticalmente (en px) la línea que deja de ser la
/// actual / la que pasa a serlo. Sutil a propósito — es un settle, no un
/// salto.
const LINE_SHIFT_PX: f32 = 10.0;

#[derive(Debug, Clone)]
pub enum LyricsMessage {
    /// Se detectó un cambio de track — dispara la carga async del .lrc.
    TrackChanged(Arc<PlayableTrack>),
    /// El archivo se leyó (o no existe / falló), resultado de la carga async.
    Loaded {
        /// Id del track al que corresponde esta carga. Necesario para
        /// descartar resultados "viejos" si el usuario cambia de track
        /// rápido y una carga lenta llega después de una más nueva.
        track_id: String,
        lyrics: Option<SyncedLyrics>,
    },
    /// Llamar en cada Tick del reproductor con la posición actual.
    PositionUpdated(Duration),
    /// Frame del compositor, solo se pide mientras hay una transición
    /// de línea en curso (igual que AnimationFrame en la cola).
    AnimationFrame(Instant),
    /// El usuario tocó una línea; lleva su timestamp para hacer seek.
    LineClicked(Duration),
}

/// Out-message hacia el padre. Solo existe el caso de seek; todo lo
/// demás lo resuelve el panel internamente.
#[derive(Debug, Clone)]
pub enum LyricsOutMessage {
    RequestSeek(Duration),
    Idle,
}

/// Estado explícito de las letras para el track actual. Reemplaza al
/// booleano `has_lyrics()` de antes: ahora la UI puede distinguir entre
/// "todavía no sabemos" (Loading), "buscamos y no hay" (NotFound), y
/// "no hay ninguna canción sonando" (NoTrack), en vez de tratarlos todos
/// como "false" indistinguible.
#[derive(Debug, Clone, PartialEq)]
pub enum LyricsStatus {
    /// No hay track actual, o no tiene file_path para derivar un .lrc.
    NoTrack,
    /// Se disparó la carga async y todavía no vuelve el resultado.
    Loading,
    /// Se buscó el .lrc y no existe, o existe pero vino vacío.
    NotFound,
    /// Letras cargadas y listas para sincronizar con `current_line`.
    Synced(SyncedLyrics),
}

/// Animación de "convertirse en la línea actual" (o dejar de serlo).
/// `weight` va de 0.0 (línea normal) a 1.0 (línea actual) y se usa para
/// interpolar tamaño, color y desplazamiento — un solo valor animado
/// en vez de tres por separado, así se mantienen sincronizados.
struct LineAnim {
    index: usize,
    weight: Animation<f32>,
}

pub struct LyricsPanel {
    status: LyricsStatus,
    /// Id del track al que pertenece el estado actual, para descartar
    /// cargas obsoletas si el usuario cambia de track rápido.
    current_track_id: Option<String>,
    /// Índice de la línea actual según la última posición conocida.
    current_line: Option<usize>,
    /// Animaciones de peso por línea, solo se guardan mientras están en
    /// transición o son la línea activa (evita cargar cientos de
    /// Animation ociosas en canciones largas).
    anims: Vec<LineAnim>,
}

impl Default for LyricsPanel {
    fn default() -> Self {
        Self {
            status: LyricsStatus::NoTrack,
            current_track_id: None,
            current_line: None,
            anims: Vec::new(),
        }
    }
}

impl LyricsPanel {
    pub fn update(&mut self, msg: LyricsMessage) -> (Task<LyricsMessage>, LyricsOutMessage) {
        match msg {
            LyricsMessage::TrackChanged(playable) => {
                let track_id = playable.track.id.clone();
                self.current_track_id = Some(track_id.clone());
                self.current_line = None;
                self.anims.clear();

                let Some(lrc_path) = lrc_path_for(&playable) else {
                    self.status = LyricsStatus::NotFound;
                    return (Task::none(), LyricsOutMessage::Idle);
                };

                self.status = LyricsStatus::Loading;

                let task = Task::perform(load_lrc(lrc_path), move |lyrics| LyricsMessage::Loaded {
                    track_id: track_id.clone(),
                    lyrics,
                });
                (task, LyricsOutMessage::Idle)
            }

            LyricsMessage::Loaded { track_id, lyrics } => {
                // Descarta si ya cambiamos de track mientras esto cargaba.
                if self.current_track_id.as_deref() == Some(track_id.as_str()) {
                    self.status = match lyrics {
                        Some(l) => LyricsStatus::Synced(l),
                        None => LyricsStatus::NotFound,
                    };
                }
                (Task::none(), LyricsOutMessage::Idle)
            }

            LyricsMessage::PositionUpdated(position) => {
                if let LyricsStatus::Synced(lyrics) = &self.status {
                    let new_line = lyrics.current_line_index(position);
                    if new_line != self.current_line {
                        self.current_line = new_line;
                        self.retarget_anims(now());
                    }
                }
                (Task::none(), LyricsOutMessage::Idle)
            }

            LyricsMessage::AnimationFrame(_) => {
                // Solo forzamos redraw; los valores se leen con interpolate
                // directo en view() usando Instant::now().
                (Task::none(), LyricsOutMessage::Idle)
            }

            LyricsMessage::LineClicked(timestamp) => {
                (Task::none(), LyricsOutMessage::RequestSeek(timestamp))
            }
        }
    }

    /// Ajusta el objetivo (0.0 o 1.0) de la animación de cada línea
    /// afectada por el cambio de `current_line`. Igual que
    /// `QueueAnimator::sync_target`: se crea la animación si no existe,
    /// se retarget-ea si ya existe.
    fn retarget_anims(&mut self, now: Instant) {
        let current = self.current_line;

        // Apaga la línea que dejó de ser actual (si tenía animación).
        for anim in self.anims.iter_mut() {
            if Some(anim.index) != current && anim.weight.value() > 0.0 {
                anim.weight.go_mut(0.0, now);
            }
        }

        if let Some(idx) = current {
            if let Some(anim) = self.anims.iter_mut().find(|a| a.index == idx) {
                anim.weight.go_mut(1.0, now);
            } else {
                self.anims.push(LineAnim {
                    index: idx,
                    weight: Animation::new(0.0).easing(Easing::EaseOut).slow().go(1.0, now),
                });
            }
        }

        // Poda animaciones que ya llegaron a 0.0 y no son la actual —
        // no queremos acumular una por cada línea que ya pasó.
        self.anims.retain(|a| Some(a.index) == current || a.weight.value() > 0.001);
    }

    fn weight_of(&self, index: usize, now: Instant) -> f32 {
        self.anims
            .iter()
            .find(|a| a.index == index)
            .map(|a| a.weight.interpolate_with(|w| w, now))
            .unwrap_or(if Some(index) == self.current_line { 1.0 } else { 0.0 })
    }

    /// True mientras alguna línea sigue en transición — el padre debe
    /// suscribirse a `iced::window::frames()` mientras esto sea true,
    /// igual que ya hacen con `queue.is_animating()`.
    pub fn is_animating(&self, now: Instant) -> bool {
        self.anims.iter().any(|a| a.weight.is_animating(now))
    }

    /// True si hay letras cargadas y sincronizadas para el track actual.
    pub fn has_lyrics(&self) -> bool {
        matches!(self.status, LyricsStatus::Synced(_))
    }

    pub fn status(&self) -> &LyricsStatus {
        &self.status
    }

    /// Panel grande para el espacio central: lista completa de líneas
    /// con scroll, resaltando la línea actual con una transición de
    /// fade + deslizamiento vertical, y clicables para saltar a ese
    /// punto de la canción. Cubre los 4 estados posibles con un mensaje
    /// claro cuando no hay letra sincronizable.
    pub fn view(&self) -> Element<'_, LyricsMessage> {
        let content: Element<'_, LyricsMessage> = match &self.status {
            LyricsStatus::NoTrack => status_message("Sin reproducción activa"),

            LyricsStatus::Loading => status_message("Buscando letra..."),

            LyricsStatus::NotFound => status_message("Letra no disponible"),

            LyricsStatus::Synced(lyrics) => {
                let now = now();
                let mut lines_col = column![].spacing(14).width(Length::Fill);

                for (i, line) in lyrics.lines.iter().enumerate() {
                    let weight = self.weight_of(i, now);

                    // Interpolación continua entre "línea normal" y
                    // "línea actual" a partir de un solo valor (weight),
                    // en vez de un salto binario de tamaño/color.
                    let size = 16.0 + (22.0 - 16.0) * weight;
                    let gray = 0.5 - 0.05 * weight;
                    let color = Color::from_rgb(
                        gray + (1.0 - gray) * weight,
                        gray + (1.0 - gray) * weight,
                        (gray + 0.05) + (1.0 - (gray + 0.05)) * weight,
                    );
                    // Se desliza hacia arriba conforme se vuelve la línea
                    // actual (weight -> 1.0), como el highlight de Spotify.
                    let offset_y = LINE_SHIFT_PX * (1.0 - weight);

                    let line_text = text(line.text.clone())
                        .font(JETBRAINS_MONO)
                        .size(size)
                        .color(color);

                    let row = container(line_text)
                        .width(Length::Fill)
                        .padding(Padding::new(0.0).top(offset_y))
                        .align_x(iced::alignment::Horizontal::Center);

                    let clickable = mouse_area(row)
                        .on_press(LyricsMessage::LineClicked(line.timestamp))
                        .interaction(iced::mouse::Interaction::Pointer);

                    lines_col = lines_col.push(clickable);
                }

                scrollable(
                    container(lines_col)
                        .width(Length::Fill)
                        .padding(Padding::new(0.0).top(24.0).bottom(24.0)),
                )
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into()
            }
        };

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(iced::alignment::Horizontal::Center)
            .align_y(iced::alignment::Vertical::Center)
            .into()
    }
}

fn now() -> Instant {
    Instant::now()
}

fn status_message(msg: &str) -> Element<'_, LyricsMessage> {
    text(msg.to_string())
        .font(JETBRAINS_MONO)
        .size(15)
        .color(Color::from_rgb(0.45, 0.45, 0.5))
        .into()
}

/// Deriva la ruta esperada del .lrc a partir del file_path del track
/// actual (mismo nombre, extensión distinta — igual que las escribe
/// el lyrics_downloader).
fn lrc_path_for(playable: &PlayableTrack) -> Option<PathBuf> {
    let file_path = playable.track.file_path.as_ref()?;
    Some(Path::new(file_path).with_extension("lrc"))
}

/// Lee y parsea el .lrc en un hilo async. Si el archivo no existe o
/// falla la lectura, devuelve None en vez de propagar el error — la
/// ausencia de letras es un caso normal, no un fallo.
async fn load_lrc(path: PathBuf) -> Option<SyncedLyrics> {
    let content = tokio::fs::read_to_string(&path).await.ok()?;
    let parsed = parse_lrc(&content);

    if parsed.is_empty() {
        None
    } else {
        Some(parsed)
    }
}