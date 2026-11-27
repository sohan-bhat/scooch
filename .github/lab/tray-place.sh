#!/usr/bin/env bash
set -u

out="$1"
mkdir -p "$out"
report="$out/report.md"
here="$PWD/.github"
data="${XDG_DATA_HOME:-$HOME/.local/share}/scooch"
photo="$PWD/assets/photos/great-wave.jpg"
export WEBKIT_DISABLE_COMPOSITING_MODE=1
export LIBGL_ALWAYS_SOFTWARE=1
line() { echo "$*" | tee -a "$report"; }
tray() { python3 "$here/app/tray.py" "$@" 2>&1 | tail -1; }
panel() { env NO_AT_BRIDGE=0 python3 "$here/app/panel.py" "$@" 2>&1 | tail -1; }
wait_for() {
  local limit="$1" waited=0
  shift
  while [ "$waited" -lt "$limit" ]; do
    if "$@"; then return 0; fi
    sleep 1
    waited=$((waited + 1))
  done
  return 1
}
lock_pid() {
  local inode
  inode=$(stat -c %i "$data/icon.lock" 2>/dev/null) || return 0
  awk -v inode="$inode" '$2 == "FLOCK" { n = split($6, at, ":"); if (at[n] == inode) print $5 }' /proc/locks 2>/dev/null | head -1
}
registered() { local pid; pid=$(lock_pid); [ -n "$pid" ] && tray menu "$pid" | grep -q '^Open Scooch'; }
window() { xdotool search --onlyvisible --name '^Scooch$' 2>/dev/null | head -1; }
has_window() { [ -n "$(window)" ]; }
icon_only() { ! has_window && registered; }
other_there() { tray items | grep -q 'probe-other'; }
places() {
  local entry spot shown told=""
  for entry in $(panel buttons | tr ';' '\n' | grep ' push button||$' | cut -d' ' -f1); do
    spot=$(echo "$entry" | awk -F, '$3 > 0 && $3 < 60 { printf "%d %d", $1 + $3 / 2, $2 + $4 / 2 }')
    [ -n "$spot" ] || continue
    xdotool mousemove $spot click 1
    sleep 1.5
    shown=$(panel menu)
    xdotool key Escape
    sleep 0.5
    case "$shown" in
      *"Open Scooch"*) told="$told Scooch at x ${spot% *};" ;;
      *"Probe other"*) told="$told the other item at x ${spot% *};" ;;
      *) told="$told an item with the menu $shown at x ${spot% *};" ;;
    esac
  done
  echo "${told:- no small button of the panel answered}"
}
hand_over() {
  local before
  before=$(lock_pid)
  scooch-app >>"$out/opens.log" 2>&1 &
  wait_for 30 has_window
  sleep 3
  xdotool windowactivate --sync "$(window)" >/dev/null 2>&1
  sleep 0.5
  xdotool key --delay 120 alt+F4
  wait_for 20 icon_only
  sleep 3
  echo "the icon process was $before and is $(lock_pid)"
}
other_item() {
  python3 - <<'PY' >"$out/other.log" 2>&1 &
import gi

gi.require_version("Gtk", "3.0")
gi.require_version("AyatanaAppIndicator3", "0.1")
from gi.repository import AyatanaAppIndicator3, Gtk

indicator = AyatanaAppIndicator3.Indicator.new("probe-other", "dialog-information", AyatanaAppIndicator3.IndicatorCategory.APPLICATION_STATUS)
menu = Gtk.Menu()
item = Gtk.MenuItem(label="Probe other")
menu.append(item)
menu.show_all()
indicator.set_menu(menu)
indicator.set_status(AyatanaAppIndicator3.IndicatorStatus.ACTIVE)
Gtk.main()
PY
  other=$!
  wait_for 20 other_there
  sleep 3
}

line "## The place of the Scooch icon among tray icons on Xfce, before and after a hand over"
line
xfce4-panel --quit >/dev/null 2>&1
sleep 3
env NO_AT_BRIDGE=0 xfce4-panel >"$out/panel-a11y.log" 2>&1 &
wait_for 30 sh -c '[ "$(python3 "'"$here"'/app/tray.py" watcher 2>/dev/null)" = yes ]'
for property in $(xfconf-query -c xfce4-desktop -l 2>/dev/null | grep 'last-image$'); do xfconf-query -c xfce4-desktop -p "$property" -s "$photo"; done
scooch use "$photo" >"$out/use.txt" 2>&1
wait_for 40 registered
sleep 3
line "- $(xfce4-panel --version 2>/dev/null | head -1), Scooch on, the tray's items $(tray items)"

line
line "### Scooch registered first, another item second"
other_item
line "- the tray's items $(tray items)"
line "- before the hand over:$(places)"
line "- $(hand_over)"
line "- after the hand over:$(places)"
line "- $(hand_over)"
line "- after a second hand over:$(places)"
kill "$other" >/dev/null 2>&1
pkill -x scooch-app >/dev/null 2>&1
sleep 3

line
line "### Another item registered first, Scooch second"
other_item
scooch-app --background >"$out/background.log" 2>&1 &
wait_for 40 registered
sleep 3
line "- the tray's items $(tray items)"
line "- before the hand over:$(places)"
line "- $(hand_over)"
line "- after the hand over:$(places)"
line "- the panel remembers the items as $(xfconf-query -c xfce4-panel -lv 2>/dev/null | grep -i -E 'known|hidden' | tr '\n' ' ')"
kill "$other" >/dev/null 2>&1
scooch off >"$out/off.txt" 2>&1
pkill -x scooch-app >/dev/null 2>&1
exit 0
