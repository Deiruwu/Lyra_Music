//! Descarga de imágenes remotas (miniaturas, portadas, banners) con un cliente
//! compartido con timeouts y reintentos.
//!
//! Los errores de red y los 5xx se reintentan con un backoff global: si se cae
//! internet, todas las descargas esperan el mismo temporizador (que crece hasta
//! `MAX_BACKOFF`) en vez de insistir cada una por su cuenta, y el primer éxito
//! lo resetea. Un 404 o una respuesta inválida no se reintentan.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use tokio::sync::Semaphore;
use tokio::time::Instant;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const BASE_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(60);
const MAX_JITTER_MS: u64 = 1_000;
/// Intentos para descargas sin bandera de aborto (nadie las cancela si dejan de importar).
const UNGUARDED_MAX_ATTEMPTS: u32 = 6;
const MAX_CONCURRENT_DOWNLOADS: usize = 15;

static CONSECUTIVE_FAILURES: AtomicU32 = AtomicU32::new(0);
static RETRY_AT: Mutex<Option<Instant>> = Mutex::new(None);

enum Failure {
    /// Red caída, timeout o error del servidor: vale la pena reintentar.
    Transient(String),
    /// 404, 403, etc.: reintentar no cambia nada.
    Permanent(String),
}

/// Limita la concurrencia global de descargas de imágenes para no saturar sockets TCP.
pub(crate) fn download_limiter() -> &'static Semaphore {
    static LIMITER: OnceLock<Semaphore> = OnceLock::new();
    LIMITER.get_or_init(|| Semaphore::new(MAX_CONCURRENT_DOWNLOADS))
}

fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .expect("no se pudo construir el cliente HTTP de imágenes")
    })
}

/// Bytes crudos de `url`. Con `aborted`, reintenta hasta lograrlo o hasta que se
/// marque la bandera; sin ella, se rinde tras `UNGUARDED_MAX_ATTEMPTS` intentos.
pub async fn fetch_image_bytes(url: &str, aborted: Option<&AtomicBool>) -> Result<Vec<u8>, String> {
    let is_aborted = || aborted.is_some_and(|flag| flag.load(Ordering::Relaxed));
    let mut attempts = 0;

    loop {
        wait_for_backoff().await;
        if is_aborted() {
            return Err("descarga abortada".into());
        }

        attempts += 1;
        let result = {
            let _permit = download_limiter().acquire().await.map_err(|e| e.to_string())?;
            if is_aborted() {
                return Err("descarga abortada".into());
            }
            try_fetch(url).await
        };

        match result {
            Ok(bytes) => {
                record_success();
                return Ok(bytes);
            }
            Err(Failure::Permanent(error)) => return Err(error),
            Err(Failure::Transient(error)) => {
                record_failure();
                if aborted.is_none() && attempts >= UNGUARDED_MAX_ATTEMPTS {
                    return Err(error);
                }
            }
        }
    }
}

async fn try_fetch(url: &str) -> Result<Vec<u8>, Failure> {
    let response = client().get(url).send().await.map_err(|e| Failure::Transient(e.to_string()))?;

    let status = response.status();
    if !status.is_success() {
        let retryable = status.is_server_error() || status.as_u16() == 408 || status.as_u16() == 429;
        let error = format!("HTTP {status} para {url}");
        return Err(if retryable { Failure::Transient(error) } else { Failure::Permanent(error) });
    }

    response
        .bytes()
        .await
        .map(|bytes| bytes.to_vec())
        .map_err(|e| Failure::Transient(e.to_string()))
}

/// Espera a que venza el backoff global (más un jitter para no salir todas a la vez).
async fn wait_for_backoff() {
    let retry_at = *RETRY_AT.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(at) = retry_at.filter(|at| *at > Instant::now()) {
        let jitter = Duration::from_millis(rand::random::<u64>() % MAX_JITTER_MS);
        tokio::time::sleep_until(at + jitter).await;
    }
}

fn record_failure() {
    let failures = CONSECUTIVE_FAILURES.fetch_add(1, Ordering::Relaxed) + 1;
    let delay = BASE_BACKOFF
        .saturating_mul(2u32.saturating_pow(failures.saturating_sub(1).min(16)))
        .min(MAX_BACKOFF);

    let mut retry_at = RETRY_AT.lock().unwrap_or_else(|e| e.into_inner());
    let candidate = Instant::now() + delay;
    if retry_at.is_none_or(|at| at < candidate) {
        *retry_at = Some(candidate);
    }
}

fn record_success() {
    CONSECUTIVE_FAILURES.store(0, Ordering::Relaxed);
    *RETRY_AT.lock().unwrap_or_else(|e| e.into_inner()) = None;
}
