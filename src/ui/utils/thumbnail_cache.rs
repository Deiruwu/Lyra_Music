use std::collections::HashSet;
use std::num::NonZeroUsize;
use iced::Task;
use iced::widget::image::Handle;
use lru::LruCache;
use crate::model::{Track, TrackState};

// ── Clave album-first para el caché de color ──────────────────────────────────

pub fn thumb_key(track: &Track) -> String {
    track.album
        .as_ref()
        .map(|a| a.id.clone())
        .unwrap_or_else(|| track.id.clone())
}

// ── Caché ─────────────────────────────────────────────────────────────────────

pub struct ThumbnailCache {
    /// Thumbnails a color. Clave: album_id si existe, track_id si no.
    /// Usado por cola y reproducción.
    color: LruCache<String, Handle>,

    /// Thumbnails en escala de grises. Clave: track_id siempre.
    /// Usado por búsqueda cuando state == Partial.
    gray: LruCache<String, Handle>,

    /// IDs con descarga de color en vuelo (evita Tasks duplicados).
    pending_color: HashSet<String>,

    /// IDs con descarga de gris en vuelo.
    pending_gray: HashSet<String>,
}

impl ThumbnailCache {
    pub fn new(color_capacity: usize, gray_capacity: usize) -> Self {
        Self {
            color: LruCache::new(NonZeroUsize::new(color_capacity).unwrap()),
            gray:  LruCache::new(NonZeroUsize::new(gray_capacity).unwrap()),
            pending_color: HashSet::new(),
            pending_gray:  HashSet::new(),
        }
    }

    // ── Lectura ───────────────────────────────────────────────────────────────

    /// Para renderizar un track_row: decide qué Handle entregar según el estado
    /// de la canción.
    ///
    /// - `Cached`  → busca en color; si no existe, cae al gris como fallback.
    /// - `Partial` → busca en gris directo.
    ///
    /// Devuelve `None` si no hay nada → `async_thumbnail` muestra el placeholder.
    pub fn peek_for_render(&self, track: &Track) -> Option<Handle> {
        match track.state {
            TrackState::Cached => self
                .color
                .peek(&thumb_key(track))
                .or_else(|| self.gray.peek(&track.id))
                .cloned(),

            TrackState::Partial => self.gray.peek(&track.id).cloned(),
        }
    }

    /// Lectura directa del caché de color (para el player / current track).
    pub fn peek_color(&self, key: &str) -> Option<Handle> {
        self.color.peek(key).cloned()
    }

    // ── Escritura ─────────────────────────────────────────────────────────────

    /// Inserta un thumbnail a color (cola, reproducción, DownloadFinished).
    /// Usa `thumb_key(track)` como clave en el call site.
    pub fn insert_color(&mut self, key: String, bytes: Vec<u8>) {
        self.pending_color.remove(&key);
        if !bytes.is_empty() {
            self.color.put(key, Handle::from_bytes(bytes));
        }
    }

    /// Inserta un thumbnail en escala de grises (búsqueda con state == Partial).
    /// Clave: track.id siempre.
    pub fn insert_gray(&mut self, track_id: String, bytes: Vec<u8>) {
        self.pending_gray.remove(&track_id);
        if bytes.is_empty() {
            return;
        }

        match image::load_from_memory(&bytes) {
            Ok(img) => {
                let rgba = img.grayscale().into_rgba8();
                let (width, height) = (rgba.width(), rgba.height());
                let raw = rgba.into_raw();
                self.gray.put(track_id, Handle::from_rgba(width, height, raw));
            }
            Err(e) => {
                println!("[ThumbnailCache] Fallo decodificando gris {}: {}", track_id, e);
                self.gray.put(track_id, Handle::from_bytes(bytes));
            }
        }
    }

    // ── Descarga ──────────────────────────────────────────────────────────────

    /// Solicita descarga a color si la clave no está ya en caché ni en vuelo.
    /// `key` debe ser el resultado de `thumb_key(track)`.
    pub fn request_color<Message: 'static + Send>(
        &mut self,
        key: String,
        url: String,
        to_message: impl Fn(String, Vec<u8>) -> Message + Send + Sync + 'static,
    ) -> Option<Task<Message>> {
        if self.color.contains(&key) || self.pending_color.contains(&key) {
            return None;
        }

        self.pending_color.insert(key.clone());

        Some(Task::perform(
            crate::ui::utils::image::download_thumbnail(url),
            move |result| match result {
                Ok(bytes) => to_message(key.clone(), bytes),
                Err(e) => {
                    println!("[ThumbnailCache] Error descargando color {}: {}", key, e);
                    to_message(key.clone(), vec![])
                }
            },
        ))
    }

    /// Solicita descarga en gris si el track_id no está ya en caché ni en vuelo.
    pub fn request_gray<Message: 'static + Send>(
        &mut self,
        track_id: String,
        url: String,
        to_message: impl Fn(String, Vec<u8>) -> Message + Send + Sync + 'static,
    ) -> Option<Task<Message>> {
        if self.gray.contains(&track_id) || self.pending_gray.contains(&track_id) {
            return None;
        }

        self.pending_gray.insert(track_id.clone());

        Some(Task::perform(
            crate::ui::utils::image::download_thumbnail(url),
            move |result| match result {
                Ok(bytes) => to_message(track_id.clone(), bytes),
                Err(e) => {
                    println!("[ThumbnailCache] Error descargando gris {}: {}", track_id, e);
                    to_message(track_id.clone(), vec![])
                }
            },
        ))
    }
}