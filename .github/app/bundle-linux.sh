#!/usr/bin/env bash
set -eu

compiled="$1"
target="$2"
root="$(cd "$(dirname "$0")/../.." && pwd)"
app="$root/crates/scooch-app"
. "$root/.github/app/take.sh"

case "$target" in
  x86_64-unknown-linux-gnu | aarch64-unknown-linux-gnu) ;;
  *) echo "No Linux target named $target."; exit 1 ;;
esac
take scooch-app "$app/target/$target/release/scooch-app"
take scooch "$app/binaries/scooch-$target"
sudo apt-get update >/dev/null
sudo apt-get install -y libwebkit2gtk-4.1-dev librsvg2-dev patchelf file >/dev/null
bundler
bundle --target "$target" --bundles deb,rpm,appimage

made="$app/target/$target/release/bundle"
mkdir -p "$root/dist"
cp "$made"/deb/*.deb "$made"/rpm/*.rpm "$made"/appimage/*.AppImage "$root/dist/"
ls -l "$root/dist"
