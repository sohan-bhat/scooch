#!/usr/bin/env bash
set -eu

built="$1"
release="$2"
version="$3"
tag="${4:-v$3}"
mkdir -p "$release"

take() {
  found=$(find "$built" -type f -name "$1" | head -1)
  if [ -z "$found" ]; then
    echo "No build output matches $1 for $2."
    exit 1
  fi
  cp "$found" "$release/$2"
}

take '*_universal.dmg' Scooch.dmg
take '*_x64-setup.exe' Scooch-Setup-x64.exe
take '*_arm64-setup.exe' Scooch-Setup-arm64.exe
take '*_amd64.deb' scooch_amd64.deb
take '*_arm64.deb' scooch_arm64.deb
take '*.x86_64.rpm' scooch.x86_64.rpm
take '*.aarch64.rpm' scooch.aarch64.rpm
take '*_amd64.AppImage' Scooch-x86_64.AppImage
take '*_aarch64.AppImage' Scooch-aarch64.AppImage

if [ -z "$(find "$built" -type f -name '*.sig' | head -1)" ]; then
  echo "No update signatures came out of the builds, so the release carries no update files."
  exit 0
fi

take '*.app.tar.gz' Scooch.app.tar.gz
take '*.app.tar.gz.sig' Scooch.app.tar.gz.sig
take '*_x64-setup.exe.sig' Scooch-Setup-x64.exe.sig
take '*_arm64-setup.exe.sig' Scooch-Setup-arm64.exe.sig
take '*_amd64.AppImage.sig' Scooch-x86_64.AppImage.sig
take '*_aarch64.AppImage.sig' Scooch-aarch64.AppImage.sig
take '*_amd64.deb.sig' scooch_amd64.deb.sig
take '*_arm64.deb.sig' scooch_arm64.deb.sig
take '*.x86_64.rpm.sig' scooch.x86_64.rpm.sig
take '*.aarch64.rpm.sig' scooch.aarch64.rpm.sig

base="https://github.com/sohan-bhat/scooch/releases/download/$tag"
entries=""
add() {
  signature=$(tr -d '\r\n' <"$release/$2.sig")
  entry=$(jq -n --arg key "$1" --arg signature "$signature" --arg url "$base/$2" '{($key): {signature: $signature, url: $url}}')
  entries="$entries$entry"
}
add darwin-aarch64 Scooch.app.tar.gz
add darwin-aarch64-app Scooch.app.tar.gz
add darwin-x86_64 Scooch.app.tar.gz
add darwin-x86_64-app Scooch.app.tar.gz
add windows-x86_64 Scooch-Setup-x64.exe
add windows-x86_64-nsis Scooch-Setup-x64.exe
add windows-aarch64 Scooch-Setup-arm64.exe
add windows-aarch64-nsis Scooch-Setup-arm64.exe
add linux-x86_64 Scooch-x86_64.AppImage
add linux-x86_64-appimage Scooch-x86_64.AppImage
add linux-aarch64 Scooch-aarch64.AppImage
add linux-aarch64-appimage Scooch-aarch64.AppImage
add linux-x86_64-deb scooch_amd64.deb
add linux-aarch64-deb scooch_arm64.deb
add linux-x86_64-rpm scooch.x86_64.rpm
add linux-aarch64-rpm scooch.aarch64.rpm
echo "$entries" | jq -s --arg version "$version" --arg date "$(date -u +%Y-%m-%dT%H:%M:%SZ)" '{version: $version, notes: "", pub_date: $date, platforms: add}' >"$release/latest.json"
echo "latest.json names $(jq '.platforms | length' "$release/latest.json") platforms."
