#!/usr/bin/env bash
set -u

out="$1"
mkdir -p "$out"
report="$out/report.md"
here="$PWD/.github"
data="${XDG_DATA_HOME:-$HOME/.local/share}/scooch"
line() { echo "$*" | tee -a "$report"; }
tray() { python3 "$here/app/tray.py" "$@" 2>&1 | tail -1; }
lock_pid() {
  local inode
  inode=$(stat -c %i "$data/icon.lock" 2>/dev/null) || return 0
  awk -v inode="$inode" '$2 == "FLOCK" { n = split($6, at, ":"); if (at[n] == inode) print $5 }' /proc/locks 2>/dev/null | head -1
}
apps() { ps -o pid=,stat=,args= -C scooch-app 2>/dev/null | tr -s ' ' | tr '\n' ';'; }

line "## A closed window on GNOME with the AppIndicator extension"
line
line "- $(gnome-shell --version), the extension $(gnome-extensions info ubuntu-appindicators@ubuntu.com 2>&1 | grep -E 'Name|State|Enabled|Version' | tr -s ' ' | tr '\n' ' '), the session bus holds the tray watcher $(tray watcher)"
mkdir -p "$data"
printf '{\n  "version": 1,\n  "on": true\n}\n' >"$data/state.json"
NO_AT_BRIDGE=0 GDK_BACKEND=wayland scooch-app >"$out/app.log" 2>&1 &
sleep 15
line "- opened the window with Scooch on, the app says $(grep 'scooch-app' "$out/app.log" | tr '\n' ' '), the tray's items $(tray items)"
line "- the icon process $(lock_pid), its menu $(tray menu "$(lock_pid)"), app processes $(apps)"
for round in 1 2 3 4 5; do
  before=$(lock_pid)
  python3 "$here/lab/follow.py" 8 >"$out/gap-$round.txt" 2>&1 &
  follower=$!
  sleep 1
  pressed=$(python3 "$here/app/controls.py" Close press 2>&1 | tail -1)
  wait "$follower"
  line "- close $round: $pressed, the icon process was ${before:-none} and is $(lock_pid || echo none), its menu $(tray menu "$(lock_pid)"), app processes $(apps)"
  line "- close $round: $(cat "$out/gap-$round.txt")"
  NO_AT_BRIDGE=0 GDK_BACKEND=wayland scooch-app >"$out/open-$round.log" 2>&1 &
  sleep 8
  line "- opening $round after that close: the start says $(tr '\n' ' ' <"$out/open-$round.log"), the window answers $(python3 "$here/app/controls.py" Close states 2>&1 | tail -1), app processes $(apps)"
done
line "- the first process said $(grep 'scooch-app' "$out/app.log" | tr '\n' ' ')"
pkill -x scooch-app >/dev/null 2>&1
exit 0
