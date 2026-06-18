use std::num::NonZeroUsize;
use iced::Task;
use iced::widget::image::Handle;
use lru::LruCache;

pub struct ThumbnailCache {
    cache: LruCache<String, Handle>,
}

impl ThumbnailCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            cache: LruCache::new(NonZeroUsize::new(capacity).unwrap()),
        }
    }

    pub fn get(&mut self, id: &str) -> Option<Handle> {
        self.cache.get(id).cloned()
    }

    pub fn peek(&self, id: &str) -> Option<Handle> {
        self.cache.peek(id).cloned()
    }

    pub fn insert(&mut self, id: String, bytes: Vec<u8>) {
        if !bytes.is_empty() {
            self.cache.put(id, Handle::from_bytes(bytes));
        }
    }

    pub fn insert_grayscale(&mut self, id: String, bytes: Vec<u8>) {
        if bytes.is_empty() {
            return;
        }

        match image::load_from_memory(&bytes) {
            Ok(img) => {
                let gray = img.grayscale();

                let rgba = gray.into_rgba8();
                let width = rgba.width();
                let height = rgba.height();
                let raw_pixels = rgba.into_raw();

                self.cache.put(id, Handle::from_rgba(width, height, raw_pixels));
            }
            Err(e) => {
                println!("Fallo al decodificar imagen {} para filtro: {}", id, e);
                self.cache.put(id, Handle::from_bytes(bytes));
            }
        }
    }

    pub fn request_download<Message: 'static + Send>(
        &mut self,
        id: String,
        url: String,
        to_message: impl Fn(String, Vec<u8>) -> Message + Send + Sync + 'static,
    ) -> Option<Task<Message>> {

        if self.cache.contains(&id) {
            return None;
        }

        Some(Task::perform(
            crate::ui::utils::image::download_thumbnail(url),
            move |result| match result {
                Ok(bytes) => to_message(id.clone(), bytes),
                Err(e) => {
                    println!("Error descargando {}: {}", id, e);
                    to_message(id.clone(), vec![])
                }
            }
        ))
    }
}