#!/usr/bin/env bash
. "$(dirname "$0")/common.sh"

start_xvfb
export XDG_CURRENT_DESKTOP=KDE
export XDG_SESSION_DESKTOP=KDE
export KDE_FULL_SESSION=true
KDE_SESSION_VERSION="$(plasmashell --version 2>/dev/null | sed -E 's/[^0-9]*([0-9]+).*/\1/')"
export KDE_SESSION_VERSION

if command -v kwin_x11 >/dev/null 2>&1; then
    kwin_x11 >"$out/kwin.log" 2>&1 &
elif command -v openbox >/dev/null 2>&1; then
    openbox >"$out/kwin.log" 2>&1 &
    echo "kwin_x11 is missing, so openbox manages the windows" >>"$out/readiness.txt"
fi
sleep 2
plasmashell >"$out/plasmashell.log" 2>&1 &

plasma_answers() {
    dbus-send --session --print-reply --dest=org.kde.plasmashell /PlasmaShell org.kde.PlasmaShell.evaluateScript "string:print(desktops().length)" 2>/dev/null | grep -q 'string "[1-9]'
}

ready "Plasma shell" 120 owned org.kde.plasmashell
ready "Plasma desktop containment" 60 plasma_answers
ready "work area" 60 workarea_set
sort_mode="$(dbus-send --session --print-reply --dest=org.kde.plasmashell /PlasmaShell org.kde.PlasmaShell.evaluateScript "string:desktops().forEach(function (d) { d.currentConfigGroup = ['General']; d.writeConfig('sortMode', -1); }); print(desktops()[0].readConfig('sortMode'));" 2>&1 | sed -n 's/.*string "\(.*\)"/\1/p')"
echo "Plasma folder sort mode now ${sort_mode:-unknown}, so icons stay where they are dropped" >>"$out/readiness.txt"
run_probe 15
