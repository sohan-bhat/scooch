#!/usr/bin/env bash
set -u

out="$1"
mkdir -p "$out"
report="$out/report.md"
line() { echo "$*" | tee -a "$report"; }
. "$(dirname "$0")/expect.sh"
owned() { dbus-send --session --print-reply --dest=org.freedesktop.DBus /org/freedesktop/DBus org.freedesktop.DBus.NameHasOwner "string:$1" 2>/dev/null | grep -q "boolean true"; }

problem_with() {
  local log="$out/app-$1.log"
  GDK_BACKEND=wayland scooch-app >"$log" 2>&1 &
  local app=$!
  sleep 15
  line "- the app says $(grep 'scooch-app' "$log" | tr '\n' ' ')"
  kill "$app" >/dev/null 2>&1
  wait "$app" 2>/dev/null
  grep -o 'problem {[^}]*}' "$log" | head -1
}

line "## GNOME with Desktop Icons NG ${PASS}"
line
if [ "$PASS" = missing ]; then
  line "- the extension $(gnome-extensions info ding@rastersoft.com 2>&1 | head -1)"
  said=$(problem_with missing)
  expect "with Desktop Icons NG not installed the window says it is missing" grep -q '"icons":"missing"' <<<"$said"
  . "$(dirname "$0")/tray-gnome.sh"
else
  gnome-extensions disable ding@rastersoft.com >/dev/null 2>&1
  sleep 4
  line "- the extension $(gnome-extensions info ding@rastersoft.com 2>&1 | grep -E 'State|Enabled' | tr -s ' ' | tr '\n' ' '), its window running $(owned com.rastersoft.dingextension && echo yes || echo no)"
  said=$(problem_with off)
  expect "with Desktop Icons NG installed and off the window offers to turn it on" grep -q '"icons":"off"' <<<"$said"
  gsettings set org.gnome.shell disable-user-extensions true
  sleep 4
  line "- user extensions turned off, the extension $(gnome-extensions info ding@rastersoft.com 2>&1 | grep -E 'State|Enabled' | tr -s ' ' | tr '\n' ' ')"
  said=$(problem_with blocked)
  expect "with user extensions turned off the window says so and offers no button" grep -q '"icons":"blocked"' <<<"$said"
  gsettings set org.gnome.shell disable-user-extensions false
fi
finish
