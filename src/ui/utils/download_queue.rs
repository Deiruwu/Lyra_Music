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
//! - Un solo `reqwest::Client` compartido, con timeout real.
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
use std::time::Duration;

use iced::Task;

/// Cuántas descargas pueden estar en vuelo simultáneamente.
const MAX_CONCURRENT_DOWNLOADS: usize = 5;

const DOWNLOAD_TIMEOUT_SECS: u64 = 8;
const CONNECT_TIMEOUT_SECS: u64 = 4;

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
    client: Arc<reqwest::Client>,
}

impl DownloadQueue {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(DOWNLOAD_TIMEOUT_SECS))
            .connect_timeout(Duration::from_secs(CONNECT_TIMEOUT_SECS))
            .pool_max_idle_per_host(MAX_CONCURRENT_DOWNLOADS)
            .build()
            .expect("no se pudo construir el cliente HTTP de thumbnails");

        Self {
            state: Arc::new(Mutex::new(QueueState {
                stack: Vec::new(),
                in_flight_or_queued: HashSet::new(),
                downloading: HashSet::new(),
                active_workers: 0,
            })),
            client: Arc::new(client),
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

    /// Elimina de la pila cualquier item cuyo epoch no esté en el
    /// conjunto de epochs todavía válidos. Útil cuando cambias de vista
    /// o el usuario scrollea tan fuerte que quieres tirar lo pendiente
    /// que ya no corresponde a lo visible, sin gastar una descarga en
    /// algo que no vas a mostrar.
    ///
    /// Ejemplo: `queue.drop_stale(|e| e == self.page_generation)`.
    pub fn drop_stale(&self, is_still_valid: impl Fn(u64) -> bool) {
        let mut state = self.state.lock().unwrap();
        let removed: Vec<String> = state
            .stack
            .iter()
            .filter(|item| !is_still_valid(item.epoch))
            .map(|item| item.key.clone())
            .collect();

        state.stack.retain(|item| is_still_valid(item.epoch));
        for key in removed {
            state.in_flight_or_queued.remove(&key);
            // No tocamos `downloading` aquí: si una key ya tiene worker
            // corriendo, no está en `stack` (spawn_next la sacó), así
            // que `removed` nunca la incluye. Es un no-op seguro.
        }
    }

    /// Poda del stack cualquier item cuya key NO esté en `still_wanted`,
    /// PERO solo si esa key todavía no tiene un worker activo (es decir,
    /// sigue esperando su turno en el stack). Las que ya están
    /// descargando se dejan terminar — cancelarlas a mitad de un
    /// `reqwest` no es seguro/sencillo y de todas formas ya casi terminan.
    ///
    /// Este es el fix al "scroll rápido deja huecos": sin esto, una key
    /// que quedó fuera de la ventana visible sigue marcada en
    /// `in_flight_or_queued` para siempre (nunca tuvo la suerte de que
    /// le tocara worker), así que si el usuario vuelve a scrollear sobre
    /// ella, `enqueue` la descarta creyendo que ya está en curso, y
    /// nunca se descarga. Llama esto en cada `Scrolled`, pasando las
    /// keys de la ventana visible actual (+buffer).
    pub fn drop_outside_visible(&self, still_wanted: &HashSet<String>) {
        let mut state = self.state.lock().unwrap();

        let removed: Vec<String> = state
            .stack
            .iter()
            .filter(|item| !still_wanted.contains(&item.key))
            .map(|item| item.key.clone())
            .collect();

        state.stack.retain(|item| still_wanted.contains(&item.key));
        for key in removed {
            debug_assert!(
                !state.downloading.contains(&key),
                "drop_outside_visible: key '{key}' estaba en el stack Y en downloading a la vez — invariante rota"
            );
            state.in_flight_or_queued.remove(&key);
        }
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

        let client = Arc::clone(&self.client);

        Some(Task::perform(
            download_bytes(client, item.url),
            move |result| match result {
                Ok(bytes) => to_message(item.key.clone(), bytes, item.epoch),
                Err(_) => to_message(item.key.clone(), Vec::new(), item.epoch),
            },
        ))
    }

    /// Útil para debug/telemetría en la UI si quieres mostrar
    /// "descargando N thumbnails...".
    pub fn active_workers(&self) -> usize {
        self.state.lock().unwrap().active_workers
    }

    pub fn queued_len(&self) -> usize {
        self.state.lock().unwrap().stack.len()
    }
}

impl Default for DownloadQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// Descarga y RECORTA A CUADRADO (mismo comportamiento que tenía
/// `ui::utils::image::download_thumbnail`), pero reusando el cliente
/// compartido en vez de crear uno por request. Si prefieres mantener
/// `image::download_thumbnail` como está y solo cambiarle la firma para
/// aceptar un `&reqwest::Client`, es un cambio mínimo — dímelo y te lo
/// ajusto; aquí lo inline para que este módulo no dependa de la ruta de
/// tu crate.
async fn download_bytes(client: Arc<reqwest::Client>, url: String) -> Result<Vec<u8>, String> {
    use image::ImageReader;
    use std::io::Cursor;

    let bytes = client
        .get(&url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .bytes()
        .await
        .map_err(|e| e.to_string())?;

    let img = ImageReader::new(Cursor::new(&bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?
        .decode()
        .map_err(|e| e.to_string())?;

    let (w, h) = (img.width(), img.height());
    let size = w.min(h);
    let x = (w - size) / 2;
    let y = (h - size) / 2;
    let cropped = img.crop_imm(x, y, size, size);

    let mut out = Vec::new();
    cropped
        .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Jpeg)
        .map_err(|e| e.to_string())?;

    Ok(out)
}