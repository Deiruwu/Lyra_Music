#!/usr/bin/env bash
# Instala atelier precompilado con su track_manager local: binarios, dependencias, ícono y .desktop. No necesita cargo ni git.
set -euo pipefail

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

ok()   { echo -e "${GREEN}[OK]${NC} $1"; }
err()  { echo -e "${RED}[ERROR]${NC} $1"; exit 1; }
info() { echo -e "${YELLOW}[INFO]${NC} $1"; }

PKG="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DATA_HOME="${XDG_DATA_HOME:-$HOME/.local/share}"
LYRA="$DATA_HOME/lyra"
APP_DIR="$LYRA/app"
SERVER="$LYRA/server"
TM_DIR="$SERVER/track_manager"
TOOLS="$SERVER/bin"
BIN_DIR="$HOME/.local/bin"
PYTHON_VERSION="3.12"

command -v curl &>/dev/null || err "Falta 'curl'."
mkdir -p "$APP_DIR" "$TOOLS" "$SERVER/music" "$TM_DIR/target/release" "$BIN_DIR"
export PATH="$TOOLS:$PATH"

# ── Paquetes del sistema ──────────────────────────────────────────────────────
MISSING=()
command -v ffmpeg  &>/dev/null || MISSING+=(ffmpeg)
command -v ffprobe &>/dev/null || MISSING+=(ffmpeg)
ldconfig -p 2>/dev/null | grep -q libasound.so.2 || MISSING+=(alsa-lib)
if [ ${#MISSING[@]} -ne 0 ]; then
    command -v pacman &>/dev/null || err "Faltan: ${MISSING[*]}. Instálalos con tu gestor de paquetes."
    read -rp "Faltan paquetes (${MISSING[*]}). ¿Ejecutar 'sudo pacman -S --needed ${MISSING[*]}'? [y/N] " answer
    [[ "$answer" =~ ^[yY]$ ]] || err "Son necesarios para reproducir y descargar audio."
    sudo pacman -S --needed "${MISSING[@]}"
fi
ok "Paquetes del sistema listos."

# ── yt-dlp ────────────────────────────────────────────────────────────────────
if [ -x "$TOOLS/yt-dlp" ]; then
    "$TOOLS/yt-dlp" -U -q || info "No se pudo actualizar yt-dlp, se usa la versión actual."
elif ! command -v yt-dlp &>/dev/null; then
    info "Descargando yt-dlp..."
    curl -fL --progress-bar -o "$TOOLS/yt-dlp" \
        https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux
    chmod +x "$TOOLS/yt-dlp"
fi
ok "yt-dlp listo."

# ── uv ────────────────────────────────────────────────────────────────────────
if ! command -v uv &>/dev/null; then
    info "Instalando uv..."
    curl -LsSf https://astral.sh/uv/install.sh | env UV_INSTALL_DIR="$TOOLS" UV_NO_MODIFY_PATH=1 sh
fi
ok "uv listo."

# ── Binarios ──────────────────────────────────────────────────────────────────
install -m 755 "$PKG/atelier"       "$APP_DIR/atelier"
install -m 755 "$PKG/track_manager" "$TM_DIR/target/release/track_manager"
cp -r "$PKG/Music_Services" "$TM_DIR/"
ok "Binarios instalados en $LYRA"

if [ -e "$BIN_DIR/atelier" ] && [ ! -L "$BIN_DIR/atelier" ]; then
    mv "$BIN_DIR/atelier" "$BIN_DIR/atelier.bak"
    info "Había un binario suelto en $BIN_DIR/atelier; se movió a atelier.bak"
fi
ln -sfn "$APP_DIR/atelier" "$BIN_DIR/atelier"
ok "Enlace $BIN_DIR/atelier -> $APP_DIR/atelier"

# ── Entorno Python ────────────────────────────────────────────────────────────
VENV="$TM_DIR/Music_Services/.venv"
if [ ! -x "$VENV/bin/python" ]; then
    info "Creando entorno Python $PYTHON_VERSION (la primera vez tarda un poco)..."
    uv venv --python "$PYTHON_VERSION" "$VENV"
fi
if ! cmp -s "$TM_DIR/Music_Services/requirements.txt" "$VENV/.requirements.installed"; then
    info "Instalando dependencias Python..."
    uv pip install --python "$VENV/bin/python" -r "$TM_DIR/Music_Services/requirements.txt"
    cp "$TM_DIR/Music_Services/requirements.txt" "$VENV/.requirements.installed"
fi
ok "Entorno Python listo."

# ── Ícono y .desktop ──────────────────────────────────────────────────────────
ICON_DIR="$DATA_HOME/icons/hicolor/256x256/apps"
APPS_DIR="$DATA_HOME/applications"
mkdir -p "$ICON_DIR" "$APPS_DIR"
cp "$PKG/atelier.png" "$ICON_DIR/atelier.png"
command -v gtk-update-icon-cache &>/dev/null && gtk-update-icon-cache -qtf "$DATA_HOME/icons/hicolor" || true

cat > "$APPS_DIR/atelier.desktop" <<EOF
[Desktop Entry]
Version=1.0
Type=Application
Name=Atelier
Comment=Reproductor de música
Exec=$APP_DIR/atelier
Path=$LYRA
Icon=atelier
Terminal=false
Categories=AudioVideo;Audio;Music;
Keywords=música;reproductor;audio;
StartupWMClass=atelier
EOF
command -v update-desktop-database &>/dev/null && update-desktop-database -q "$APPS_DIR" || true
ok "Acceso directo creado."

ok "Listo. Abre Atelier desde el menú de aplicaciones (modo Local)."
