#!/usr/bin/env bash
set -u
out="$1"
here="$(dirname "$0")"
if [ -n "${SCOOCH_REPLACE_FIRST:-}" ]; then
    python3 "$here/replace.py" "$out" "${SCOOCH_REPLACES:-100}" >"$out/replace-console.txt" 2>&1
fi
exec bash "$here/writes.sh" "$out"
