#!/bin/bash
# Build the Vault HQ Mac app (Tauri, src-tauri/) and install it to ~/Applications.
# The app is only the native front; the vault server is the local.vault-hq service
# (scripts/service.sh). Usage: scripts/app.sh            build, install, (re)open
#                              scripts/app.sh build      build only
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"
command -v cargo >/dev/null || { echo "Rust is missing: brew install rustup && rustup default stable" >&2; exit 1; }

npx tauri build --bundles app
built="src-tauri/target/release/bundle/macos/Vault HQ.app"
[ "${1:-}" = "build" ] && { echo "built: $built"; exit 0; }

dest="$HOME/Applications/Vault HQ.app"
mkdir -p "$HOME/Applications"
osascript -e 'tell application id "local.vaulthq.desktop" to quit' >/dev/null 2>&1 || true
sleep 1
rm -rf "$dest"
cp -R "$built" "$dest"
open "$dest"
echo "installed and opened: $dest"
