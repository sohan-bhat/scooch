#!/usr/bin/env bash
set -u

out="$1"
mkdir -p "$out"
report="$out/report.md"
data="${XDG_DATA_HOME:-$HOME/.local/share}/scooch"
photo="$PWD/assets/photos/great-wave.jpg"
here="$PWD/.github/app"
readonly out report data photo here
export WEBKIT_DISABLE_COMPOSITING_MODE=1
export LIBGL_ALWAYS_SOFTWARE=1
: >"$report"

line() { echo "$*" | tee -a "$report"; }
. "$here/expect.sh"
allowed="HOME USER LOGNAME PATH LANG DISPLAY XAUTHORITY XDG_RUNTIME_DIR DBUS_SESSION_BUS_ADDRESS XDG_CURRENT_DESKTOP XDG_SESSION_DESKTOP XDG_SESSION_TYPE NO_AT_BRIDGE WEBKIT_DISABLE_COMPOSITING_MODE LIBGL_ALWAYS_SOFTWARE KDE_FULL_SESSION KDE_SESSION_VERSION"
clean() {
  local pass=() name
  for name in $allowed; do
    if [ -n "${!name:-}" ]; then pass+=("$name=${!name}"); fi
  done
  env -i "${pass[@]}" "$@"
}
wait_for() {
  local limit="$1" waited=0
  shift
  while [ "$waited" -lt "$limit" ]; do
    if "$@"; then
      echo "$waited"
      return 0
    fi
    sleep 1
    waited=$((waited + 1))
  done
  return 1
}
now_ms() { date +%s%3N; }
window() { xdotool search --onlyvisible --name '^Scooch$' 2>/dev/null | head -1; }
has_window() { [ -n "$(window)" ]; }
window_gone() { ! has_window; }
state_on() { grep -q '"on": true' "$data/state.json" 2>/dev/null; }
rendered() { grep -q '"fingerprint": "' "$data/state.json" 2>/dev/null; }
tray() { python3 "$here/tray.py" "$@" 2>&1 | tail -1; }
watcher_there() { [ "$(tray watcher)" = yes ]; }
lock_pid() {
  local inode
  inode=$(stat -c %i "$data/icon.lock" 2>/dev/null) || return 0
  awk -v inode="$inode" '$2 == "FLOCK" { n = split($6, at, ":"); if (at[n] == inode) print $5 }' /proc/locks 2>/dev/null | head -1
}
registered() {
  local pid
  pid=$(lock_pid)
  [ -n "$pid" ] && tray menu "$pid" | grep -q '^Open Scooch'
}
icon_only() { window_gone && registered; }
set_wallpaper() {
  command -v xfconf-query >/dev/null 2>&1 || return 0
  for property in $(xfconf-query -c xfce4-desktop -l 2>/dev/null | grep 'last-image$'); do
    xfconf-query -c xfce4-desktop -p "$property" -s "$1"
  done
}
plasma_answers() {
  dbus-send --session --print-reply --dest=org.kde.plasmashell /PlasmaShell org.kde.PlasmaShell.evaluateScript "string:print(desktops().length)" 2>/dev/null | grep -q 'string "[1-9]'
}
follow_items() {
  python3 - "$1" "$2" <<'PY'
import json
import sys
import time

from gi.repository import Gio, GLib

WATCHER = "org.kde.StatusNotifierWatcher"
STEP = 0.02


def listed(bus):
    try:
        answer = bus.call_sync(WATCHER, "/StatusNotifierWatcher", "org.freedesktop.DBus.Properties", "Get", GLib.Variant("(ss)", (WATCHER, "RegisteredStatusNotifierItems")), GLib.VariantType.new("(v)"), Gio.DBusCallFlags.NONE, 1000, None)
        return list(answer.unpack()[0])
    except GLib.Error:
        return None


def owner(bus, known, entry):
    slash = entry.find("/")
    name = entry if slash < 0 else entry[:slash]
    if name not in known:
        try:
            pid = bus.call_sync("org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "GetConnectionUnixProcessID", GLib.Variant("(s)", (name,)), GLib.VariantType.new("(u)"), Gio.DBusCallFlags.NONE, 1000, None).unpack()[0]
            with open(f"/proc/{pid}/comm") as handle:
                known[name] = (pid, handle.read().strip())
        except (GLib.Error, OSError):
            return (0, "")
    return known[name]


def main():
    seconds, path = float(sys.argv[1]), sys.argv[2]
    bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    known, spans, changes = {}, [], []
    state, since, polls, slowest = None, 0.0, 0, 0.0
    started = time.monotonic()
    while True:
        now = time.monotonic()
        if now - started >= seconds:
            break
        entries = listed(bus)
        slowest = max(slowest, time.monotonic() - now)
        polls += 1
        if entries is None:
            current = "no watcher"
        else:
            current = sorted(pid for pid, comm in (owner(bus, known, entry) for entry in entries) if comm == "scooch-app")
        if current != state:
            if state is not None:
                spans.append((state, since, now))
            changes.append((round((now - started) * 1000), current))
            state, since = current, now
        rest = STEP - (time.monotonic() - now)
        if rest > 0:
            time.sleep(rest)
    spans.append((state, since, time.monotonic()))
    none = round(sum((end - start) * 1000 for held, start, end in spans if held == []))
    two = round(sum((end - start) * 1000 for held, start, end in spans if isinstance(held, list) and len(held) >= 2))
    blind = round(sum((end - start) * 1000 for held, start, end in spans if held == "no watcher"))
    first, last = changes[0][1], changes[-1][1]
    moved = isinstance(first, list) and isinstance(last, list) and len(first) == 1 and len(last) == 1 and first != last
    with open(path, "w") as handle:
        json.dump({"none": none, "two": two, "blind": blind, "moved": moved, "polls": polls, "slowest": round(slowest * 1000, 1), "changes": changes}, handle)


main()
PY
}
read_gap() { python3 -c 'import json, sys; print(json.load(open(sys.argv[1]))[sys.argv[2]])' "$1" "$2" 2>/dev/null; }

line "## The tray while a closed window hands the icon over, on ${XDG_CURRENT_DESKTOP:-no desktop}"
line
line "- scooch $(dpkg-query -W -f '${Version}' scooch 2>/dev/null), libayatana-appindicator3-1 $(dpkg-query -W -f '${Version}' libayatana-appindicator3-1 2>/dev/null), $(plasmashell --version 2>/dev/null | head -1)$(xfce4-panel --version 2>/dev/null | head -1)"
if [ "${XDG_CURRENT_DESKTOP:-}" = KDE ] && ! watcher_there; then
  kded=$(command -v kded5 || command -v kded6 || echo /usr/lib/x86_64-linux-gnu/libexec/kf5/kded5)
  pkill -x plasmashell >/dev/null 2>&1
  pkill -x "$(basename "$kded")" >/dev/null 2>&1
  sleep 2
  "$kded" >"$out/kded.log" 2>&1 &
  plasmashell >"$out/plasmashell-again.log" 2>&1 &
  took=$(wait_for 60 watcher_there) || took=never
  plasma=$(wait_for 60 plasma_answers) || plasma=never
  line "- started $(basename "$kded") and plasmashell together, as a Plasma session does, the tray watcher was there after $took s and the desktop answered after $plasma s"
  sleep 5
fi
expect "the session has a tray watcher" watcher_there
set_wallpaper "$photo"
clean /usr/bin/scooch use "$photo" >"$out/use.txt" 2>&1
on=$(wait_for 30 state_on) || on=never
drawn=$(wait_for 40 rendered) || drawn=never
own=$(wait_for 40 registered) || own=never
line "- scooch use turned Scooch on after $on s and drew after $drawn s, the icon the daemon started registered after $own s, icon process $(lock_pid)"
expect "Scooch is on with its icon in the tray" registered
sleep 3

closer=keys
worst_none=0
worst_two=0
moves=0
for round in 1 2 3 4 5 6 7 8 9 10; do
  wait_for 20 icon_only >/dev/null
  before=$(lock_pid)
  from=$(now_ms)
  clean /usr/bin/scooch-app >>"$out/opens.log" 2>&1 &
  took=none
  while [ $(( $(now_ms) - from )) -lt 30000 ]; do
    if has_window; then
      took=$(( $(now_ms) - from ))
      break
    fi
    sleep 0.02
  done
  sleep 4
  follow_items 8 "$out/gap-$round.json" >"$out/gap-$round.txt" 2>&1 &
  follower=$!
  sleep 1
  id=$(window)
  if [ "$closer" = keys ]; then
    timeout 5 xdotool windowactivate --sync "$id" >/dev/null 2>&1
    timeout 5 xdotool windowfocus --sync "$id" >/dev/null 2>&1
    sleep 0.5
    xdotool key --delay 120 alt+F4
    if ! wait_for 3 window_gone >/dev/null; then
      closer=wmctrl
      line "- close $round: alt+F4 left the window open, so from here wmctrl asks the window manager to close the window"
    fi
  fi
  if [ "$closer" = wmctrl ] && has_window; then
    wmctrl -ic "$id"
    wait_for 10 window_gone >/dev/null
  fi
  wait "$follower"
  none=$(read_gap "$out/gap-$round.json" none)
  two=$(read_gap "$out/gap-$round.json" two)
  moved=$(read_gap "$out/gap-$round.json" moved)
  line "- close $round: the window was mapped $took ms after the start of scooch-app, the icon process was $before and is $(lock_pid), the tray's items, read every 20 ms for 8 s in $(read_gap "$out/gap-$round.json" polls) reads with the slowest at $(read_gap "$out/gap-$round.json" slowest) ms, held no Scooch item for ${none:-unread} ms and two for ${two:-unread} ms, the watcher was away for $(read_gap "$out/gap-$round.json" blind) ms, and changed at $(read_gap "$out/gap-$round.json" changes)"
  [ "${none:-9999}" -gt "$worst_none" ] && worst_none=${none:-9999}
  [ "${two:-9999}" -gt "$worst_two" ] && worst_two=${two:-9999}
  [ "$moved" = True ] && moves=$((moves + 1))
done
wait_for 20 icon_only >/dev/null
line "- over ten closes the tray held no Scooch item for at most $worst_none ms and two for at most $worst_two ms, and $moves closes gave the item to another process"
expect "each of ten closes gave the tray's item to another process" [ "$moves" = 10 ]
expect "the tray held no Scooch item for at most 250 ms at each close" [ "$worst_none" -le 250 ]
expect "the tray never held two Scooch items for more than 250 ms" [ "$worst_two" -le 250 ]
expect "after ten closes the icon is in the tray with no window" icon_only
clean /usr/bin/scooch off >"$out/off.txt" 2>&1
pkill -x scooch-app >/dev/null 2>&1
pkill -x scooch >/dev/null 2>&1
finish
