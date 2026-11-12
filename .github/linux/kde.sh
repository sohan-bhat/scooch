#!/usr/bin/env bash
. "$(dirname "$0")/common.sh"

start_xvfb
export XDG_CURRENT_DESKTOP=KDE
export XDG_SESSION_DESKTOP=KDE
export KDE_FULL_SESSION=true
export KDE_SESSION_VERSION="$(plasmashell --version 2>/dev/null | sed -E 's/[^0-9]*([0-9]+).*/\1/')"

kwin_x11 >"$out/kwin.log" 2>&1 &
sleep 3
plasmashell >"$out/plasmashell.log" 2>&1 &

wait_for_name org.kde.plasmashell 120
run_probe 15
