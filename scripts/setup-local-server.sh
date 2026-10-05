#!/usr/bin/env bash
# Instala o actualiza un track_manager local (SQLite) que atelier lanza al iniciar en modo Local.
set -euo pipefail

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

ok()   { echo -e "${GREEN}[OK]${NC} $1"; }
err()  { echo -e "${RED}[ERROR]${NC} $1"; exit 1; }
info() { echo -e "${YELLOW}[INFO]${NC} $1"; }

REPO_URL="${TRACK_MANAGER_REPO:-https://github.com/Deiruwu/track_manager.git}"
ROOT="${XDG_DATA_HOME:-$HOME/.local/share}/lyra/server"
BIN="$ROOT/bin"
REPO="$ROOT/track_manager"
PYTHON_VERSION="3.12"

mkdir -p "$BIN" "$ROOT/music"
export PATH="$BIN:$PATH"

# ── Herramientas de compilación ───────────────────────────────────────────────
for cmd in git cargo curl; do
    command -v "$cmd" &>/dev/null || err "Falta '$cmd'. Instálalo y vuelve a correr el script."
done

# ── ffmpeg / ffprobe ──────────────────────────────────────────────────────────
if ! command -v ffmpeg &>/dev/null || ! command -v ffprobe &>/dev/null; then
    if   command -v pacman &>/dev/null; then INSTALL="sudo pacman -S --needed ffmpeg"
    elif command -v apt    &>/dev/null; then INSTALL="sudo apt install -y ffmpeg"
    elif command -v dnf    &>/dev/null; then INSTALL="sudo dnf install -y ffmpeg"
    else err "Falta ffmpeg y no reconozco tu gestor de paquetes. Instálalo a mano."
    fi
    read -rp "Falta ffmpeg. ¿Ejecutar '$INSTALL'? [y/N] " answer
    [[ "$answer" =~ ^[yY]$ ]] || err "ffmpeg es necesario para descargar audio."
    $INSTALL
fi
ok "ffmpeg listo."

# ── yt-dlp ────────────────────────────────────────────────────────────────────
if [ -x "$BIN/yt-dlp" ]; then
    info "Actualizando yt-dlp..."
    "$BIN/yt-dlp" -U -q || info "No se pudo actualizar yt-dlp, se usa la versión actual."
elif ! command -v yt-dlp &>/dev/null; then
    info "Descargando yt-dlp..."
    curl -fL --progress-bar -o "$BIN/yt-dlp" \
        https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux
    chmod +x "$BIN/yt-dlp"
fi
ok "yt-dlp listo."

# ── uv ────────────────────────────────────────────────────────────────────────
if ! command -v uv &>/dev/null; then
    info "Instalando uv..."
    curl -LsSf https://astral.sh/uv/install.sh | env UV_INSTALL_DIR="$BIN" UV_NO_MODIFY_PATH=1 sh
fi
ok "uv listo."

# ── track_manager ─────────────────────────────────────────────────────────────
if [ -d "$REPO/.git" ]; then
    info "Actualizando track_manager..."
    git -C "$REPO" pull --ff-only
else
    info "Clonando track_manager..."
    git clone "$REPO_URL" "$REPO"
fi

info "Compilando track_manager (SQLite)..."
cargo build --release --no-default-features --features sqlite --manifest-path "$REPO/Cargo.toml"
ok "Binario listo en $REPO/target/release/track_manager"

# ── Entorno Python ────────────────────────────────────────────────────────────
SERVICES="$REPO/Music_Services"
VENV="$SERVICES/.venv"

if [ ! -x "$VENV/bin/python" ]; then
    info "Creando entorno Python $PYTHON_VERSION..."
    uv venv --python "$PYTHON_VERSION" "$VENV"
fi

if ! cmp -s "$SERVICES/requirements.txt" "$VENV/.requirements.installed"; then
    info "Instalando dependencias Python..."
    uv pip install --python "$VENV/bin/python" -r "$SERVICES/requirements.txt"
    cp "$SERVICES/requirements.txt" "$VENV/.requirements.installed"
fi
ok "Entorno Python listo."

ok "track_manager local instalado en $ROOT. Inicia atelier en modo Local."
