#!/usr/bin/env bash
set -eu

dist="$1"
root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"
case "$TAG" in
  v[0-9]*) ;;
  *) echo "No release tag named $TAG."; exit 1 ;;
esac
notes="docs/releases/$TAG.md"
if [ -f "$notes" ]; then
  gh release create "$TAG" "$dist"/* --title "Scooch ${TAG#v}" --notes-file "$notes"
else
  gh release create "$TAG" "$dist"/* --title "Scooch ${TAG#v}" --generate-notes
fi
