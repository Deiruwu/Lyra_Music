#!/usr/bin/env bash
# Compila atelier, lo enlaza en ~/.local/bin, instala ícono y .desktop y, opcionalmente, el servidor local.
set -euo pipefail

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

ok()   { echo -e "${GREEN}[OK]${NC} $1"; }
err()  { echo -e "${RED}[ERROR]${NC} $1"; exit 1; }
info() { echo -e "${YELLOW}[INFO]${NC} $1"; }

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN_DIR="$HOME/.local/bin"
DATA_HOME="${XDG_DATA_HOME:-$HOME/.local/share}"
APPS_DIR="$DATA_HOME/applications"
ICON_DIR="$DATA_HOME/icons/hicolor/256x256/apps"

command -v cargo &>/dev/null || err "Falta 'cargo'. Instala Rust (https://rustup.rs) y vuelve a correr el script."

# ── Compilar ──────────────────────────────────────────────────────────────────
info "Compilando atelier..."
cargo build --release --manifest-path "$REPO/Cargo.toml"
ok "Binario listo en $REPO/target/release/atelier"

# ── Enlace en ~/.local/bin ────────────────────────────────────────────────────
mkdir -p "$BIN_DIR"
if [ -e "$BIN_DIR/atelier" ] && [ ! -L "$BIN_DIR/atelier" ]; then
    mv "$BIN_DIR/atelier" "$BIN_DIR/atelier.bak"
    info "Había un binario suelto en $BIN_DIR/atelier; se movió a atelier.bak"
fi
ln -sfn "$REPO/target/release/atelier" "$BIN_DIR/atelier"
ok "Enlace $BIN_DIR/atelier -> $REPO/target/release/atelier"

case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *) info "$BIN_DIR no está en tu PATH: el menú de apps funciona igual, pero 'atelier' no desde la terminal." ;;
esac

# ── Ícono ─────────────────────────────────────────────────────────────────────
mkdir -p "$ICON_DIR"
cp "$REPO/assets/icons/atelier_icon_256.png" "$ICON_DIR/atelier.png"
command -v gtk-update-icon-cache &>/dev/null && gtk-update-icon-cache -qtf "$DATA_HOME/icons/hicolor" || true
ok "Ícono instalado."

# ── .desktop ──────────────────────────────────────────────────────────────────
mkdir -p "$APPS_DIR"
cat > "$APPS_DIR/atelier.desktop" <<EOF
[Desktop Entry]
Version=1.0
Type=Application
Name=Atelier
Comment=Reproductor de música
Exec=$BIN_DIR/atelier
Path=$REPO
Icon=atelier
Terminal=false
Categories=AudioVideo;Audio;Music;
Keywords=música;reproductor;audio;
StartupWMClass=atelier
EOF
command -v update-desktop-database &>/dev/null && update-desktop-database -q "$APPS_DIR" || true
ok "Acceso directo en $APPS_DIR/atelier.desktop"

# ── Servidor local ────────────────────────────────────────────────────────────
read -rp "¿Instalar también el servidor local (track_manager)? [Y/n] " answer
if [[ ! "$answer" =~ ^[nN]$ ]]; then
    "$REPO/scripts/setup-local-server.sh"
else
    info "Sin servidor local: configura uno remoto en Ajustes → Servidor."
fi

ok "Listo. Abre Atelier desde el menú de aplicaciones."
