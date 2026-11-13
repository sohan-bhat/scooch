#!/usr/bin/env bash
. "$(dirname "$0")/common.sh"

export XDG_CURRENT_DESKTOP=KDE
export XDG_SESSION_DESKTOP=KDE
export XDG_SESSION_TYPE=wayland
export KDE_FULL_SESSION=true
KDE_SESSION_VERSION="$(plasmashell --version 2>/dev/null | sed -E 's/[^0-9]*([0-9]+).*/\1/')"
export KDE_SESSION_VERSION
unset DISPLAY
dbus-update-activation-environment XDG_CURRENT_DESKTOP XDG_SESSION_TYPE KDE_FULL_SESSION KDE_SESSION_VERSION >/dev/null 2>&1 || true

outputs=1
if [ "${SCOOCH_MONITORS:-1}" = "2" ]; then
    outputs=2
fi
kwin_wayland --virtual --width 1920 --height 1080 --output-count "$outputs" --no-lockscreen --no-global-shortcuts --socket wayland-0 >"$out/kwin.log" 2>&1 &

wayland_socket() {
    [ -S "$XDG_RUNTIME_DIR/wayland-0" ]
}

ready "KWin Wayland socket" 60 wayland_socket
export WAYLAND_DISPLAY=wayland-0
dbus-update-activation-environment WAYLAND_DISPLAY >/dev/null 2>&1 || true
plasmashell >"$out/plasmashell.log" 2>&1 &

plasma_answers() {
    dbus-send --session --print-reply --dest=org.kde.plasmashell /PlasmaShell org.kde.PlasmaShell.evaluateScript "string:print(desktops().length)" 2>/dev/null | grep -q 'string "[1-9]'
}

ready "Plasma shell" 120 owned org.kde.plasmashell
ready "Plasma desktop containment" 60 plasma_answers
sort_mode="$(dbus-send --session --print-reply --dest=org.kde.plasmashell /PlasmaShell org.kde.PlasmaShell.evaluateScript "string:desktops().forEach(function (d) { d.currentConfigGroup = ['General']; d.writeConfig('sortMode', -1); }); print(desktops()[0].readConfig('sortMode'));" 2>&1 | sed -n 's/.*string "\(.*\)"/\1/p')"
echo "Plasma folder sort mode now ${sort_mode:-unknown}, so icons stay where they are dropped" >>"$out/readiness.txt"
run_probe 15
