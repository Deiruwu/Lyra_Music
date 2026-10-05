use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use iced::widget::image::Handle;
use iced::Task;

use crate::model::Track;
use crate::ui::utils::image::crop_and_encode_cover;
use crate::ui::utils::image_fetch::fetch_image_bytes;

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
    /// Lado máximo al que se reescala antes de cachear. Las imágenes de
    /// origen vienen a resolución completa (hasta 1080px o más) y se
    /// pintaban a 40-156px: cachearlas sin reescalar desperdiciaba RAM y
    /// además, sobre ~724px, iced le da a cada imagen su propia textura
    /// privada en vez del atlas compartido.
    max_side: u32,
}

impl AsyncThumbnail {
    /// Inicializa un gestor vacío con `max_side` en el doble del tamaño de
    /// pintado del consumidor (margen para pantallas HiDPI).
    pub fn new(max_side: u32) -> Self {
        Self { slots: HashMap::new(), max_side }
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

            tasks.push(Task::perform(download_with_abort(url, abort_flag, self.max_side), move |bytes| {
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

/// Descarga (con reintentos), recorta a cuadrado y reescala a `max_side`.
/// Revisa la bandera de aborto antes del procesamiento de CPU para permitir early-return.
async fn download_with_abort(url: String, aborted: Arc<AtomicBool>, max_side: u32) -> Vec<u8> {
    let Ok(bytes) = fetch_image_bytes(&url, Some(&aborted)).await else { return Vec::new() };
    if aborted.load(Ordering::Relaxed) { return Vec::new(); }
    crop_and_encode_cover(&bytes, max_side).unwrap_or_default()
}
