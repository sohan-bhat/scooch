#!/usr/bin/env bash
set -eu

script="$(cd "$(dirname "$0")" && pwd)/release-files.sh"
work=$(mktemp -d)
names="Scooch_0.9.0_universal.dmg Scooch_0.9.0_x64-setup.exe Scooch_0.9.0_arm64-setup.exe Scooch_0.9.0_amd64.deb Scooch_0.9.0_arm64.deb Scooch-0.9.0-1.x86_64.rpm Scooch-0.9.0-1.aarch64.rpm Scooch_0.9.0_amd64.AppImage Scooch_0.9.0_aarch64.AppImage"
plain="Scooch-Setup-arm64.exe Scooch-Setup-x64.exe Scooch-aarch64.AppImage Scooch-x86_64.AppImage Scooch.dmg scooch.aarch64.rpm scooch.x86_64.rpm scooch_amd64.deb scooch_arm64.deb"
updates="Scooch-Setup-arm64.exe.sig Scooch-Setup-x64.exe.sig Scooch-aarch64.AppImage.sig Scooch-x86_64.AppImage.sig Scooch.app.tar.gz Scooch.app.tar.gz.sig latest.json scooch.aarch64.rpm.sig scooch.x86_64.rpm.sig scooch_amd64.deb.sig scooch_arm64.deb.sig"

listed() { (cd "$1" && LC_ALL=C ls -1 | LC_ALL=C sort | tr '\n' ' ' | sed 's/ $//'); }
sorted() { printf '%s\n' $1 | LC_ALL=C sort | tr '\n' ' ' | sed 's/ $//'; }

mkdir -p "$work/built"
for name in $names; do echo "$name" >"$work/built/$name"; done
bash "$script" "$work/built" "$work/plain" 0.9.0 >/dev/null
if [ "$(listed "$work/plain")" != "$(sorted "$plain")" ]; then
  echo "Without update keys the release would hold $(listed "$work/plain")"
  exit 1
fi
echo "Without update keys the release holds $(ls "$work/plain" | wc -l | tr -d ' ') files, $(listed "$work/plain")"

echo "stand in" >"$work/built/Scooch.app.tar.gz"
for name in $names Scooch.app.tar.gz; do printf 'dW50cnVzdGVkIGNvbW1lbnQ6IHN0YW5kIGluIGZvciAlcw==%s' "$name" >"$work/built/$name.sig"; done
bash "$script" "$work/built" "$work/keyed" 0.9.0 v0.9.0
if [ "$(listed "$work/keyed")" != "$(sorted "$plain $updates")" ]; then
  echo "With update keys the release would hold $(listed "$work/keyed")"
  exit 1
fi
echo "With update keys the release holds $(ls "$work/keyed" | wc -l | tr -d ' ') files, $(listed "$work/keyed")"

json="$work/keyed/latest.json"
jq -e '.version == "0.9.0" and (.pub_date | test("^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9:]{8}Z$")) and (.platforms | length == 16)' "$json" >/dev/null
for key in $(jq -r '.platforms | keys[]' "$json"); do
  url=$(jq -r --arg key "$key" '.platforms[$key].url' "$json")
  file=${url##*/}
  [ "${url%/*}" = "https://github.com/sohan-bhat/scooch/releases/download/v0.9.0" ] || { echo "$key points at $url"; exit 1; }
  [ -f "$work/keyed/$file" ] || { echo "$key points at $file, which the release does not hold"; exit 1; }
  [ "$(jq -r --arg key "$key" '.platforms[$key].signature' "$json")" = "$(cat "$work/keyed/$file.sig")" ] || { echo "$key carries a signature that is not the one of $file"; exit 1; }
done
echo "latest.json from stand in files:"
jq '{version, platforms: (.platforms | map_values(.url | split("/") | last))}' "$json"
