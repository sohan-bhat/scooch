#!/usr/bin/env bash
. "$(dirname "$0")/common.sh"

scale="${SCOOCH_SCALE:-1}"
if [ "$scale" = "2" ]; then
    start_xvfb 3840x2160
else
    start_xvfb
fi
export XDG_CURRENT_DESKTOP=XFCE
export XDG_SESSION_DESKTOP=xfce

if [ "$scale" != "1" ]; then
    xfconf-query -c xsettings -p /Gdk/WindowScalingFactor -n -t int -s "$scale" || true
    xfsettingsd >"$out/xfsettingsd.log" 2>&1 &
    ready "Xfce settings daemon" 60 owned org.xfce.SettingsDaemon
    sleep 1
fi

xfwm4 >"$out/xfwm4.log" 2>&1 &
sleep 2
xfce4-panel >"$out/panel.log" 2>&1 &
xfdesktop >"$out/xfdesktop.log" 2>&1 &

ready "Xfconf" 60 owned org.xfce.Xfconf
ready "work area" 60 workarea_set
ready "xfdesktop window" 60 window_listed '"Desktop":'
run_probe 10
