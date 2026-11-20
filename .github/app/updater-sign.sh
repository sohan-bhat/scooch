#!/usr/bin/env bash
set -eu

target="$1"
if [ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" ]; then
  echo "No updater key, so this build carries no update files."
  exit 0
fi
if [ -z "${TAURI_ARGS:-}" ]; then
  echo "The updater key is set but the public key is not, so this build carries no update files."
  exit 0
fi

bundle="target/$target/release/bundle"
if [ -d "$bundle/macos/Scooch.app" ]; then
  COPYFILE_DISABLE=1 tar -czf "$bundle/macos/Scooch.app.tar.gz" -C "$bundle/macos" Scooch.app
fi

signed=0
for file in "$bundle"/macos/*.app.tar.gz "$bundle"/nsis/*-setup.exe "$bundle"/appimage/*.AppImage "$bundle"/deb/*.deb "$bundle"/rpm/*.rpm; do
  [ -f "$file" ] || continue
  node ui/node_modules/@tauri-apps/cli/tauri.js signer sign "$file" >/dev/null
  [ -s "$file.sig" ] || { echo "No signature came out for $(basename "$file")."; exit 1; }
  echo "signed $(basename "$file")"
  signed=$((signed + 1))
done
[ "$signed" -gt 0 ] || { echo "Nothing to sign under $bundle."; exit 1; }
