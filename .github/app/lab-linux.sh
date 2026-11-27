#!/usr/bin/env bash
set -u

out="$1"
mkdir -p "$out"
report="$out/report.md"
data="${XDG_DATA_HOME:-$HOME/.local/share}/scooch"
photo="$PWD/assets/photos/great-wave.jpg"
here="$PWD/.github/app"
leg="${LAB_LEG:-base}"
plan="${LAB_PLAN:-memory}"
if [ "${LAB_GL:-0}" != 1 ]; then
  export WEBKIT_DISABLE_COMPOSITING_MODE=1
  export LIBGL_ALWAYS_SOFTWARE=1
fi
: >"$report"
: >"$out/data.jsonl"

line() { echo "$*" | tee -a "$report"; }
allowed="HOME USER LOGNAME PATH LANG DISPLAY XAUTHORITY XDG_RUNTIME_DIR DBUS_SESSION_BUS_ADDRESS XDG_CURRENT_DESKTOP XDG_SESSION_DESKTOP XDG_SESSION_TYPE NO_AT_BRIDGE WEBKIT_DISABLE_COMPOSITING_MODE LIBGL_ALWAYS_SOFTWARE KDE_FULL_SESSION KDE_SESSION_VERSION SCOOCH_LAB"
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
now_ms() { echo $(( $(date +%s%N) / 1000000 )); }
window() { xdotool search --onlyvisible --name '^Scooch$' 2>/dev/null | head -1; }
has_window() { [ -n "$(window)" ]; }
window_gone() { ! has_window; }
state_on() { grep -q '"on": true' "$data/state.json" 2>/dev/null; }
rendered() { grep -q '"fingerprint": "' "$data/state.json" 2>/dev/null; }
gone_app() { [ -z "$(pgrep -x scooch-app)" ]; }
tray() { python3 "$here/tray.py" "$@" 2>&1 | tail -1; }
watcher_there() { [ "$(tray watcher)" = yes ]; }
mem() { python3 "$here/lab-mem.py" "$@"; }
icon_pid() {
  local pid
  for pid in $(pgrep -x scooch-app); do
    if ls -l "/proc/$pid/fd" 2>/dev/null | grep -q 'icon\.lock'; then
      echo "$pid"
      return 0
    fi
  done
  return 1
}
registered() {
  local pid
  pid=$(icon_pid) || return 1
  tray menu "$pid" | grep -q '^Open Scooch'
}
set_wallpaper() {
  command -v xfconf-query >/dev/null 2>&1 || return 0
  for property in $(xfconf-query -c xfce4-desktop -l 2>/dev/null | grep 'last-image$'); do
    xfconf-query -c xfce4-desktop -p "$property" -s "$1"
  done
}
lab_of() { tr '\0' '\n' <"/proc/$1/environ" 2>/dev/null | sed -n 's/^SCOOCH_LAB=//p'; }

snap() {
  local pid
  pid=$(icon_pid) || pid=$(pgrep -x scooch-app | head -1)
  line
  line "#### $1"
  if [ -z "$pid" ]; then
    line "- no icon process runs"
    return
  fi
  mem snap "$pid" "$1" "$out" 2>&1 | tee -a "$report"
}
wakes() {
  local pid
  pid=$(icon_pid) || return
  xdotool mousemove 300 300 >/dev/null 2>&1
  mem wakes "$pid" 60 "$1" "$out" 2>&1 | tee -a "$report"
}

opens=0
open_window() {
  local started took=-1 before
  opens=$((opens + 1))
  wait_for 20 registered >/dev/null
  before=$(icon_pid)
  started=$(now_ms)
  clean /usr/bin/scooch-app >>"$out/open-$opens.log" 2>&1 &
  while [ $(( $(now_ms) - started )) -lt 60000 ]; do
    if has_window; then
      took=$(( $(now_ms) - started ))
      break
    fi
    sleep 0.02
  done
  line "- opening $opens: the window was mapped $took ms after the request, a start of scooch-app while the icon process $before runs"
  echo "{\"what\": \"open\", \"number\": $opens, \"ms\": $took, \"pid\": ${before:-0}}" >>"$out/data.jsonl"
}

closes=0
closed_ms=0
close_window() {
  local id old poller
  closes=$((closes + 1))
  old=$(icon_pid)
  python3 "$here/lab-tray.py" 12 "$out/gap-$closes.json" >"$out/gap-$closes.txt" 2>&1 &
  poller=$!
  mem watch "$old" "$1" "$out/watch-$closes.txt" >/dev/null 2>&1 &
  sleep 1
  id=$(window)
  timeout 5 xdotool windowactivate --sync "$id" >/dev/null 2>&1
  timeout 5 xdotool windowfocus --sync "$id" >/dev/null 2>&1
  sleep 0.5
  xdotool key --delay 120 alt+F4
  closed_ms=$(now_ms)
  if ! wait_for 5 window_gone >/dev/null; then
    wmctrl -c Scooch
    closed_ms=$(now_ms)
    line "- close $closes: alt+F4 left the window open, so wmctrl asked the window manager to close it"
    wait_for 10 window_gone >/dev/null
  fi
  wait "$poller"
  line "- close $closes: the window is $(has_window && echo still open || echo gone), the icon process was $old and is $(icon_pid || echo none), app processes $(pgrep -xc scooch-app)"
  cat "$out/gap-$closes.txt" | tee -a "$report"
  if [ -s "$out/gap-$closes.json" ]; then
    echo "{\"what\": \"gap\", \"number\": $closes, \"old\": ${old:-0}, \"gap\": $(cat "$out/gap-$closes.json")}" >>"$out/data.jsonl"
  fi
}
until_close() {
  while [ $(( ($(now_ms) - closed_ms) / 1000 )) -lt "$1" ]; do sleep 0.5; done
}

start_icon() {
  local took
  pkill -x scooch-app >/dev/null 2>&1
  wait_for 10 gone_app >/dev/null
  clean /usr/bin/scooch-app --background >"$out/app${1:-}.log" 2>&1 &
  if took=$(wait_for 40 registered); then
    line "- a background start registered its icon after $took s, icon process $(icon_pid), lab mode in its environment ${SCOOCH_LAB:-none} and read back $(lab_of "$(icon_pid)")"
  else
    line "- a background start registered no icon within 40 s, it says $(tr '\n' ' ' <"$out/app${1:-}.log")"
    exit 1
  fi
}

line "## Memory lab, leg $leg, mode ${SCOOCH_LAB:-unchanged}, on $(. /etc/os-release && echo "$PRETTY_NAME") with ${XDG_CURRENT_DESKTOP:-no desktop}"
line
line "- scooch $(dpkg-query -W -f '${Version}' scooch 2>/dev/null), libwebkit2gtk-4.1-0 $(dpkg-query -W -f '${Version}' libwebkit2gtk-4.1-0 2>/dev/null), libayatana-appindicator3-1 $(dpkg-query -W -f '${Version}' libayatana-appindicator3-1 2>/dev/null), kernel $(uname -r), $(nproc) processors"
line "- WebKit's compositing mode is $([ -n "${WEBKIT_DISABLE_COMPOSITING_MODE:-}" ] && echo 'disabled and GL is software, as in the probe' || echo 'left as WebKit chooses')"
if [ "${XDG_CURRENT_DESKTOP:-}" = KDE ] && ! watcher_there; then
  kded=$(command -v kded5 || command -v kded6 || echo /usr/lib/x86_64-linux-gnu/libexec/kf5/kded5)
  "$kded" >"$out/kded.log" 2>&1 &
  took=$(wait_for 60 watcher_there) || took=never
  line "- started $kded so the session has a tray watcher, there after $took s"
fi
set_wallpaper "$photo"
clean /usr/bin/scooch use "$photo" >"$out/use.txt" 2>&1
on=$(wait_for 30 state_on) || on=never
drawn=$(wait_for 40 rendered) || drawn=never
own=$(wait_for 40 registered) || own=never
line "- scooch use turned Scooch on after $on s and drew after $drawn s, daemon $(pgrep -x scooch | tr '\n' ' '), the daemon's own icon registered after $own s, the session bus holds the tray watcher $(tray watcher), items $(tray items)"
sleep 3

if [ "$plan" = gap ]; then
  for SCOOCH_LAB in $LAB_MODES; do
    export SCOOCH_LAB
    line
    line "### Handing the icon over, mode $SCOOCH_LAB"
    start_icon "-$SCOOCH_LAB"
    sleep 5
    for round in 1 2 3 4 5; do
      open_window
      sleep 6
      close_window 20
      until_close 16
    done
  done
  pkill -x scooch-app >/dev/null 2>&1
  pkill -x scooch >/dev/null 2>&1
  exit 0
fi

line
line "### No window ever"
start_icon
sleep 20
snap "no window ever, 20 s after the background start"
wakes "no window ever"

line
line "### The window open"
open_window
sleep 20
snap "window open for 20 s"
wakes "window open"

line
line "### After the window closed"
close_window 300
until_close 30
snap "30 s after the close"
until_close 65
snap "65 s after the close"
wakes "65 s to 125 s after the close"
until_close 240
wakes "240 s to 300 s after the close"
until_close 300
snap "300 s after the close"
line "- what ended after the first close: $(tr '\n' ';' <"$out/watch-1.txt")"

line
line "### Five windows"
for round in 2 3 4 5; do
  open_window
  sleep 10
  close_window 60
  until_close 16
done
until_close 65
snap "65 s after the fifth close"
wakes "65 s to 125 s after the fifth close"
snap "125 s after the fifth close"
line "- what ended after the fifth close: $(tr '\n' ';' <"$out/watch-5.txt")"

line
line "### What the app said"
line "- $(grep -c 'window closed, the icon stays' "$out/app.log") times that the window closed and the icon stays"
grep ' lab ' "$out/app.log" | sed 's/^/- /' | tee -a "$report"
pkill -x scooch-app >/dev/null 2>&1
pkill -x scooch >/dev/null 2>&1
exit 0
