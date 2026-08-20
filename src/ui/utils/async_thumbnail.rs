use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use iced::widget::image::Handle;
use iced::Task;
use image::{ImageFormat, ImageReader};
use tokio::sync::Semaphore;

use crate::model::Track;

/// Genera la clave única para la miniatura de un track.
/// Deduplica usando el ID del álbum si existe, o el ID del track como fallback.
pub fn thumb_key(track: &Track) -> String {
    track.album
        .as_ref()
        .map(|a| a.id.clone())
        .unwrap_or_else(|| track.id.clone())
}

/// Guarda una bandera atómica compartida con el hilo asíncrono.
/// Al destruirse (Drop), cambia la bandera a `true` para abortar la tarea de red o CPU.
#[derive(Debug)]
pub(crate) struct DropGuard(Arc<AtomicBool>);

impl DropGuard {
    /// Crea una nueva guardia y retorna su instancia junto con el puntero atómico compartido.
    pub fn new() -> (Self, Arc<AtomicBool>) {
        let flag = Arc::new(AtomicBool::new(false));
        (Self(flag.clone()), flag)
    }
}

impl Drop for DropGuard {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// Estado interno de una miniatura en el ciclo de vida de carga.
enum Slot {
    Loading { _guard: DropGuard },
    Ready(Handle),
}

/// Gestor de miniaturas atado al ciclo de vida de la vista.
/// Funciona como la única fuente de verdad para evitar fugas de memoria y condiciones de carrera.
#[derive(Default)]
pub struct AsyncThumbnail {
    slots: HashMap<String, Slot>,
}

impl AsyncThumbnail {
    /// Inicializa un gestor vacío.
    pub fn new() -> Self {
        Self { slots: HashMap::new() }
    }

    /// Recibe las claves y URLs requeridas actualmente por la vista y una función constructora de mensajes.
    /// Retiene en RAM solo las claves solicitadas (abortando las demás automáticamente vía Drop).
    /// Despacha tareas de descarga únicamente para las claves faltantes.
    pub fn sync<Msg: 'static + Send>(
        &mut self,
        wanted: &[(String, String)],
        to_message: impl Fn(String, Vec<u8>) -> Msg + Send + Sync + 'static + Clone,
    ) -> Task<Msg> {
        let wanted_keys: HashSet<&str> = wanted.iter().map(|(k, _)| k.as_str()).collect();

        self.slots.retain(|k, _| wanted_keys.contains(k.as_str()));

        let mut tasks = Vec::new();
        for (key, url) in wanted {
            if self.slots.contains_key(key) {
                continue;
            }

            let (guard, abort_flag) = DropGuard::new();
            self.slots.insert(key.clone(), Slot::Loading { _guard: guard });

            let key = key.clone();
            let url = url.clone();
            let to_message = to_message.clone();

            tasks.push(Task::perform(download_with_abort(url, abort_flag), move |bytes| {
                to_message(key.clone(), bytes)
            }));
        }

        Task::batch(tasks)
    }

    /// Registra los bytes decodificados y los convierte en un `Handle` de Iced.
    /// Si la clave ya fue eliminada del mapa (usuario hizo scroll), descarta los bytes.
    pub fn on_loaded(&mut self, key: String, bytes: Vec<u8>) {
        if bytes.is_empty() || !self.slots.contains_key(&key) {
            return;
        }
        self.slots.insert(key, Slot::Ready(Handle::from_bytes(bytes)));
    }

    /// Obtiene el handle de la imagen si la descarga y decodificación ya terminaron.
    pub fn get(&self, key: &str) -> Option<&Handle> {
        match self.slots.get(key)? {
            Slot::Ready(h) => Some(h),
            Slot::Loading { .. } => None,
        }
    }
}

/// Limita la concurrencia global de descargas a 15 hilos simultáneos para no saturar sockets TCP.
pub(crate) fn download_limiter() -> &'static Semaphore {
    static LIMITER: OnceLock<Semaphore> = OnceLock::new();
    LIMITER.get_or_init(|| Semaphore::new(15))
}

/// Realiza la descarga HTTP, decodificación y recorte de la imagen a un formato cuadrado.
/// Revisa la bandera atómica antes del I/O de red y antes del procesamiento de CPU para permitir early-return.
async fn download_with_abort(url: String, aborted: Arc<AtomicBool>) -> Vec<u8> {
    let Ok(_permit) = download_limiter().acquire().await else { return Vec::new() };

    if aborted.load(Ordering::Relaxed) { return Vec::new(); }

    let Ok(resp) = reqwest::get(&url).await else { return Vec::new() };
    let Ok(bytes) = resp.bytes().await else { return Vec::new() };

    if aborted.load(Ordering::Relaxed) { return Vec::new(); }

    let Ok(reader) = ImageReader::new(Cursor::new(&bytes)).with_guessed_format() else {
        return Vec::new();
    };

    let Ok(img) = reader.decode() else {
        return Vec::new();
    };

    let (w, h) = (img.width(), img.height());
    let size = w.min(h);
    let x = (w - size) / 2;
    let y = (h - size) / 2;
    let cropped = img.crop_imm(x, y, size, size);

    let mut out = Vec::new();
    if cropped.write_to(&mut Cursor::new(&mut out), ImageFormat::Jpeg).is_err() {
        return Vec::new();
    }

    out
}