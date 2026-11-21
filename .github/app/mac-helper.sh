#!/usr/bin/env bash
set -eu

identity="$1"
root="$(cd "$(dirname "$0")/../.." && pwd)"
app="$root/crates/scooch-app"
version=$(grep -m1 '^version' "$app/Cargo.toml" | cut -d'"' -f2)
minimum=$(grep -m1 '"minimumSystemVersion"' "$app/tauri.conf.json" | cut -d'"' -f4)
helper="$app/target/helper/Scooch.app"

rm -rf "$helper"
mkdir -p "$helper/Contents/MacOS" "$helper/Contents/Resources"
cp "$app/binaries/scooch-universal-apple-darwin" "$helper/Contents/MacOS/scooch"
chmod 755 "$helper/Contents/MacOS/scooch"
cp "$app/icons/icon.icns" "$helper/Contents/Resources/icon.icns"
sed -e "s/@VERSION@/$version/g" -e "s/@MINIMUM@/$minimum/g" "$app/macos/Helper.plist" >"$helper/Contents/Info.plist"
plutil -lint "$helper/Contents/Info.plist" >/dev/null

stamp=()
if [ "$identity" != "-" ]; then stamp=(--timestamp); fi
codesign --force --sign "$identity" ${stamp[@]+"${stamp[@]}"} --options runtime --entitlements "$app/macos/Entitlements.plist" "$helper"
codesign --verify --strict "$helper"
echo "helper $(codesign -dv "$helper" 2>&1 | grep -E '^Identifier=|^Info.plist' | tr '\n' ' ')"
