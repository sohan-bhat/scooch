line
line "### The tray icon"
app_pid() { pgrep -x scooch-app | head -1; }
app_count() { pgrep -xc scooch-app; }
app_alive() { [ -n "$(app_pid)" ]; }
window_gone() { ! has_window; }
tray() { python3 "$PWD/.github/app/tray.py" "$@" 2>&1 | tail -1; }
panel() { env NO_AT_BRIDGE=0 python3 "$PWD/.github/app/panel.py" "$@" 2>&1 | tail -1; }
registered() { tray menu "$(app_pid)" | grep -q '^Open Scooch'; }
checked_on() { tray menu "$(app_pid)" | grep -q '|On checked|'; }
reads_menu() { [ "$(tray menu "$(app_pid)" | sed 's/|On \(un\)\{0,1\}checked|/|On|/')" = "Open Scooch|On|separator|Quit Scooch" ]; }
switches() { cat /proc/"$1"/task/*/status 2>/dev/null | awk '/ctxt_switches/ { s += $2 } END { print s + 0 }'; }
threads() { for task in /proc/"$1"/task/*; do printf '%s:%s ' "$(cat "$task/comm" 2>/dev/null)" "$(awk '/ctxt_switches/ { s += $2 } END { print s + 0 }' "$task/status" 2>/dev/null)"; done; }
cpu_ms() { awk -v tick="$(getconf CLK_TCK)" '{ print int(($14 + $15) * 1000 / tick) }' "/proc/$1/stat" 2>/dev/null; }
memory() { awk '/^Rss:/ { r = $2 } /^Pss:/ { p = $2 } /^Private_(Clean|Dirty):/ { u += $2 } END { printf "%.1f MB resident, %.1f MB proportional, %.1f MB private", r / 1024, p / 1024, u / 1024 }' "/proc/$1/smaps_rollup" 2>/dev/null; }
no_zombie() { [ -n "$watcher_pid" ] && kill -0 "$watcher_pid" 2>/dev/null && ! ps -o stat= --ppid "$watcher_pid" | grep -q Z; }
not_child() { [ -n "$icon" ] && [ -n "$watcher_pid" ] && [ "$(ps -o ppid= -p "$icon" | tr -d ' ')" != "$watcher_pid" ]; }

line "- the session bus holds org.kde.StatusNotifierWatcher $(tray watcher), items $(tray items)"
wait_for 15 window_gone >/dev/null
sleep 3
line "- closed the window, window $(has_window && echo open || echo closed), app processes $(app_count), its menu $(tray menu "$(app_pid)")"
expect "closing the window while Scooch is on leaves the app process on a desktop with a tray" app_alive
expect "closing the window removes the window" window_gone
expect "the app's icon is registered with the tray" registered
pid=$(app_pid)
before=$(switches "$pid")
cpu=$(cpu_ms "$pid")
start_threads=$(threads "$pid")
sleep 60
quiet=$(( $(switches "$pid") - before ))
line "- over a quiet minute with no window the icon process $pid switched $quiet times over all its threads and used $(( $(cpu_ms "$pid") - cpu )) ms of CPU, and holds $(memory "$pid")"
line "- its threads as name and switches before $start_threads"
line "- and after $(threads "$pid")"
expect "the icon process switched at most 6 times in a quiet minute" [ "$quiet" -le 6 ]

line
line "#### The panel restarts with accessibility"
xfce4-panel --quit >/dev/null 2>&1
sleep 3
env NO_AT_BRIDGE=0 xfce4-panel >"$out/panel-a11y.log" 2>&1 &
wait_for 30 sh -c '[ "$(python3 "'"$PWD"'/.github/app/tray.py" watcher 2>/dev/null)" = yes ]' >/dev/null
wait_for 20 registered >/dev/null
sleep 3
line "- the panel came back with accessibility, the watcher $(tray watcher), the icon registered again $(registered && echo yes || echo no)"
expect "the icon registers again after the panel restarts" registered
pkill -x scooch-app >/dev/null 2>&1
wait_for 10 sh -c '! pgrep -x scooch-app >/dev/null' >/dev/null
sleep 2
without=$(panel buttons)
clean /usr/bin/scooch-app --background >"$out/background-on.log" 2>&1 &
if took=$(wait_for 30 registered); then line "- a background start with Scooch on registered its icon after $took s"; else line "- a background start registered no icon within 30 s, it says $(tr '\n' ' ' <"$out/background-on.log")"; fi
sleep 3
with=$(panel buttons)
line "- the panel's buttons before $without"
line "- and after $with"
expect "a background start with Scooch on makes the icon" registered
expect "a background start with Scooch on opens no window" window_gone
spot=$(comm -13 <(tr ';' '\n' <<<"$without" | sort) <(tr ';' '\n' <<<"$with" | sort) | head -1 | awk '{ split($1, b, ","); printf "%d %d", b[1] + b[3] / 2, b[2] + b[4] / 2 }')
line "- the icon's button in the panel sits at ${spot:-no place the probe could find}"

panel_menu() {
  [ -n "$spot" ] || return 1
  xdotool mousemove $spot click 1
  sleep 2
  panel menu | grep -q 'Open Scooch' && return 0
  xdotool key Escape
  sleep 1
  xdotool mousemove $spot click 3
  sleep 2
  panel menu | grep -q 'Open Scooch'
}

press_item() {
  local how
  if panel_menu; then
    line "- a press on the icon opened the panel's menu, it reads $(panel menu)"
    how=$(panel press "$1")
    line "- $how"
    if grep -q '^pressed' <<<"$how"; then sleep 2; return 0; fi
    xdotool key Escape
  else
    line "- a press on the icon opened no menu the probe could read through AT-SPI, it reads $(panel menu)"
  fi
  line "- $(tray press "$(app_pid)" "$1"), the menu the panel shows"
  sleep 2
}

line
line "#### Open Scooch from the menu"
line "- the item's menu reads $(tray menu "$(app_pid)")"
expect "the menu reads Open Scooch, On and Quit Scooch" reads_menu
expect "On is checked while Scooch is on" checked_on
press_item 'Open Scooch'
if took=$(wait_for 20 has_window); then line "- the window came after $took s"; else line "- no window within 20 s"; fi
expect "Open Scooch in the menu opens the window" has_window

line
line "#### On in the menu turns Scooch off and on with the window open"
press_item On
wait_for 30 state_off >/dev/null
sleep 3
line "- after On, wallpaper $(wallpaper), login item $(login), the icon registered $(registered && echo yes || echo no), its menu $(tray menu "$(app_pid)")"
expect "On in the menu turned Scooch off" state_off
expect "off from the menu put the photo back" [ "$(wallpaper)" = "$photo" ]
expect "with the window open the icon stays while Scooch is off" registered
expect "On is not checked while Scooch is off" eval '! checked_on'
press_item On
wait_for 30 state_on >/dev/null
wait_for 30 rendered >/dev/null
sleep 3
line "- after On again, wallpaper $(wallpaper), daemons $(daemons), app processes $(app_count)"
expect "On in the menu turned Scooch on again" state_on
expect "on from the menu drew the desktop" [ "$(wallpaper)" != "$photo" ]
expect "the daemon that On started left one app process" [ "$(app_count)" = 1 ]

line
line "#### Opening Scooch while only the icon runs"
press_pill alt+F4
wait_for 15 window_gone >/dev/null
sleep 2
before=$(app_pid)
clean /usr/bin/scooch-app >"$out/app-again.log" 2>&1 &
if took=$(wait_for 20 has_window); then line "- starting scooch-app showed the window after $took s"; else line "- no window within 20 s"; fi
sleep 3
line "- app processes $(app_count), the same one $([ "$(app_pid)" = "$before" ] && echo yes || echo no)"
expect "opening Scooch while the icon runs shows the window" has_window
expect "opening Scooch while the icon runs keeps one app process" [ "$(app_count)" = 1 ]
costs=""
for run in 1 2 3; do
  costs="$costs $( { TIMEFORMAT='%R %U %S'; time clean /usr/bin/scooch-app --background >/dev/null 2>&1; } 2>&1 | tail -1 | awk '{ printf "%d/%d", $1 * 1000, ($2 + $3) * 1000 }')"
done
line "- three starts with the hidden argument while the app runs, each as wall ms over CPU ms:$costs"
expect "background starts while the app runs leave one app process" [ "$(app_count)" = 1 ]
press_pill alt+F4
wait_for 15 window_gone >/dev/null
sleep 2

line
line "#### Quit Scooch"
sentences=$(python3 -c 'import json, sys; print("\n".join(json.load(open(sys.argv[1]))["failure"].values()))' crates/scooch-app/ui/src/copy/en.json)
said_first() { tray menu "$(app_pid)" | cut -d'|' -f1 | sed 's/ dim$//' | grep -qxF -f <(printf '%s\n' "$sentences"); }
chmod a-w "$data"
press_item 'Quit Scooch'
sleep 4
line "- with the data folder read only, Quit left app processes $(app_count), saved on $(grep -c '"on": true' "$data/state.json"), the menu reads $(tray menu "$(app_pid)")"
chmod u+w "$data"
expect "a Quit whose off fails keeps the icon" app_alive
expect "a Quit whose off fails keeps Scooch on" state_on
expect "a Quit whose off fails shows a sentence of the window's copy in the menu" said_first
press_item 'Quit Scooch'
wait_for 30 sh -c '! pgrep -x scooch-app >/dev/null' >/dev/null
wait_for 30 state_off >/dev/null
sleep 3
line "- after Quit, app processes $(app_count), wallpaper $(wallpaper), login item $(login), daemons $(daemons)"
expect "Quit Scooch turned Scooch off" state_off
expect "Quit Scooch put the photo back" [ "$(wallpaper)" = "$photo" ]
expect "Quit Scooch removed the login item" [ ! -f "$autostart" ]
expect "Quit Scooch ended the app process" gone_app

line
line "#### A background start with Scooch off"
started=$(date +%s%N)
clean /usr/bin/scooch-app --background >"$out/background-off.log" 2>&1
line "- the hidden start with Scooch off ended after $(( ($(date +%s%N) - started) / 1000000 )) ms, it says $(tr '\n' ' ' <"$out/background-off.log")"
expect "a background start with Scooch off ends at once" [ $(( ($(date +%s%N) - started) / 1000000 )) -lt 5000 ]
expect "a background start with Scooch off leaves no app process" gone_app

line
line "#### Login with Scooch on"
clean /usr/bin/scooch >"$out/tray-on.txt" 2>&1
wait_for 30 state_on >/dev/null
if took=$(wait_for 30 registered); then line "- scooch turned Scooch on and the daemon started the icon, registered after $took s"; else line "- scooch turned Scooch on, no icon registered within 30 s"; fi
expect "a daemon started for a turn on starts the icon" registered
pkill -x scooch-app >/dev/null 2>&1
pkill -x scooch >/dev/null 2>&1
wait_for 15 sh -c '! pgrep -x scooch-app >/dev/null && ! pgrep -x scooch >/dev/null' >/dev/null
line "- logged out in effect, app processes $(app_count), daemons $(daemons), the login item $(login)"
clean gio launch "$autostart" >"$out/login.txt" 2>&1
if took=$(wait_for 30 registered); then line "- gio launch of the login item, as a session starts it, brought the icon after $took s"; else line "- no icon within 30 s of the login item, it says $(tr '\n' ' ' <"$out/login.txt")"; fi
sleep 3
icon=$(app_pid)
watcher_pid=$(pgrep -x scooch | head -1)
line "- icon process $icon with parent $(ps -o ppid= -p "$icon" | tr -d ' '), daemon $watcher_pid, window $(has_window && echo open || echo closed)"
expect "after a login with Scooch on the icon is there" registered
expect "after a login with Scooch on no window opens" window_gone
expect "the icon process is no child of the daemon" not_child
sleep 60
line "- a minute later the daemon's children are $(ps -o pid=,stat=,comm= --ppid "$watcher_pid" | tr '\n' ';' | tr -s ' ')"
expect "a minute after the login the daemon runs with no defunct child" no_zombie
pkill -x scooch >/dev/null 2>&1
wait_for 15 sh -c '! pgrep -x scooch >/dev/null' >/dev/null
sleep 3
line "- stopped the daemon, the icon process $icon $(kill -0 "$icon" 2>/dev/null && echo still runs || echo ended)"
expect "the icon does not die with the daemon" kill -0 "$icon"
clean gio launch "$autostart" >>"$out/login.txt" 2>&1
wait_for 15 sh -c 'pgrep -x scooch >/dev/null' >/dev/null
sleep 8
line "- the login item started the daemon again, app processes $(app_count), the same icon $([ "$(app_pid)" = "$icon" ] && echo yes || echo no)"
expect "a daemon that starts again while the icon runs leaves one app process" [ "$(app_count)" = 1 ]
pkill -x scooch-app >/dev/null 2>&1
wait_for 10 gone_app >/dev/null
line "- ended the icon so the next parts start their own window process, Scooch stays on with daemons $(daemons)"
