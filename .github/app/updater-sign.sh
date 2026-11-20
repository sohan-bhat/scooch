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
signed=0
while IFS= read -r -d '' file; do
  if [ -L "$file" ] || [ ! -f "$file" ]; then
    echo "Skipped $(basename "$file"), it is not a plain file."
    continue
  fi
  name=$(basename "$file")
  cp "$file" "$work/$name"
  (cd "$app" && node ui/node_modules/@tauri-apps/cli/tauri.js signer sign "$work/$name" >/dev/null)
  if [ ! -s "$work/$name.sig" ]; then
    echo "No signature came out for $name."
    exit 1
  fi
  cp "$work/$name.sig" "$out/$name.sig"
  echo "signed $name"
  signed=$((signed + 1))
done < <(find "$bundles" \( -name '*.app.tar.gz' -o -name '*-setup.exe' -o -name '*.AppImage' -o -name '*.deb' -o -name '*.rpm' \) -print0)
if [ "$signed" -eq 0 ]; then
  echo "Nothing to sign under $bundles."
  exit 1
fi
