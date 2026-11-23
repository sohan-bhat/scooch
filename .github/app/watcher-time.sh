#!/usr/bin/env bash
set -u

desktop="$1"
out="$2"
mkdir -p "$out"
report="$out/report.md"
: >"$report"
line() { echo "$*" | tee -a "$report"; }
owned() { dbus-send --session --print-reply --dest=org.freedesktop.DBus /org/freedesktop/DBus org.freedesktop.DBus.NameHasOwner "string:$1" 2>/dev/null | grep -q "boolean true"; }
owner() {
  local pid
  pid=$(dbus-send --session --print-reply --dest=org.freedesktop.DBus /org/freedesktop/DBus org.freedesktop.DBus.GetConnectionUnixProcessID "string:$1" 2>/dev/null | awk '/uint32/ { print $2 }')
  [ -n "$pid" ] && echo "$(cat "/proc/$pid/comm" 2>/dev/null) $pid"
}
name=org.kde.StatusNotifierWatcher

Xvfb :99 -screen 0 1920x1080x24 -nolisten tcp >"$out/xvfb.log" 2>&1 &
export DISPLAY=:99
export XDG_SESSION_TYPE=x11
for step in $(seq 1 50); do xdpyinfo >/dev/null 2>&1 && break; sleep 0.2; done
case "$desktop" in
  xfce)
    export XDG_CURRENT_DESKTOP=XFCE XDG_SESSION_DESKTOP=xfce
    xfwm4 >"$out/wm.log" 2>&1 &
    start_panel() { xfce4-panel >>"$out/panel.log" 2>&1 & }
    stop_panel() { xfce4-panel --quit >/dev/null 2>&1; }
    version="Xfce panel $(xfce4-panel --version 2>/dev/null | head -1 | grep -o '[0-9][0-9.]*' | head -1)"
    ;;
  kde)
    export XDG_CURRENT_DESKTOP=KDE XDG_SESSION_DESKTOP=KDE KDE_FULL_SESSION=true
    KDE_SESSION_VERSION="$(plasmashell --version 2>/dev/null | sed -E 's/[^0-9]*([0-9]+).*/\1/')"
    export KDE_SESSION_VERSION
    kwin_x11 >"$out/wm.log" 2>&1 &
    start_panel() { plasmashell >>"$out/panel.log" 2>&1 & }
    stop_panel() { pkill -x plasmashell >/dev/null 2>&1; pkill -f kded >/dev/null 2>&1; }
    version="$(plasmashell --version 2>/dev/null | head -1)"
    ;;
esac
dbus-update-activation-environment DISPLAY XDG_SESSION_TYPE XDG_CURRENT_DESKTOP >/dev/null 2>&1 || true
sleep 2
line "## How late the tray watcher comes on $desktop, $version"
line
line "The clock starts when the panel process starts, the moment a session also starts its autostart entries, and stops when $name has an owner on the session bus."
line
for run in 1 2 3; do
  started=$(date +%s%N)
  start_panel
  took=""
  while [ $(( ($(date +%s%N) - started) / 1000000 )) -lt 60000 ]; do
    if owned "$name"; then took=$(( ($(date +%s%N) - started) / 1000000 )); break; fi
    sleep 0.05
  done
  line "- run $run, $([ "$run" = 1 ] && echo cold || echo warm), the watcher came after ${took:-more than 60000} ms, owned by $(owner "$name")"
  sleep 5
  stop_panel
  for step in $(seq 1 100); do owned "$name" || break; sleep 0.1; done
  sleep 2
done
