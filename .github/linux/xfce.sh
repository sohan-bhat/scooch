#!/usr/bin/env bash
. "$(dirname "$0")/common.sh"

start_xvfb
export XDG_CURRENT_DESKTOP=XFCE
export XDG_SESSION_DESKTOP=xfce

xfwm4 >"$out/xfwm4.log" 2>&1 &
sleep 2
xfce4-panel >"$out/panel.log" 2>&1 &
xfdesktop >"$out/xfdesktop.log" 2>&1 &

wait_for_name org.xfce.Xfconf 60
waited=0
until xprop -root _NET_WORKAREA >/dev/null 2>&1 || [ "$waited" -ge 30 ]; do
    sleep 1
    waited=$((waited + 1))
done
run_probe 10
