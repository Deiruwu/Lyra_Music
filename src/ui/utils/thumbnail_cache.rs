use std::collections::HashSet;
use std::num::NonZeroUsize;
use iced::Task;
use iced::widget::image::Handle;
use lru::LruCache;
use crate::model::{Track, TrackState};
use crate::ui::utils::download_queue::DownloadQueue;

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

    /// Cola LIFO compartida para color y gris. LIFO porque bajo scroll
    /// brusco lo último pedido (lo que el usuario ve ahora) debe
    /// descargarse antes que lo pedido hace 2 segundos y ya fuera de
    /// pantalla. Ver `download_queue.rs` para el porqué completo.
    color_queue: DownloadQueue,
    gray_queue: DownloadQueue,
}

impl ThumbnailCache {
    pub fn new(color_capacity: usize, gray_capacity: usize) -> Self {
        Self {
            color: LruCache::new(NonZeroUsize::new(color_capacity).unwrap()),
            gray:  LruCache::new(NonZeroUsize::new(gray_capacity).unwrap()),
            color_queue: DownloadQueue::new(),
            gray_queue: DownloadQueue::new(),
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
        if !bytes.is_empty() {
            self.color.put(key, Handle::from_bytes(bytes));
        }
    }

    /// Inserta un thumbnail en escala de grises (búsqueda con state == Partial).
    /// Clave: track.id siempre.
    pub fn insert_gray(&mut self, track_id: String, bytes: Vec<u8>) {
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
    /// `epoch` es tu generación de vista actual (p. ej. `page_generation`);
    /// se te devolverá tal cual en `to_message` para que puedas descartar
    /// resultados obsoletos en tu `update()`.
    pub fn request_color<Message: 'static + Send>(
        &mut self,
        key: String,
        url: String,
        epoch: u64,
        to_message: impl Fn(String, Vec<u8>, u64) -> Message + Send + Sync + 'static,
    ) -> Option<Task<Message>> {

        if self.color.contains(&key) {
            return None;
        }
        self.color_queue.enqueue(key, url, epoch, to_message)
    }

    /// Solicita descarga en gris si el track_id no está ya en caché ni en vuelo.
    pub fn request_gray<Message: 'static + Send>(
        &mut self,
        track_id: String,
        url: String,
        epoch: u64,
        to_message: impl Fn(String, Vec<u8>, u64) -> Message + Send + Sync + 'static,
    ) -> Option<Task<Message>> {

        if self.gray.contains(&track_id) {
            return None;
        }
        self.gray_queue.enqueue(track_id, url, epoch, to_message)
    }

    /// Llama esto en tu `update()` al recibir el resultado de una
    /// descarga a color (éxito o error), pasando la key que terminó.
    /// Libera el slot de concurrencia y arranca la siguiente descarga
    /// pendiente en la pila, si hay alguna.
    pub fn on_color_finished<Message: 'static + Send>(
        &self,
        finished_key: &str,
        to_message: impl Fn(String, Vec<u8>, u64) -> Message + Send + Sync + 'static,
    ) -> Task<Message> {
        self.color_queue.on_finished(finished_key, to_message)
    }

    /// Igual que `on_color_finished` pero para la cola de gris.
    pub fn on_gray_finished<Message: 'static + Send>(
        &self,
        finished_key: &str,
        to_message: impl Fn(String, Vec<u8>, u64) -> Message + Send + Sync + 'static,
    ) -> Task<Message> {
        self.gray_queue.on_finished(finished_key, to_message)
    }

    /// Descarta de ambas colas (color y gris) todo lo que no pertenezca
    /// al epoch/rango de epochs aún válido. Llama esto cuando cambias de
    /// página, de vista, o detectas scroll brusco.
    pub fn drop_stale(&self, is_still_valid: impl Fn(u64) -> bool + Copy) {
        self.color_queue.drop_stale(is_still_valid);
        self.gray_queue.drop_stale(is_still_valid);
    }

    /// Poda ambas colas (color y gris) dejando solo lo que esté en
    /// `still_wanted_color` / `still_wanted_gray` respectivamente,
    /// SIN tocar epoch. Pensado para llamarse en cada `Scrolled`: el
    /// universo de tracks no cambió, solo la ventana visible, así que
    /// no corresponde invalidar por epoch — pero sí hay que liberar las
    /// keys que quedaron enterradas en el stack fuera de la ventana
    /// (ver docstring de `DownloadQueue::drop_outside_visible`).
    ///
    /// Puedes pasar el mismo `HashSet` de keys para color y gris si tu
    /// vista usa la misma clave para ambos; si no, calcula cada uno por
    /// separado según qué pediste con `request_color`/`request_gray`.
    pub fn drop_outside_visible(
        &self,
        still_wanted_color: &HashSet<String>,
        still_wanted_gray: &HashSet<String>,
    ) {
        self.color_queue.drop_outside_visible(still_wanted_color);
        self.gray_queue.drop_outside_visible(still_wanted_gray);
    }
}