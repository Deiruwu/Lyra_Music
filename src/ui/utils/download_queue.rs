//! Cola de descargas LIFO con concurrencia limitada, timeout real y
//! cancelación por "epoch" (generación).
//!
//! ## Por qué existe
//!
//! Antes, cada `request_color`/`request_gray` disparaba un
//! `Task::perform(download_thumbnail(url), ...)` de forma independiente:
//! sin límite de concurrencia, sin timeout de cliente (creaba un
//! `reqwest::Client` nuevo por llamada), y sin forma de cancelar. Bajo
//! scroll rápido esto satura conexiones → los errores
//! "error sending request for url ..." que veías.
//!
//! ## Modelo
//!
//! - `Vec` usado como pila (push/pop): LIFO real. Lo último pedido
//!   (lo que el usuario ve *ahora*) se descarga primero.
//! - Máximo `MAX_CONCURRENT_DOWNLOADS` workers activos a la vez.
//! - Cada item lleva un `epoch: u64`. Tu `update()` compara el epoch
//!   recibido contra el epoch actual antes de escribir al caché — si no
//!   coincide, lo ignoras (igual que ya haces con `page_generation`).
//! - La red pasa por `image_fetch` (cliente compartido con timeouts y reintentos).
//!
//! ## Cómo se integra (importante, léelo antes de usar)
//!
//! iced no permite "encadenar" lógica de limpieza de estado compartido
//! dentro de un `Task` de forma transparente sin pasar por tu propio
//! `update()` — y no debería, porque el estado de la cola (quién está
//! activo, qué sigue) tiene que vivir donde tu app pueda inspeccionarlo.
//! Por eso el ciclo de vida es:
//!
//! 1. Llamas `queue.enqueue(key, url, epoch)` → te da `Option<Task<Message>>`
//!    si hay que empezar a trabajar (lo mezclas a tu `Task::batch`).
//! 2. Cuando ese `Task` resuelve, tu `update()` recibe tu mensaje
//!    (p. ej. `ThumbnailLoaded(key, bytes, epoch)`).
//! 3. Dentro de tu `update()`, llamas `queue.on_finished()` **una vez**
//!    por cada resultado recibido — esto libera el slot de concurrencia
//!    y te devuelve el siguiente `Task` a encadenar (si hay algo más en
//!    la pila). Lo mezclas también a tu `Task::batch`.
//!
//! Esto es explícito a propósito: nada mágico escondido en callbacks,
//! todo el estado vive en `DownloadQueue` y tú decides cuándo avanzar.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use iced::Task;

use crate::ui::utils::image::crop_and_encode_cover;
use crate::ui::utils::image_fetch::fetch_image_bytes;

/// Lado máximo al que se reescala antes de cachear. Los dos consumidores de
/// esta cola pintan chico — filas de resultados de búsqueda (~55px) y pills
/// de descarga (32px) — pero las fuentes llegan a resolución completa. Sin
/// este tope, `ThumbnailCache::insert_gray` guardaba RGBA crudo de 1080²
/// (4.6 MB por entrada) para pintarlo a 55px.
const THUMBNAIL_MAX_SIDE: u32 = 128;

/// Cuántas descargas pueden estar en vuelo simultáneamente.
const MAX_CONCURRENT_DOWNLOADS: usize = 5;

#[derive(Clone)]
struct QueuedItem {
    key: String,
    url: String,
    epoch: u64,
}

struct QueueState {
    /// Pila LIFO: el último `push` es el primero en salir (`pop`).
    stack: Vec<QueuedItem>,
    /// Keys actualmente en la pila O siendo descargadas ahora mismo.
    /// Evita encolar la misma key dos veces mientras está pendiente.
    in_flight_or_queued: HashSet<String>,
    /// Subconjunto de `in_flight_or_queued`: keys que YA tienen un
    /// worker corriendo (ya salieron del stack vía `spawn_next`). Estas
    /// no se pueden cancelar — hay que esperar a que `on_finished` las
    /// libere. Todo lo que esté en `in_flight_or_queued` pero NO aquí
    /// sigue en el stack sin empezar, y por lo tanto SÍ se puede podar
    /// sin costo (ver `drop_outside_visible`).
    downloading: HashSet<String>,
    active_workers: usize,
}

#[derive(Clone)]
pub struct DownloadQueue {
    state: Arc<Mutex<QueueState>>,
}

impl DownloadQueue {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(QueueState {
                stack: Vec::new(),
                in_flight_or_queued: HashSet::new(),
                downloading: HashSet::new(),
                active_workers: 0,
            })),
        }
    }

    /// Encola una descarga (LIFO) si la key no está ya encolada/en vuelo.
    /// `epoch` debe ser el epoch de tu vista en el momento del pedido
    /// (tú lo generas y lo llevas, este módulo no lo inventa: así puedes
    /// tener un epoch por Explorer, otro por cada playlist abierta, etc.,
    /// sin acoplar este módulo a tu enum de vistas).
    ///
    /// Devuelve `Some(Task)` si esto disparó un worker nuevo. Puede
    /// devolver `None` si la key ya estaba encolada/descargándose, o si
    /// ya se llegó al máximo de concurrencia (el item queda igual
    /// apilado, esperando a que `on_finished` libere un slot).
    pub fn enqueue<Message: 'static + Send>(
        &self,
        key: String,
        url: String,
        epoch: u64,
        to_message: impl Fn(String, Vec<u8>, u64) -> Message + Send + Sync + 'static,
    ) -> Option<Task<Message>> {
        {
            let mut state = self.state.lock().unwrap();
            if state.in_flight_or_queued.contains(&key) {
                return None;
            }
            state.in_flight_or_queued.insert(key.clone());
            state.stack.push(QueuedItem { key, url, epoch });
        }

        self.spawn_next(to_message)
    }

    /// Llama esto **desde tu `update()`**, exactamente una vez por cada
    /// resultado de descarga recibido (sea éxito o error), pasando la
    /// key que acaba de terminar. Libera el slot de concurrencia y, si
    /// quedan items en la pila, arranca el siguiente worker.
    ///
    /// Mezcla el `Task` resultante a tu `Task::batch` de esa rama del
    /// `update`.
    pub fn on_finished<Message: 'static + Send>(
        &self,
        finished_key: &str,
        to_message: impl Fn(String, Vec<u8>, u64) -> Message + Send + Sync + 'static,
    ) -> Task<Message> {
        {
            let mut state = self.state.lock().unwrap();
            state.active_workers = state.active_workers.saturating_sub(1);
            state.in_flight_or_queued.remove(finished_key);
            state.downloading.remove(finished_key);
        }

        self.spawn_next(to_message).unwrap_or(Task::none())
    }

    /// Intenta tomar el siguiente item de la pila (el más reciente) y
    /// arrancar un worker para él, respetando el límite de concurrencia.
    fn spawn_next<Message: 'static + Send>(
        &self,
        to_message: impl Fn(String, Vec<u8>, u64) -> Message + Send + Sync + 'static,
    ) -> Option<Task<Message>> {
        let item = {
            let mut state = self.state.lock().unwrap();
            if state.active_workers >= MAX_CONCURRENT_DOWNLOADS {
                return None;
            }
            let item = state.stack.pop()?;
            state.active_workers += 1;
            state.downloading.insert(item.key.clone());
            item
        };

        Some(Task::perform(
            download_bytes(item.url),
            move |result| match result {
                Ok(bytes) => to_message(item.key.clone(), bytes, item.epoch),
                Err(_) => to_message(item.key.clone(), Vec::new(), item.epoch),
            },
        ))
    }


}

impl Default for DownloadQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// Descarga (con reintentos) y recorta a cuadrado con el mismo tope que el resto de miniaturas.
async fn download_bytes(url: String) -> Result<Vec<u8>, String> {
    let bytes = fetch_image_bytes(&url, None).await?;
    crop_and_encode_cover(&bytes, THUMBNAIL_MAX_SIDE).ok_or_else(|| format!("imagen inválida: {url}"))
}
