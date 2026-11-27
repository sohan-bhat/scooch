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
now_ms() { date +%s%3N; }
private_mb() { awk '/^Private_(Clean|Dirty):/ { u += $2 } END { printf "%.1f", u / 1024 }' "/proc/$1/smaps_rollup" 2>/dev/null; }
below() { ps -o pid=,comm= --ppid "$1" 2>/dev/null | tr -s ' ' | tr '\n' ';'; }
live_apps() { ps -o pid=,stat= -C scooch-app 2>/dev/null | awk '$2 !~ /^Z/ { print $1 }'; }
single_app() { [ "$(live_apps | wc -l)" = 1 ]; }
lock_pids() {
  local inode
  inode=$(stat -c %i "$data/icon.lock" 2>/dev/null) || return 0
  awk -v inode="$inode" '$2 == "FLOCK" { n = split($6, at, ":"); if (at[n] == inode) print $5 }' /proc/locks 2>/dev/null
}
scooch_items() {
  local pid count=0
  for pid in $(tray items | tr ';' '\n' | sed -n 's/.* pid \([0-9][0-9]*\) *$/\1/p'); do
    [ "$(cat "/proc/$pid/comm" 2>/dev/null)" = scooch-app ] && count=$((count + 1))
  done
  echo "$count"
}
one_icon() {
  local held
  held=$(lock_pids)
  [ "$(echo $held | wc -w)" = 1 ] && [ "$(scooch_items)" = 1 ] && tray menu "$held" | grep -q '^Open Scooch'
}
shown_windows() { xdotool search --onlyvisible --name '^Scooch$' 2>/dev/null; }
has_shown() { [ -n "$(shown_windows)" ]; }
icon_only() { [ -z "$(shown_windows)" ] && one_icon; }
settle() {
  local limit="$1" from
  shift
  from=$(now_ms)
  while [ $(( $(now_ms) - from )) -lt "$limit" ]; do
    if "$@"; then
      echo $(( $(now_ms) - from ))
      return 0
    fi
    sleep 0.05
  done
  return 1
}
within_ten() { awk -v a="$1" -v b="$2" 'BEGIN { exit !(a != "" && b != "" && a + 0 <= b + 10) }'; }
fresh_icon() {
  local now
  now=$(live_apps)
  [ "$(echo $now | wc -w)" = 1 ] && [ "$now" != "$1" ] && [ -z "$(below "$now")" ]
}

handed=""
while [ $(( $(now_ms) - pressed_at )) -lt 3000 ]; do
  if fresh_icon "$before_close"; then
    handed=$(( $(now_ms) - pressed_at ))
    break
  fi
  sleep 0.05
done
line "- the session bus holds org.kde.StatusNotifierWatcher $(tray watcher), items $(tray items)"
wait_for 15 window_gone >/dev/null
sleep 3
line "- closed the window, window $(has_window && echo open || echo closed), app processes $(app_count), its menu $(tray menu "$(app_pid)")"
line "- the icon process was $before_close before the close and is $(live_apps | tr '\n' ' ')after, alone with no process below $([ -n "$handed" ] && echo "$handed ms after the close keys" || echo "not within 3 s"), processes below now $(below "$(app_pid)")"
expect "closing the window while Scooch is on leaves the app process on a desktop with a tray" app_alive
expect "closing the window removes the window" window_gone
expect "the app's icon is registered with the tray" registered
expect "within 3 s of the close one fresh process holds the icon, with another process id and no process below" [ -n "$handed" ]
snap() { for task in /proc/"$1"/task/*; do echo "$(basename "$task") $(cat "$task/comm" 2>/dev/null | tr ' ' '_') $(awk '/ctxt_switches/ { s += $2 } END { print s + 0 }' "$task/status" 2>/dev/null)"; done; }
quiet_minute() {
  local pid before after cpu
  xdotool mousemove 300 300 >/dev/null 2>&1
  sleep 3
  pid=$(app_pid)
  before=$(snap "$pid")
  cpu=$(cpu_ms "$pid")
  sleep 60
  after=$(snap "$pid")
  quiet=$(awk 'NR == FNR { was[$1] = $3; next } { total += $3 - ($1 in was ? was[$1] : 0) } END { print total + 0 }' <(echo "$before") <(echo "$after"))
  woken=$(awk 'NR == FNR { was[$1] = $3; next } { d = $3 - ($1 in was ? was[$1] : 0); if (d > 0) printf "%s:%d ", $2, d }' <(echo "$before") <(echo "$after"))
  quiet_threads=$(echo "$after" | wc -l)
  quiet_private=$(private_mb "$pid")
  line "- over a quiet minute, $1, the icon process $pid woke its $quiet_threads threads $quiet times, ${woken:-none of them}, used $(( $(cpu_ms "$pid") - cpu )) ms of CPU, and holds $(memory "$pid")"
}
quiet_minutes() {
  local pid before after counts="" least="" woke cpu
  xdotool mousemove 300 300 >/dev/null 2>&1
  sleep 3
  pid=$(app_pid)
  cpu=$(cpu_ms "$pid")
  before=$(snap "$pid")
  for minute in 1 2 3 4 5; do
    sleep 60
    after=$(snap "$pid")
    woke=$(awk 'NR == FNR { was[$1] = $3; next } { total += $3 - ($1 in was ? was[$1] : 0) } END { print total + 0 }' <(echo "$before") <(echo "$after"))
    counts="$counts $woke"
    if [ -z "$least" ] || [ "$woke" -lt "$least" ]; then least=$woke; fi
    before=$after
  done
  quiet_least=$least
  quiet_threads=$(echo "$after" | wc -l)
  quiet_private=$(private_mb "$pid")
  line "- over five quiet minutes, $1, the icon process $pid woke its $quiet_threads threads$counts times, one count a minute, used $(( $(cpu_ms "$pid") - cpu )) ms of CPU in all, and holds $(memory "$pid")"
}
quiet_minute "after the window closed"
closed_wakes=$quiet
closed_threads=$quiet_threads
closed_private=$quiet_private

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

line
line "#### A turn off while a background start waits for a tray"
pkill -x scooch-app >/dev/null 2>&1
wait_for 10 gone_app >/dev/null
xfce4-panel --quit >/dev/null 2>&1
sleep 3
clean /usr/bin/scooch-app --background >"$out/background-wait.log" 2>&1 &
sleep 3
line "- with no panel a background start waits for a tray, app processes $(app_count), the watcher $(tray watcher)"
clean /usr/bin/scooch off >"$out/off-while-waiting.txt" 2>&1
wait_for 30 state_off >/dev/null
env NO_AT_BRIDGE=0 xfce4-panel >>"$out/panel-a11y.log" 2>&1 &
wait_for 30 sh -c '[ "$(python3 "'"$PWD"'/.github/app/tray.py" watcher 2>/dev/null)" = yes ]' >/dev/null
if took=$(wait_for 20 gone_app); then line "- Scooch went off during the wait, the panel came back, and the app ended $took s later"; else line "- Scooch went off during the wait, the panel came back, and the app still runs"; fi
line "- the waiting start says $(tr '\n' ' ' <"$out/background-wait.log")"
expect "a background start that waited for a tray while Scooch went off ends once the tray comes" gone_app
clean /usr/bin/scooch >"$out/on-after-wait.txt" 2>&1
wait_for 30 state_on >/dev/null
wait_for 40 rendered >/dev/null
wait_for 30 registered >/dev/null
line "- Scooch on again, daemons $(daemons), the icon registered $(registered && echo yes || echo no)"
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

popups() { xwininfo -root -children 2>/dev/null | awk '/^ +0x/ { print $1 }' | while read -r window; do xwininfo -id "$window" 2>/dev/null | grep -q 'Map State: IsViewable' && echo "$window"; done | sort; }
panel_menu() {
  [ -n "$spot" ] || return 1
  local before after
  before=$(popups)
  xdotool mousemove $spot click 1
  sleep 1.5
  after=$(popups)
  shots=$((shots + 1))
  import -window root -crop 700x400+1220+0 "$out/panel-menu-$shots.png" 2>/dev/null
  [ -n "$(comm -13 <(echo "$before") <(echo "$after"))" ]
}
shots=0
fallbacks=0

press_item() {
  local steps
  steps=$(tray menu "$(app_pid)" | tr '|' '\n' | grep -v '^separator$' | grep -v ' dim$' | sed -E 's/ (un)?checked$//' | grep -nxF "$1" | cut -d: -f1)
  if [ -n "$steps" ] && panel_menu; then
    line "- a press on the icon opened the panel's menu, a new popup of the panel kept as panel-menu-$shots.png, whose items over DBusMenu read $(tray menu "$(app_pid)")"
    xdotool key --delay 200 $(for step in $(seq 1 "$steps"); do printf 'Down '; done) Return
    line "- pressed $1 in the panel's menu with the keyboard, $steps times Down and Enter"
    sleep 2
    return 0
  fi
  xdotool key Escape
  fallbacks=$((fallbacks + 1))
  line "- a press on the icon opened no popup of the panel, so the probe pressed $1 over DBusMenu, $(tray press "$(app_pid)" "$1")"
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
timeout 5 xdotool windowactivate --sync "$(window)" >/dev/null 2>&1
sleep 1
front_before=$(xdotool getactivewindow getwindowname 2>/dev/null)
clean /usr/bin/scooch-app --background >/dev/null 2>&1
sleep 2
front_after=$(xdotool getactivewindow getwindowname 2>/dev/null)
line "- a background start while the window is open, window $(has_window && echo open || echo closed), in front before $front_before and after $front_after"
expect "a background start while the window is open leaves the window open and in front" eval 'has_window && [ "$front_after" = Scooch ] && [ "$front_before" = Scooch ]'
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
line "#### Twenty openings and closes in a row"
opens=""
second=""
handovers=0
for round in $(seq 1 20); do
  settle 15000 icon_only >/dev/null
  from=$(now_ms)
  clean /usr/bin/scooch-app >>"$out/fresh-opens.log" 2>&1 &
  took=""
  while [ $(( $(now_ms) - from )) -lt 20000 ]; do
    if has_shown; then
      took=$(( $(now_ms) - from ))
      break
    fi
    sleep 0.02
  done
  opens="$opens ${took:-none}"
  [ "$round" = 2 ] && second=$took
  sleep 1
  holder=$(lock_pids)
  press_pill alt+F4
  moved() { icon_only && [ "$(lock_pids)" != "$holder" ]; }
  settle 8000 moved >/dev/null && handovers=$((handovers + 1))
done
settle 15000 single_app >/dev/null
sleep 2
twenty_icon=$(lock_pids)
twenty_private=$(private_mb "$twenty_icon")
line "- twenty openings, each as ms from the start of scooch-app until the window was mapped:$opens"
line "- $handovers of the twenty closes gave the icon to another process, then app processes $(live_apps | wc -l), Scooch items in the tray $(scooch_items), the icon process $twenty_icon with $(below "$twenty_icon" | grep -c .) processes below holds $(memory "$twenty_icon")"
expect "each of twenty closes gave the icon to a fresh process" [ "$handovers" = 20 ]
expect "after twenty openings and closes one app process runs" single_app
expect "after twenty openings and closes the tray lists one Scooch item" [ "$(scooch_items)" = 1 ]
expect "the second opening of the window is mapped within 1 s" eval '[ -n "$second" ] && [ "$second" -lt 1000 ]'

line
line "#### A start of Scooch during the hand over"
lost=0
strays=0
for delay in 0 30 60 120 250 500; do
  for trial in 1 2 3; do
    settle 15000 single_app >/dev/null || strays=$((strays + 1))
    settle 15000 icon_only >/dev/null
    clean /usr/bin/scooch-app >>"$out/fresh-race.log" 2>&1 &
    settle 20000 has_shown >/dev/null
    sleep 1.5
    old=$(shown_windows | head -1)
    holder=$(lock_pids)
    timeout 5 xdotool windowactivate --sync "$old" >/dev/null 2>&1
    timeout 5 xdotool windowfocus --sync "$old" >/dev/null 2>&1
    sleep 0.3
    xdotool key --delay 0 alt+F4
    asked=$(now_ms)
    [ "$delay" = 0 ] || sleep "0.$(printf '%03d' "$delay")"
    from=$(now_ms)
    clean /usr/bin/scooch-app >>"$out/fresh-race.log" 2>&1 &
    new=""
    took=""
    while [ $(( $(now_ms) - from )) -lt 8000 ]; do
      new=$(shown_windows | grep -vx "$old" | head -1)
      if [ -n "$new" ]; then
        took=$(( $(now_ms) - from ))
        break
      fi
      sleep 0.03
    done
    sleep 1.5
    held=$(lock_pids)
    one=$(one_icon && echo yes || echo no)
    left=no
    if [ -n "$new" ]; then
      timeout 5 xdotool windowactivate --sync "$new" >/dev/null 2>&1
      timeout 5 xdotool windowfocus --sync "$new" >/dev/null 2>&1
      sleep 0.3
      xdotool key --delay 0 alt+F4
      settle 8000 icon_only >/dev/null && left=yes
    fi
    line "- a start $(( from - asked )) ms after the close keys, the window was mapped after ${took:-more than 8000} ms, the icon went from process ${holder:-none} to ${held:-none}, one process holds the icon $one, the next close left the icon $left, app processes then $(live_apps | wc -l)"
    if [ -z "$took" ] || [ "$took" -gt 5000 ] || [ "$one" != yes ] || [ "$left" != yes ]; then lost=$((lost + 1)); fi
  done
done
settle 15000 single_app >/dev/null || strays=$((strays + 1))
line "- the starts said $(sort "$out/fresh-race.log" | uniq -c | sed 's/^ *//' | paste -sd '|' - | sed 's/|/ | /g')"
expect "each of eighteen starts during the hand over got a window within 5 s, left one process holding the icon and kept the icon at the next close" [ "$lost" = 0 ]
expect "after each start during the hand over one app process was left within 15 s" [ "$strays" = 0 ]

line
line "#### On in the menu after a hand over"
settle 15000 icon_only >/dev/null
fresh=$(lock_pids)
press_item On
wait_for 30 state_off >/dev/null
if took=$(wait_for 15 gone_app); then line "- On in the menu of the fresh icon process $fresh turned Scooch off and the process ended $took s later"; else line "- On in the menu of the fresh icon process $fresh left app processes $(app_count)"; fi
line "- after that, wallpaper $(wallpaper), login item $(login), daemons $(daemons)"
expect "On in the menu of a fresh icon process turned Scooch off" state_off
expect "off from the menu of a fresh icon process put the photo back" [ "$(wallpaper)" = "$photo" ]
expect "off from the menu of a fresh icon process removed the login item" [ ! -f "$autostart" ]
expect "off from the menu of a fresh icon process ended the daemon" no_daemon
expect "the fresh icon process ended when Scooch went off" gone_app
clean /usr/bin/scooch >"$out/on-after-menu-off.txt" 2>&1
wait_for 30 state_on >/dev/null
wait_for 40 rendered >/dev/null
wait_for 30 registered >/dev/null
clean /usr/bin/scooch-app >>"$out/fresh-opens.log" 2>&1 &
settle 20000 has_shown >/dev/null
sleep 2
holder=$(lock_pids)
press_pill alt+F4
settle 8000 moved >/dev/null
sleep 2
line "- Scooch on again, a window opened and closed, the icon went from process $holder to $(lock_pids), registered $(registered && echo yes || echo no), so Quit Scooch below is pressed on a fresh icon process"
expect "the icon is with a fresh process before Quit Scooch" eval 'single_app && registered && [ "$(lock_pids)" != "$holder" ]'

line
line "#### Quit Scooch"
sentences=$(python3 -c 'import json, sys; copy = json.load(open(sys.argv[1])); print("\n".join(list(copy["failure"].values()) + [copy["problem"]["stopped"]["line"]]))' crates/scooch-app/ui/src/copy/en.json)
stopped_line=$(python3 -c 'import json, sys; print(json.load(open(sys.argv[1]))["problem"]["stopped"]["line"])' crates/scooch-app/ui/src/copy/en.json)
first_item() { tray menu "$(app_pid)" | cut -d'|' -f1 | sed 's/ dim$//'; }
said_first() { first_item | grep -qxF -f <(printf '%s\n' "$sentences"); }
true_to_what_runs() { if no_daemon; then [ "$(first_item)" = "$stopped_line" ]; else [ "$(first_item)" != "$stopped_line" ]; fi; }
chmod a-w "$data"
press_item 'Quit Scooch'
sleep 4
line "- with the data folder read only, Quit left app processes $(app_count), saved on $(grep -c '"on": true' "$data/state.json"), the menu reads $(tray menu "$(app_pid)")"
chmod u+w "$data"
expect "a Quit whose off fails keeps the icon" app_alive
expect "a Quit whose off fails keeps Scooch on" state_on
expect "a Quit whose off fails shows a sentence of the window's copy in the menu" said_first
line "- after the failed off the daemons are $(daemons), and the menu's first line is $(first_item)"
expect "the sentence after a failed off is true about whether the daemon runs" true_to_what_runs
press_item 'Quit Scooch'
wait_for 30 sh -c '! pgrep -x scooch-app >/dev/null' >/dev/null
wait_for 30 state_off >/dev/null
sleep 3
line "- after Quit, app processes $(app_count), wallpaper $(wallpaper), login item $(login), daemons $(daemons)"
expect "Quit Scooch turned Scooch off" state_off
expect "Quit Scooch put the photo back" [ "$(wallpaper)" = "$photo" ]
expect "Quit Scooch removed the login item" [ ! -f "$autostart" ]
expect "Quit Scooch ended the app process" gone_app
expect "every press on the icon opened the panel's own menu" [ "$fallbacks" = 0 ]

line
line "#### A background start with Scooch off"
started=$(date +%s%N)
clean /usr/bin/scooch-app --background >"$out/background-off.log" 2>&1
line "- the hidden start with Scooch off ended after $(( ($(date +%s%N) - started) / 1000000 )) ms, it says $(tr '\n' ' ' <"$out/background-off.log")"
expect "a background start with Scooch off ends at once" [ $(( ($(date +%s%N) - started) / 1000000 )) -lt 5000 ]
expect "a background start with Scooch off leaves no app process" gone_app
clean /usr/bin/scooch-app >"$out/app-off.log" 2>&1 &
wait_for 40 has_window >/dev/null
sleep 3
press_pill alt+F4
wait_for 15 window_gone >/dev/null
if took=$(wait_for 15 gone_app); then line "- with Scooch off a plain start showed its window, and the process ended $took s after the window closed"; else line "- with Scooch off the process still runs after its window closed"; fi
expect "closing the window while Scooch is off ends the process" gone_app

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
logged=$(cat "$data/watch.log" 2>/dev/null)
line "- the daemon's watch.log says $(grep ' icon ' <<<"$logged" | tail -1)"
expect "the daemon's watch.log says when the icon started and how many ms that took" grep -q 'icon started .* in [0-9.]* ms' <<<"$logged"
quiet_minutes "started at the login with no window ever"
if [ "$quiet_least" != 0 ]; then
  line "- no minute of those five was free of wakes, so five more minutes are measured, and a timer of 60 s or less would wake every minute of both"
  quiet_minutes "started at the login, a second time"
fi
expect "the icon started at the login had a minute with no wake of its threads, so no timer of 60 s or less runs in it" [ "$quiet_least" = 0 ]
line "- the minute after the first close held $closed_private MB private in $closed_threads threads with $closed_wakes wakes, after twenty closes the icon held $twenty_private MB private, and this icon that never had a window holds $quiet_private MB private in $quiet_threads threads"
expect "the quiet minute after the close holds at most 10 MB more private memory than an icon that never had a window" within_ten "$closed_private" "$quiet_private"
expect "the quiet minute after the close had no wake" [ "$closed_wakes" = 0 ]
expect "the icon after the close runs as many threads as an icon that never had a window" [ "$closed_threads" = "$quiet_threads" ]
expect "after twenty closes the icon holds at most 10 MB more private memory than an icon that never had a window" within_ten "$twenty_private" "$quiet_private"
line "- a minute later the daemon's children are $(ps -o pid=,stat=,comm= --ppid "$watcher_pid" | tr '\n' ';' | tr -s ' ')"
expect "a minute after the login the daemon runs with no defunct child" no_zombie
pkill -x scooch >/dev/null 2>&1
wait_for 15 sh -c '! pgrep -x scooch >/dev/null' >/dev/null
sleep 3
line "- stopped the daemon, the icon process $icon $(kill -0 "$icon" 2>/dev/null && echo still runs || echo ended)"
expect "the icon does not die with the daemon" kill -0 "$icon"
from=$(wc -l <"$data/watch.log" 2>/dev/null || echo 0)
said_from=$(wc -l <"$out/login.txt" 2>/dev/null || echo 0)
clean gio launch "$autostart" >>"$out/login.txt" 2>&1
since_start() { if [ "$(wc -l <"$data/watch.log" 2>/dev/null || echo 0)" -lt "$from" ]; then cat "$data/watch.log"; else sed -n "$((from + 1)),\$p" "$data/watch.log"; fi 2>/dev/null; }
icon_said() { since_start | grep -E -m1 ' icon (not )?started '; }
one_app() { [ "$(app_count)" = 1 ]; }
heard=$(wait_for 60 icon_said >/dev/null && echo yes || echo no)
sleep 3
wait_for 10 one_app >/dev/null
line "- the login item started the daemon again, its line $([ "$heard" = yes ] && echo "'$(icon_said)'" || echo 'on the icon did not come within 60 s'), app processes $(app_count), the same icon $([ "$(app_pid)" = "$icon" ] && echo yes || echo no)"
line "- the daemon's lines since that start $(since_start | sed '/^$/d' | paste -sd '|' - | sed 's/|/ | /g'), its error output $(sed -n "$((said_from + 1)),\$p" "$out/login.txt" 2>/dev/null | sed '/^$/d' | paste -sd '|' - | sed 's/|/ | /g')"
expect "the restarted daemon wrote its icon line within 60 s, which the restart check stands on" [ "$heard" = yes ]
expect "a daemon that starts again while the icon runs leaves one app process" [ "$(app_count)" = 1 ]

line
line "#### A replaced app file hands the icon to the new one"
old_icon=$(app_pid)
sudo cp -p /usr/bin/scooch-app /usr/bin/scooch-app.new && sudo mv -f /usr/bin/scooch-app.new /usr/bin/scooch-app
started=$(date +%s%3N)
touch "$data/probe-nudge" && rm -f "$data/probe-nudge"
gone_at=""
back_at=""
for step in $(seq 1 150); do
  if [ -z "$gone_at" ] && ! kill -0 "$old_icon" 2>/dev/null; then gone_at=$(date +%s%3N); fi
  if [ -n "$gone_at" ] && [ -n "$(app_pid)" ] && [ "$(app_pid)" != "$old_icon" ] && registered; then back_at=$(date +%s%3N); break; fi
  sleep 0.1
done
line "- after the program file was replaced and the data folder changed, the old process $old_icon ended after $([ -n "$gone_at" ] && echo "$((gone_at - started)) ms" || echo never), the new one $(app_pid) registered its icon $([ -n "$back_at" ] && echo "$((back_at - gone_at)) ms after the old one ended" || echo never), Scooch on $(state_on && echo yes || echo no)"
expect "a replaced program file with Scooch on ends the old icon process" [ -n "$gone_at" ]
expect "the new program takes over the icon" [ -n "$back_at" ]

line
line "#### The icon lock"
pkill -x scooch-app >/dev/null 2>&1
wait_for 10 gone_app >/dev/null
python3 -c 'import fcntl, sys, time; f = open(sys.argv[1], "a"); fcntl.flock(f, fcntl.LOCK_EX); print("held", flush=True); time.sleep(90)' "$data/icon.lock" >"$out/lock-holder.txt" 2>&1 &
holder=$!
wait_for 10 grep -q held "$out/lock-holder.txt" >/dev/null
started=$(date +%s%N)
clean /usr/bin/scooch-app --background >"$out/background-locked.log" 2>&1
ended=$(( ($(date +%s%N) - started) / 1000000 ))
line "- with another process holding the lock, a background start ended after $ended ms and says $(tr '\n' ' ' <"$out/background-locked.log")"
expect "a background start without the lock ends at once" [ "$ended" -lt 5000 ]
expect "a background start without the lock leaves no app process" gone_app
clean /usr/bin/scooch-app >"$out/window-locked.log" 2>&1 &
wait_for 40 has_window >/dev/null
sleep 3
line "- a plain start without the lock, window $(has_window && echo open || echo closed), it says $(tr '\n' ' ' <"$out/window-locked.log" | head -c 300)"
expect "a plain start without the lock shows its window" has_window
expect "a plain start without the lock makes no icon" eval '! grep -q "tray made" "$out/window-locked.log"'
press_pill alt+F4
wait_for 15 gone_app >/dev/null
expect "the window without the lock ends its process when it closes" gone_app
kill "$holder" >/dev/null 2>&1
wait "$holder" 2>/dev/null

pkill -x scooch-app >/dev/null 2>&1
wait_for 10 gone_app >/dev/null
line "- ended the icon so the next parts start their own window process, Scooch stays on with daemons $(daemons)"
cp "$data/watch.log" "$out/daemon-watch.log" 2>/dev/null
