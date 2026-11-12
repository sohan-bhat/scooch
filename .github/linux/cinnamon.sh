#!/usr/bin/env bash
. "$(dirname "$0")/common.sh"

start_xvfb
export XDG_CURRENT_DESKTOP=X-Cinnamon
export XDG_SESSION_DESKTOP=cinnamon

cinnamon --replace >"$out/cinnamon.log" 2>&1 &
sleep 5
nemo-desktop >"$out/nemo.log" 2>&1 &

wait_for_name org.Cinnamon 90
run_probe 10
