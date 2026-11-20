#!/usr/bin/env bash
set -eu

bundles="$1"
root="$(cd "$(dirname "$0")/../.." && pwd)"
app="$root/crates/scooch-app"
out="$root/signatures"
mkdir -p "$out"

if [ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" ]; then
  echo "No updater key, so the release carries no update files."
  exit 0
fi

(cd "$app/ui" && npm ci --ignore-scripts --no-audit --no-fund >/dev/null)
work=$(mktemp -d)

sign() {
  local folder="$bundles/$1" pattern="$2" fixed="$3" found
  found=$(find "$folder" -maxdepth 1 -type f -name "$pattern" | head -1)
  if [ -z "$found" ] || [ -L "$found" ]; then
    echo "No plain file matches $pattern in $1."
    exit 1
  fi
  cp -- "$found" "$work/$fixed"
  (cd "$app" && node ui/node_modules/@tauri-apps/cli/tauri.js signer sign "$work/$fixed" >/dev/null)
  if [ ! -s "$work/$fixed.sig" ]; then
    echo "No signature came out for $fixed."
    exit 1
  fi
  cp "$work/$fixed.sig" "$out/$fixed.sig"
  echo "signed $fixed"
}

sign app-macos '*.app.tar.gz' Scooch.app.tar.gz
sign app-windows-x64 '*_x64-setup.exe' Scooch_x64-setup.exe
sign app-windows-arm64 '*_arm64-setup.exe' Scooch_arm64-setup.exe
sign app-linux-x64 '*_amd64.AppImage' Scooch_amd64.AppImage
sign app-linux-arm64 '*_aarch64.AppImage' Scooch_aarch64.AppImage
sign app-linux-x64 '*_amd64.deb' scooch_amd64.deb
sign app-linux-arm64 '*_arm64.deb' scooch_arm64.deb
sign app-linux-x64 '*.x86_64.rpm' scooch.x86_64.rpm
sign app-linux-arm64 '*.aarch64.rpm' scooch.aarch64.rpm
