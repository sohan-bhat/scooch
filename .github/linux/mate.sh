#!/usr/bin/env bash
. "$(dirname "$0")/common.sh"

start_xvfb
export XDG_CURRENT_DESKTOP=MATE
export XDG_SESSION_DESKTOP=mate

gsettings set org.mate.background show-desktop-icons true || true

marco >"$out/marco.log" 2>&1 &
sleep 2
mate-panel >"$out/panel.log" 2>&1 &
caja --force-desktop --no-default-window >"$out/caja.log" 2>&1 &

ready "work area" 60 workarea_set
ready "Caja desktop window" 60 window_listed '"Caja")'
run_probe 10
