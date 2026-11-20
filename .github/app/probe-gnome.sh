#!/usr/bin/env bash
set -u

out="$1"
mkdir -p "$out"
report="$out/report.md"
line() { echo "$*" | tee -a "$report"; }
owned() { dbus-send --session --print-reply --dest=org.freedesktop.DBus /org/freedesktop/DBus org.freedesktop.DBus.NameHasOwner "string:$1" 2>/dev/null | grep -q "boolean true"; }

line "## GNOME with Desktop Icons NG ${PASS}"
line
if [ "$PASS" = off ]; then
  gnome-extensions disable ding@rastersoft.com >/dev/null 2>&1
  sleep 4
fi
line "- the extension $(gnome-extensions info ding@rastersoft.com 2>&1 | grep -E 'State|Enabled' | tr -s ' ' | tr '\n' ' '), its window running $(owned com.rastersoft.dingextension && echo yes || echo no)"
GDK_BACKEND=wayland scooch-app >"$out/app-$PASS.log" 2>&1 &
app=$!
sleep 15
line "- the app says $(grep 'scooch-app' "$out/app-$PASS.log" | tr '\n' ' ')"
if [ "$PASS" = off ]; then
  answer=$(gdbus call --session --dest org.gnome.Shell.Extensions --object-path /org/gnome/Shell/Extensions --method org.gnome.Shell.Extensions.EnableExtension ding@rastersoft.com 2>&1)
  sleep 6
  line "- EnableExtension, the call behind Turn them on, answered $answer, the extension window running $(owned com.rastersoft.dingextension && echo yes || echo no)"
fi
kill "$app" >/dev/null 2>&1
exit 0
