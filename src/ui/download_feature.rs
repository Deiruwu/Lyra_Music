//! Escucha `subscribe_downloads` del microservicio y expone el estado de las
//! descargas activas como una pila de "píldoras" flotantes, compuesta por
//! `App::view` justo encima de la barra de reproducción.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use futures::SinkExt;
use iced::{stream, Alignment, Element, Length, Subscription, Task};
use iced::widget::column;

use crate::microservices::client::MicroserviceClient;
use crate::model::{DownloadEvent, Track};
use crate::ui::assets::spacing;
use crate::ui::utils::thumbnail_cache::ThumbnailCache;
use crate::ui::widgets::download_pill::{download_pill, DownloadPillEntry, PillPhase};

const HOLD_AFTER_TERMINAL_MS: u64 = 1400;
/// Hold extra del pop-up de análisis: "un pelín más grande, que dure
/// 0.5-1s más" que una píldora terminal normal.
const SUCCESS_EXTRA_HOLD_MS: u64 = 750;
const RECONNECT_DELAY_SECS: u64 = 2;

/// Sufijo de la `key` del pop-up de análisis, para que conviva en la misma
/// lista que la píldora de descarga (que usa el id de track tal cual) sin
/// pisarla si todavía no terminó de desvanecerse.
const ANALYZED_KEY_SUFFIX: &str = "::analyzed";
const LYRICS_KEY_SUFFIX: &str = "::lyrics";
const METADATA_KEY_SUFFIX: &str = "::metadata";
const PLAYLIST_ADD_KEY_SUFFIX: &str = "::playlist";

/// Las píldoras no pertenecen a ninguna vista paginada — el epoch de
/// `ThumbnailCache` no aplica acá, se pasa constante.
const EPOCH: u64 = 0;

static DOWNLOAD_CLIENT: OnceLock<Arc<MicroserviceClient>> = OnceLock::new();

#[derive(Debug, Clone)]
pub enum DownloadFeatureMessage {
    Event(DownloadEvent),
    Dismiss(String),
    ThumbnailLoaded { id: String, bytes: Vec<u8>, epoch: u64 },
}

/// Mensaje de salida hacia `App`: la única pieza de estado que vive fuera de
/// este feature es el `Track` completo de `analyzefinished`, que necesita
/// llegar a `CatalogStore` para refrescar el bpm/camelot_key en memoria.
pub enum DownloadFeatureOutMessage {
    Idle,
    TrackReady(Track),
    /// Se guardó una letra nueva para este track (id).
    LyricsUpdated(String),
}

pub struct DownloadFeature {
    active: Vec<DownloadPillEntry>,
}

impl DownloadFeature {
    pub fn new(client: Arc<MicroserviceClient>) -> Self {
        DOWNLOAD_CLIENT.set(client).ok();
        Self { active: Vec::new() }
    }

    pub fn update(
        &mut self,
        message: DownloadFeatureMessage,
        thumbnails: &mut ThumbnailCache,
    ) -> (Task<DownloadFeatureMessage>, DownloadFeatureOutMessage) {
        match message {
            DownloadFeatureMessage::Event(DownloadEvent::Requested { id, title, thumbnail_small }) => {
                let fetch_task = Self::request_thumbnail(thumbnails, &id, &thumbnail_small);
                self.upsert(id, title, thumbnail_small, PillPhase::Requested);
                (fetch_task, DownloadFeatureOutMessage::Idle)
            }

            DownloadFeatureMessage::Event(DownloadEvent::Downloading {
                id, title, thumbnail_small, downloaded_bytes, total_bytes, speed_bytes_per_sec, ..
            }) => {
                let fetch_task = Self::request_thumbnail(thumbnails, &id, &thumbnail_small);

                match self.active.iter_mut().find(|e| e.key == id) {
                    Some(entry) => {
                        entry.title = title;
                        if thumbnail_small.is_some() {
                            entry.thumbnail_url = thumbnail_small;
                        }
                        entry.downloaded_bytes = downloaded_bytes;
                        entry.total_bytes = total_bytes;
                        entry.speed_bytes_per_sec = speed_bytes_per_sec;
                        entry.phase = PillPhase::Downloading;
                    }
                    None => self.active.push(DownloadPillEntry {
                        key: id.clone(),
                        id,
                        title,
                        thumbnail_url: thumbnail_small,
                        downloaded_bytes,
                        total_bytes,
                        speed_bytes_per_sec,
                        bpm: None,
                        phase: PillPhase::Downloading,
                    }),
                }

                (fetch_task, DownloadFeatureOutMessage::Idle)
            }

            DownloadFeatureMessage::Event(DownloadEvent::Finished { id, title, thumbnail_small }) => {
                let fetch_task = Self::request_thumbnail(thumbnails, &id, &thumbnail_small);
                self.upsert(id.clone(), title, thumbnail_small, PillPhase::Finished);
                let dismiss_task = self.schedule_dismiss(id, HOLD_AFTER_TERMINAL_MS);
                (Task::batch(vec![fetch_task, dismiss_task]), DownloadFeatureOutMessage::Idle)
            }

            // El usuario no quiere ver "empezó a analizar" — solo el
            // resultado (`AnalyzeFinished`, más abajo). Se ignora sin tocar
            // la píldora de descarga, que ya está en `Finished` esperando su
            // propio dismiss.
            DownloadFeatureMessage::Event(DownloadEvent::AnalyzeStarted { .. }) => {
                (Task::none(), DownloadFeatureOutMessage::Idle)
            }

            DownloadFeatureMessage::Event(DownloadEvent::Failed { id, title, thumbnail_small, message }) => {
                eprintln!("[DOWNLOAD] {} ({}) falló: {}", id, title, message);
                let fetch_task = Self::request_thumbnail(thumbnails, &id, &thumbnail_small);
                self.upsert(id.clone(), title, thumbnail_small, PillPhase::Failed);
                let dismiss_task = self.schedule_dismiss(id, HOLD_AFTER_TERMINAL_MS);
                (Task::batch(vec![fetch_task, dismiss_task]), DownloadFeatureOutMessage::Idle)
            }

            // El análisis es "otro pop-up": no reemplaza la píldora de
            // descarga (que ya se desvanece por su cuenta desde `Finished`),
            // se inserta como una entry nueva con una `key` propia.
            DownloadFeatureMessage::Event(DownloadEvent::AnalyzeFinished { track }) => {
                let task = self.push_notice(
                    thumbnails,
                    NoticeTrack::from(&track),
                    ANALYZED_KEY_SUFFIX,
                    PillPhase::Analyzed,
                    HOLD_AFTER_TERMINAL_MS + SUCCESS_EXTRA_HOLD_MS,
                );
                (task, DownloadFeatureOutMessage::TrackReady(track))
            }

            DownloadFeatureMessage::Event(DownloadEvent::AnalyzeFailed { id, title, thumbnail_small, message }) => {
                eprintln!("[DOWNLOAD] Análisis de {} ({}) falló: {}", id, title, message);
                let task = self.push_notice(
                    thumbnails,
                    notice_track(id, title, thumbnail_small),
                    ANALYZED_KEY_SUFFIX,
                    PillPhase::AnalyzeFailed,
                    HOLD_AFTER_TERMINAL_MS,
                );
                (task, DownloadFeatureOutMessage::Idle)
            }

            DownloadFeatureMessage::Event(DownloadEvent::LyricsFound { id, title, thumbnail_small }) => {
                let task = self.push_notice(
                    thumbnails,
                    notice_track(id.clone(), title, thumbnail_small),
                    LYRICS_KEY_SUFFIX,
                    PillPhase::LyricsFound,
                    HOLD_AFTER_TERMINAL_MS + SUCCESS_EXTRA_HOLD_MS,
                );
                (task, DownloadFeatureOutMessage::LyricsUpdated(id))
            }

            DownloadFeatureMessage::Event(DownloadEvent::LyricsNotFound { id, title, thumbnail_small }) => {
                let task = self.push_notice(
                    thumbnails,
                    notice_track(id, title, thumbnail_small),
                    LYRICS_KEY_SUFFIX,
                    PillPhase::LyricsNotFound,
                    HOLD_AFTER_TERMINAL_MS,
                );
                (task, DownloadFeatureOutMessage::Idle)
            }

            DownloadFeatureMessage::Event(DownloadEvent::MetadataUpdated { track }) => {
                let task = self.push_notice(
                    thumbnails,
                    NoticeTrack::from(&track),
                    METADATA_KEY_SUFFIX,
                    PillPhase::MetadataUpdated,
                    HOLD_AFTER_TERMINAL_MS + SUCCESS_EXTRA_HOLD_MS,
                );
                (task, DownloadFeatureOutMessage::TrackReady(track))
            }

            DownloadFeatureMessage::Event(DownloadEvent::MetadataFailed { id, title, thumbnail_small, message }) => {
                eprintln!("[DOWNLOAD] Metadatos de {} ({}) fallaron: {}", id, title, message);
                let task = self.push_notice(
                    thumbnails,
                    notice_track(id, title, thumbnail_small),
                    METADATA_KEY_SUFFIX,
                    PillPhase::MetadataFailed,
                    HOLD_AFTER_TERMINAL_MS,
                );
                (task, DownloadFeatureOutMessage::Idle)
            }

            DownloadFeatureMessage::Dismiss(key) => {
                self.active.retain(|e| e.key != key);
                (Task::none(), DownloadFeatureOutMessage::Idle)
            }

            DownloadFeatureMessage::ThumbnailLoaded { id, bytes, .. } => {
                thumbnails.insert_color(id.clone(), bytes);
                let next = thumbnails.on_color_finished(&id, |id, bytes, epoch| {
                    DownloadFeatureMessage::ThumbnailLoaded { id, bytes, epoch }
                });
                (next, DownloadFeatureOutMessage::Idle)
            }
        }
    }

    /// Pide la miniatura a color apenas se conoce la URL — ya desde
    /// `requested`, para que esté lista (o casi) para cuando arranque el
    /// progreso real y no se vea "en blanco" al inicio de la descarga.
    /// Idempotente: `ThumbnailCache`/`DownloadQueue` ignoran el pedido si la
    /// key ya está en caché o en vuelo, así que es seguro llamarlo en cada
    /// evento.
    fn request_thumbnail(
        thumbnails: &mut ThumbnailCache,
        id: &str,
        thumbnail_small: &Option<String>,
    ) -> Task<DownloadFeatureMessage> {
        thumbnail_small
            .as_ref()
            .and_then(|url| {
                thumbnails.request_color(id.to_string(), url.clone(), EPOCH, |id, bytes, epoch| {
                    DownloadFeatureMessage::ThumbnailLoaded { id, bytes, epoch }
                })
            })
            .unwrap_or(Task::none())
    }

    /// Letrero de canciones agregadas a una playlist: cuántas entraron y cuántas ya estaban.
    pub fn notify_playlist_add(
        &mut self,
        thumbnails: &mut ThumbnailCache,
        playlist_name: &str,
        added: usize,
        already: usize,
        sample: &Track,
    ) -> Task<DownloadFeatureMessage> {
        let summary = playlist_add_summary(added, already);
        self.push_notice(
            thumbnails,
            notice_track(sample.id.clone(), playlist_name.to_string(), sample.thumbnail_small.clone()),
            PLAYLIST_ADD_KEY_SUFFIX,
            PillPhase::PlaylistAdd { summary, added_any: added > 0 },
            HOLD_AFTER_TERMINAL_MS + SUCCESS_EXTRA_HOLD_MS,
        )
    }

    /// Inserta o actualiza la píldora de descarga (key == id de track) por
    /// id, preservando el progreso si ya existía (las fases post-descarga
    /// no traen esos campos).
    fn upsert(&mut self, id: String, title: String, thumbnail_url: Option<String>, phase: PillPhase) {
        match self.active.iter_mut().find(|e| e.key == id) {
            Some(entry) => {
                entry.title = title;
                if thumbnail_url.is_some() {
                    entry.thumbnail_url = thumbnail_url;
                }
                entry.phase = phase;
            }
            None => self.active.push(DownloadPillEntry {
                key: id.clone(),
                id,
                title,
                thumbnail_url,
                downloaded_bytes: None,
                total_bytes: None,
                speed_bytes_per_sec: None,
                bpm: None,
                phase,
            }),
        }
    }

    /// Pop-up aparte (análisis, letra, metadatos) con su propia `key`, que
    /// reemplaza a uno anterior del mismo tipo y se desvanece tras `hold_ms`.
    fn push_notice(
        &mut self,
        thumbnails: &mut ThumbnailCache,
        track: NoticeTrack,
        key_suffix: &str,
        phase: PillPhase,
        hold_ms: u64,
    ) -> Task<DownloadFeatureMessage> {
        let fetch_task = Self::request_thumbnail(thumbnails, &track.id, &track.thumbnail_small);

        let key = format!("{}{}", track.id, key_suffix);
        self.active.retain(|e| e.key != key);
        self.active.push(DownloadPillEntry {
            key: key.clone(),
            id: track.id,
            title: track.title,
            thumbnail_url: track.thumbnail_small,
            downloaded_bytes: None,
            total_bytes: None,
            speed_bytes_per_sec: None,
            bpm: track.bpm,
            phase,
        });

        Task::batch(vec![fetch_task, self.schedule_dismiss(key, hold_ms)])
    }

    fn schedule_dismiss(&self, key: String, hold_ms: u64) -> Task<DownloadFeatureMessage> {
        Task::perform(
            tokio::time::sleep(Duration::from_millis(hold_ms)),
            move |_| DownloadFeatureMessage::Dismiss(key.clone()),
        )
    }

    pub fn subscription(&self) -> Subscription<DownloadFeatureMessage> {
        Subscription::run(download_events)
    }

    pub fn view<'a>(&'a self, thumbnails: &'a ThumbnailCache) -> Element<'a, DownloadFeatureMessage> {
        if self.active.is_empty() {
            return column![].into();
        }

        let pills = self.active.iter().map(|entry| download_pill(entry, thumbnails));

        column(pills)
            .spacing(spacing::SP_10)
            .align_x(Alignment::Center)
            .width(Length::Fill)
            .into()
    }
}

/// Lo mínimo de un track que necesita un pop-up.
struct NoticeTrack {
    id: String,
    title: String,
    thumbnail_small: Option<String>,
    bpm: Option<i32>,
}

impl From<&Track> for NoticeTrack {
    fn from(track: &Track) -> Self {
        Self {
            id: track.id.clone(),
            title: track.title.clone(),
            thumbnail_small: track.thumbnail_small.clone(),
            bpm: track.bpm,
        }
    }
}

fn notice_track(id: String, title: String, thumbnail_small: Option<String>) -> NoticeTrack {
    NoticeTrack { id, title, thumbnail_small, bpm: None }
}

fn download_events() -> impl futures::Stream<Item = DownloadFeatureMessage> {
    let client = DOWNLOAD_CLIENT.get().unwrap().clone();

    stream::channel(100, async move |mut output| {
        loop {
            match client.subscribe_downloads().await {
                Ok(mut rx) => {
                    while let Some(event) = rx.recv().await {
                        let _ = output.send(DownloadFeatureMessage::Event(event)).await;
                    }
                }
                Err(e) => {
                    eprintln!("[DOWNLOAD-EVENTS] No se pudo suscribir: {}", e);
                }
            }

            tokio::time::sleep(Duration::from_secs(RECONNECT_DELAY_SECS)).await;
        }
    })
}

/// "2 canciones agregadas", "Ya estaba en la playlist", "3 agregadas · 1 ya estaba"...
fn playlist_add_summary(added: usize, already: usize) -> String {
    match (added, already) {
        (1, 0) => "Canción agregada".to_string(),
        (n, 0) => format!("{n} canciones agregadas"),
        (0, 1) => "Ya estaba en la playlist".to_string(),
        (0, n) => format!("Ya estaban las {n} en la playlist"),
        (n, 1) => format!("{n} agregadas · 1 ya estaba"),
        (n, m) => format!("{n} agregadas · {m} ya estaban"),
    }
}

#[cfg(test)]
mod tests {
    use super::playlist_add_summary;

    #[test]
    fn el_letrero_dice_que_entro_y_que_ya_estaba() {
        assert_eq!(playlist_add_summary(1, 0), "Canción agregada");
        assert_eq!(playlist_add_summary(4, 0), "4 canciones agregadas");
        assert_eq!(playlist_add_summary(0, 1), "Ya estaba en la playlist");
        assert_eq!(playlist_add_summary(0, 3), "Ya estaban las 3 en la playlist");
        assert_eq!(playlist_add_summary(2, 1), "2 agregadas · 1 ya estaba");
        assert_eq!(playlist_add_summary(2, 5), "2 agregadas · 5 ya estaban");
    }
}
