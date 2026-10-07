#!/usr/bin/env bash
# Compila atelier y track_manager (SQLite) en release y arma dist/atelier-linux-x86_64.tar.gz listo para instalar sin cargo.
set -euo pipefail

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

ok()   { echo -e "${GREEN}[OK]${NC} $1"; }
err()  { echo -e "${RED}[ERROR]${NC} $1"; exit 1; }
info() { echo -e "${YELLOW}[INFO]${NC} $1"; }

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TRACK_MANAGER="${TRACK_MANAGER_DIR:-$REPO/../track_manager}"
NAME="atelier-linux-x86_64"
STAGE="$REPO/dist/$NAME"

[ -f "$TRACK_MANAGER/Cargo.toml" ] || err "No encuentro track_manager en $TRACK_MANAGER (usa TRACK_MANAGER_DIR=...)."

# ── Compilar ──────────────────────────────────────────────────────────────────
info "Compilando atelier..."
cargo build --release --manifest-path "$REPO/Cargo.toml"

info "Compilando track_manager (SQLite)..."
(cd "$TRACK_MANAGER" && CARGO_TARGET_DIR=target/sqlite cargo build --release --no-default-features --features sqlite)
ok "Binarios listos."

# ── Armar el paquete ──────────────────────────────────────────────────────────
mkdir -p "$STAGE/Music_Services"
cp "$REPO/target/release/atelier"                     "$STAGE/atelier"
cp "$TRACK_MANAGER/target/sqlite/release/track_manager" "$STAGE/track_manager"
cp "$REPO/assets/icons/atelier_icon_256.png"          "$STAGE/atelier.png"
cp "$REPO/scripts/install-prebuilt.sh"                "$STAGE/install.sh"
strip "$STAGE/atelier" "$STAGE/track_manager" 2>/dev/null || true

tar -C "$TRACK_MANAGER/Music_Services" \
    --exclude=.venv --exclude=__pycache__ --exclude=.idea --exclude=.pytest_cache \
    --exclude='test*.py' -cf - . | tar -C "$STAGE/Music_Services" -xf -

tar -C "$REPO/dist" -czf "$REPO/dist/$NAME.tar.gz" "$NAME"
ok "Paquete listo: $REPO/dist/$NAME.tar.gz ($(du -h "$REPO/dist/$NAME.tar.gz" | cut -f1))"
info "Para instalar: tar xzf $NAME.tar.gz && ./$NAME/install.sh"
