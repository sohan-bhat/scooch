#!/usr/bin/env bash
set -eu

compiled="$1"
target="$2"
root="$(cd "$(dirname "$0")/../.." && pwd)"
app="$root/crates/scooch-app"
. "$root/.github/app/take.sh"

case "$target" in
  x86_64-pc-windows-msvc | aarch64-pc-windows-msvc) ;;
  *) echo "No Windows target named $target."; exit 1 ;;
esac
take scooch-app.exe "$app/target/$target/release/scooch-app.exe"
take scooch.exe "$app/binaries/scooch-$target.exe"
take scoochw.exe "$app/binaries/scoochw-$target.exe"
bundler

if [ -n "${WINDOWS_CERTIFICATE:-}" ]; then
  echo "Signing with the Windows certificate."
else
  echo "No Windows certificate, so the installer stays unsigned."
  unset WINDOWS_CERTIFICATE WINDOWS_CERTIFICATE_PASSWORD
fi
bundle --target "$target" --bundles nsis

mkdir -p "$root/dist"
cp "$app/target/$target/release/bundle/nsis/"*-setup.exe "$root/dist/"
ls -l "$root/dist"
