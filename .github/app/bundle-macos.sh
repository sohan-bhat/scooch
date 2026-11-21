#!/usr/bin/env bash
set -eu

compiled="$1"
root="$(cd "$(dirname "$0")/../.." && pwd)"
app="$root/crates/scooch-app"
. "$root/.github/app/take.sh"

take scooch-app "$app/target/universal-apple-darwin/release/scooch-app"
take scooch "$app/binaries/scooch-universal-apple-darwin"
bundler

keychain="$RUNNER_TEMP/sign.keychain-db"
finish() {
  rm -f "$RUNNER_TEMP/AuthKey.p8" "$RUNNER_TEMP/sign.p12"
  security delete-keychain "$keychain" >/dev/null 2>&1 || true
}
trap finish EXIT

identity="-"
if [ -n "${APPLE_CERTIFICATE:-}" ]; then
  word=$(uuidgen)
  printf '%s' "$APPLE_CERTIFICATE" | base64 --decode >"$RUNNER_TEMP/sign.p12"
  security create-keychain -p "$word" "$keychain"
  security set-keychain-settings -lut 3600 "$keychain"
  security unlock-keychain -p "$word" "$keychain"
  security import "$RUNNER_TEMP/sign.p12" -k "$keychain" -P "$APPLE_CERTIFICATE_PASSWORD" -T /usr/bin/codesign
  security set-key-partition-list -S apple-tool:,apple: -s -k "$word" "$keychain" >/dev/null
  security list-keychains -d user -s "$keychain" $(security list-keychains -d user | tr -d '"')
  identity="$APPLE_SIGNING_IDENTITY"
  if [ -n "${APPLE_API_PRIVATE_KEY:-}" ]; then
    printf '%s' "$APPLE_API_PRIVATE_KEY" >"$RUNNER_TEMP/AuthKey.p8"
    export APPLE_API_KEY_PATH="$RUNNER_TEMP/AuthKey.p8"
  fi
  echo "Signing with the Apple certificate."
else
  echo "No Apple certificate, so the app and its helper get the ad hoc signature."
fi
unset APPLE_API_PRIVATE_KEY
for name in APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD APPLE_SIGNING_IDENTITY APPLE_API_ISSUER APPLE_API_KEY APPLE_ID APPLE_PASSWORD APPLE_TEAM_ID; do
  if [ -z "${!name:-}" ]; then unset "$name"; fi
done
if [ "$identity" != "-" ]; then
  export APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD APPLE_SIGNING_IDENTITY
fi

bash "$root/.github/app/mac-helper.sh" "$identity"
TAURI_BUNDLER_DMG_IGNORE_CI=true bundle --target universal-apple-darwin --bundles app,dmg

made="$app/target/universal-apple-darwin/release/bundle"
mkdir -p "$root/dist"
cp "$made"/dmg/*.dmg "$root/dist/"
COPYFILE_DISABLE=1 tar -czf "$root/dist/Scooch.app.tar.gz" -C "$made/macos" Scooch.app
ls -l "$root/dist"
