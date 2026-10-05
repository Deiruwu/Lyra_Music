//! track_manager local: lo lanza como proceso hijo atado a la vida de atelier y publica su estado de arranque.

use std::fs::File;
use std::net::{SocketAddr, TcpStream};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{LazyLock, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use tokio::sync::watch;

use crate::ui::utils::data_dir::data_dir;

/// Puertos del track_manager local y de su microservicio Python.
pub const LOCAL_PORT: u16 = 47878;
const LOCAL_PYTHON_PORT: u16 = 49999;
const READY_TIMEOUT: Duration = Duration::from_secs(90);
const POLL_INTERVAL: Duration = Duration::from_millis(250);
const SHUTDOWN_GRACE: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq)]
pub enum ServerState {
    Starting,
    Ready,
    Failed(String),
}

/// Estado de arranque del servidor local (`Ready` en modo remoto).
static STATE: LazyLock<watch::Sender<ServerState>> = LazyLock::new(|| watch::Sender::new(ServerState::Ready));
static CHILD: Mutex<Option<Child>> = Mutex::new(None);

/// Raíz de la instalación local que crea `scripts/setup-local-server.sh`.
pub fn server_root() -> PathBuf {
    data_dir().join("lyra").join("server")
}

fn binary_path() -> PathBuf {
    server_root().join("track_manager").join("target").join("release").join("track_manager")
}

pub fn log_path() -> PathBuf {
    server_root().join("track_manager.log")
}

pub fn is_installed() -> bool {
    binary_path().is_file()
}

pub fn state() -> ServerState {
    STATE.borrow().clone()
}

/// Espera a que el servidor termine de arrancar; `Err` con el motivo si falló.
pub async fn wait_ready() -> Result<(), String> {
    let mut rx = STATE.subscribe();
    let state = rx.wait_for(|s| *s != ServerState::Starting).await.map(|s| s.clone());
    match state {
        Ok(ServerState::Failed(reason)) => Err(reason),
        _ => Ok(()),
    }
}

/// Lanza track_manager desde el hilo principal y vigila su arranque.
pub fn start() {
    if !is_installed() {
        fail("track_manager local no está instalado: corre scripts/setup-local-server.sh".into());
        return;
    }
    if port_open(LOCAL_PORT) {
        fail(format!("El puerto {LOCAL_PORT} ya está ocupado por otro proceso"));
        return;
    }

    STATE.send_replace(ServerState::Starting);
    match spawn() {
        Ok(child) => {
            *child_slot() = Some(child);
            std::thread::spawn(watch_startup);
        }
        Err(e) => fail(format!("No se pudo iniciar track_manager: {e}")),
    }
}

/// Termina el hijo con SIGTERM y, si no sale a tiempo, con SIGKILL.
pub fn shutdown() {
    let Some(mut child) = child_slot().take() else { return };

    unsafe {
        libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
    }
    let deadline = Instant::now() + SHUTDOWN_GRACE;
    while Instant::now() < deadline {
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn spawn() -> std::io::Result<Child> {
    let root = server_root();
    let log = File::create(log_path())?;

    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let path = std::env::join_paths(std::iter::once(root.join("bin")).chain(std::env::split_paths(&inherited)))
        .map_err(std::io::Error::other)?;

    let mut command = Command::new(binary_path());
    command
        .current_dir(root.join("track_manager"))
        .env("SERVER_HOST", "127.0.0.1")
        .env("SERVER_PORT", LOCAL_PORT.to_string())
        .env("PYTHON_HOST", "127.0.0.1")
        .env("PYTHON_PORT", LOCAL_PYTHON_PORT.to_string())
        .env("DATABASE_URL", format!("sqlite://{}", root.join("track_manager.db").display()))
        .env("MUSIC_STORAGE_PATH", root.join("music"))
        .env("PATH", path)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);

    unsafe {
        command.pre_exec(|| {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }

    command.spawn()
}

/// Sondea el puerto hasta que responde, el hijo muere o se agota el tiempo.
fn watch_startup() {
    let deadline = Instant::now() + READY_TIMEOUT;
    loop {
        let exited = child_slot().as_mut().and_then(|c| c.try_wait().ok().flatten());
        if let Some(status) = exited {
            fail(format!("track_manager terminó al arrancar ({status}); revisa {}", log_path().display()));
            return;
        }
        if port_open(LOCAL_PORT) {
            STATE.send_replace(ServerState::Ready);
            return;
        }
        if Instant::now() >= deadline {
            fail(format!("track_manager no respondió en {} s; revisa {}", READY_TIMEOUT.as_secs(), log_path().display()));
            return;
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

fn fail(reason: String) {
    eprintln!("[LocalServer] {reason}");
    STATE.send_replace(ServerState::Failed(reason));
}

fn port_open(port: u16) -> bool {
    TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], port)), POLL_INTERVAL).is_ok()
}

fn child_slot() -> MutexGuard<'static, Option<Child>> {
    CHILD.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
