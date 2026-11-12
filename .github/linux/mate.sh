#!/usr/bin/env bash
. "$(dirname "$0")/common.sh"

start_xvfb
export XDG_CURRENT_DESKTOP=MATE
export XDG_SESSION_DESKTOP=mate

marco >"$out/marco.log" 2>&1 &
sleep 2
mate-panel >"$out/panel.log" 2>&1 &
caja --force-desktop --no-default-window >"$out/caja.log" 2>&1 &

run_probe 15
