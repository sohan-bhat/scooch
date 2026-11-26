#!/usr/bin/env bash
set -u

script="$1"
probe=./target/release/examples/linuxprobe
out=linuxprobe
silent=0
mkdir -p "$out"

session_end() {
    local runtime="$1"
    for pid in $(pgrep -u "$(id -u)"); do
        if tr '\0' '\n' < "/proc/$pid/environ" 2>/dev/null | grep -qx "XDG_RUNTIME_DIR=$runtime"; then
            kill -9 "$pid" 2>/dev/null || true
        fi
    done
    for mount in "$runtime/doc" "$runtime/gvfs"; do
        fusermount3 -uz "$mount" 2>/dev/null || fusermount -uz "$mount" 2>/dev/null || true
    done
    rm -rf "$runtime" 2>/dev/null || true
}

for try in 1 2 3; do
    runtime="$(mktemp -d)"
    chmod 700 "$runtime"
    home="$HOME"
    if [ "$try" -gt 1 ]; then
        home="$(mktemp -d)"
    fi
    code=0
    HOME="$home" XDG_RUNTIME_DIR="$runtime" SCOOCH_PROBE_TRY="$try" SCOOCH_PROBE_SILENT="$silent" \
        dbus-run-session -- bash ".github/linux/$script.sh" "$probe" "$out" || code=$?
    session_end "$runtime"
    if [ "$code" != 75 ]; then
        exit "$code"
    fi
    silent=$((silent + 1))
    echo "try $try: the desktop stopped answering, so a fresh session makes the sections again" | tee -a "$out/silent.txt"
    if [ "$try" -lt 3 ]; then
        mkdir -p "$out/try-$try"
        find "$out" -mindepth 1 -maxdepth 1 ! -name 'try-*' ! -name versions.txt ! -name packages.log ! -name silent.txt -exec mv {} "$out/try-$try/" \;
    fi
done
echo "- **failed** the desktop stopped answering in three sessions in a row, so the probe gave up" | tee -a "$out/report.md"
exit 1
