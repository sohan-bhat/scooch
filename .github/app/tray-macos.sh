line
line "### The menu bar icon"
icon_pid() { pgrep -f "/Contents/MacOS/scooch-app" | head -1; }
icon_count() { pgrep -f "/Contents/MacOS/scooch-app" | wc -l | tr -d ' '; }
app_alive() { [ -n "$(icon_pid)" ]; }
app_gone() { [ -z "$(icon_pid)" ]; }
window_gone() { [ -z "$(window_id)" ]; }
dock_item() { limited 15 osascript -e 'tell application "System Events" to tell process "Dock"' -e 'set told to "items"' -e 'repeat with item_ in (every UI element of list 1)' -e 'if name of item_ is "Scooch" then set told to told & " running " & (value of attribute "AXIsApplicationRunning" of item_ as text)' -e 'end repeat' -e 'return told' -e 'end tell' 2>/dev/null; }
in_dock() { grep -q 'running true' <<<"$(dock_item)"; }
not_in_dock() { ! in_dock; }
kind() { lsappinfo info -only ApplicationType -app me.sohanbhat.scooch.window 2>/dev/null | sed 's/.*=//' | tr -d '"'; }

status() {
  limited 60 osascript - "$@" <<'APPLESCRIPT' 2>&1
on run argv
  set what to item 1 of argv
  tell application "System Events"
    set scoochApp to first process whose bundle identifier is "me.sohanbhat.scooch.window"
    set bars to menu bars of scoochApp
    if (count of bars) is 0 then return "no menu bar"
    set bar to item -1 of bars
    if (count of menu bar items of bar) is 0 then return "no status item in " & (count of bars) & " menu bars"
    set icon to menu bar item 1 of bar
    if what is "where" then
      set {x, y} to position of icon
      set {w, h} to size of icon
      return "at " & x & " " & y & " size " & w & " " & h & " of " & (count of bars) & " menu bars"
    end if
    if what is "ax" then
      click icon
      delay 1
      return "pressed the icon through accessibility"
    end if
    if what is "open" then
      return (exists menu 1 of icon) as text
    end if
    if what is "items" then
      set told to {}
      repeat with entry in (menu items of menu 1 of icon)
        set label to "separator"
        try
          set label to (name of entry) as text
        end try
        if label is "missing value" then set label to "separator"
        set mark to ""
        try
          set mark to (value of attribute "AXMenuItemMarkChar" of entry) as text
        end try
        if mark is "missing value" then set mark to ""
        set shown to "on"
        try
          if not (enabled of entry) then set shown to "dim"
        end try
        set end of told to label & " " & shown & " " & mark
      end repeat
      set AppleScript's text item delimiters to "|"
      return told as text
    end if
    if what is "press" then
      click menu item (item 2 of argv) of menu 1 of icon
      return "pressed " & (item 2 of argv)
    end if
    if what is "close" then
      key code 53
      return "closed the menu"
    end if
  end tell
end run
APPLESCRIPT
}

mouse_at() {
  limited 15 osascript -l JavaScript -e "
    ObjC.import('CoreGraphics');
    const spot = \$.CGPointMake($1, $2);
    for (const kind of [\$.kCGEventLeftMouseDown, \$.kCGEventLeftMouseUp]) {
      \$.CGEventPost(\$.kCGHIDEventTap, \$.CGEventCreateMouseEvent(null, kind, spot, \$.kCGMouseButtonLeft));
      delay(0.08);
    }
    'clicked';" 2>&1
}

menu_open() { [ "$(status open)" = true ]; }
icon_shown() { grep -q '^at .* of 2 menu bars' <<<"$(status where)"; }
front() { limited 15 osascript -e 'tell application "System Events" to get bundle identifier of first process whose frontmost is true' 2>/dev/null; }
to_front() { limited 15 osascript -e 'tell application "System Events" to set frontmost of (first process whose bundle identifier is "me.sohanbhat.scooch.window") to true' >/dev/null 2>&1; sleep 1; }
ms() { perl -MTime::HiRes=time -e 'printf "%d\n", time * 1000'; }

mouse_move() {
  limited 15 osascript -l JavaScript -e "ObjC.import('CoreGraphics'); \$.CGEventPost(\$.kCGHIDEventTap, \$.CGEventCreateMouseEvent(null, \$.kCGEventMouseMoved, \$.CGPointMake($1, $2), \$.kCGMouseButtonLeft)); 'moved';" >/dev/null 2>&1
}

open_menu() {
  local where x y w h
  status ax >/dev/null
  if wait_for 3 menu_open >/dev/null; then echo "the menu opened through accessibility"; return 0; fi
  where=$(status where)
  read -r x y w h <<<"$(sed -n 's/^at \([0-9-]*\) \([0-9-]*\) size \([0-9]*\) \([0-9]*\).*/\1 \2 \3 \4/p' <<<"$where")"
  if [ -z "${x:-}" ]; then echo "no icon to press, $where"; return 1; fi
  mouse_at "$((x + w / 2))" "$((y + h / 2))" >/dev/null
  if wait_for 5 menu_open >/dev/null; then echo "a press of the icon opened the menu, accessibility alone did not"; return 0; fi
  echo "the menu did not open, $where"
  return 1
}

press_item() {
  local how
  how=$(open_menu)
  line "- $how"
  line "- the menu reads $(status items)"
  line "- $(status press "$1")"
  sleep 2
}

checked_on() { status items | tr '|' '\n' | grep -q '^On on ✓'; }
reads_menu() {
  local told
  told=$(status items)
  [ "$(cut -d'|' -f1 <<<"$told")" = "Open Scooch on " ] && grep -q '^On on' <<<"$(cut -d'|' -f2 <<<"$told")" && [ "$(cut -d'|' -f4 <<<"$told")" = "Quit Scooch on " ]
}

rusage() {
  python3 - "$1" <<'PY'
import ctypes, sys
class Usage(ctypes.Structure):
    _fields_ = [("uuid", ctypes.c_uint8 * 16)] + [(name, ctypes.c_uint64) for name in ("user", "system", "idle", "interrupt", "pageins", "wired", "resident", "footprint", "start", "exit", "cu", "cs", "ci", "cint", "cp", "ce", "read", "written")]
class Base(ctypes.Structure):
    _fields_ = [("numer", ctypes.c_uint32), ("denom", ctypes.c_uint32)]
system = ctypes.CDLL("/usr/lib/libSystem.B.dylib")
usage, base = Usage(), Base()
system.mach_timebase_info(ctypes.byref(base))
if system.proc_pid_rusage(int(sys.argv[1]), 2, ctypes.byref(usage)) != 0:
    sys.exit(1)
cpu = (usage.user + usage.system) * base.numer / base.denom / 1e6
print(usage.idle, usage.interrupt, usage.footprint, usage.resident, round(cpu, 1))
PY
}

quiet_minutes() {
  local pid before after first counts="" least="" woke idle0 idle1 cpu0 cpu1 foot1 res1
  mouse_move 300 300
  sleep 3
  pid=$(icon_pid)
  first=$(rusage "$pid")
  before=$first
  for minute in 1 2 3 4 5; do
    sleep 60
    after=$(rusage "$pid")
    read -r idle0 _ _ _ _ <<<"${before:-0 0 0 0 0}"
    read -r idle1 _ _ _ _ <<<"${after:-0 0 0 0 0}"
    woke=$((idle1 - idle0))
    counts="$counts $woke"
    if [ -z "$least" ] || [ "$woke" -lt "$least" ]; then least=$woke; fi
    before=$after
  done
  read -r _ _ _ _ cpu0 <<<"${first:-0 0 0 0 0}"
  read -r _ _ foot1 res1 cpu1 <<<"${after:-0 0 0 0 0}"
  quiet_least=$least
  if [ -z "$first" ] || [ -z "$after" ]; then quiet_least=unread; line "- the usage of the icon process $pid could not be read"; fi
  line "- over five quiet minutes, $1, the icon process $pid woke from idle$counts times, one count a minute, used $(awk -v a="$cpu0" -v b="$cpu1" 'BEGIN { printf "%.1f", b - a }') ms of CPU in all, and holds a footprint of $(awk -v f="$foot1" 'BEGIN { printf "%.1f", f / 1048576 }') MB with $(awk -v r="$res1" 'BEGIN { printf "%.1f", r / 1048576 }') MB resident"
}

quiet_minute() {
  local pid before after
  mouse_move 300 300
  sleep 3
  pid=$(icon_pid)
  before=$(rusage "$pid")
  sleep 60
  after=$(rusage "$pid")
  read -r idle0 int0 _ _ cpu0 <<<"${before:-0 0 0 0 0}"
  read -r idle1 int1 foot1 res1 cpu1 <<<"${after:-0 0 0 0 0}"
  quiet_idle=$((idle1 - idle0))
  quiet_int=$((int1 - int0))
  quiet_read=no
  if [ -n "$before" ] && [ -n "$after" ]; then quiet_read=yes; else line "- the usage of the icon process $pid could not be read"; fi
  line "- over a quiet minute, $1, the icon process $pid woke $quiet_idle times from idle and $quiet_int times by interrupt, used $(awk -v a="$cpu0" -v b="$cpu1" 'BEGIN { printf "%.1f", b - a }') ms of CPU, and holds a footprint of $(awk -v f="$foot1" 'BEGIN { printf "%.1f", f / 1048576 }') MB with $(awk -v r="$res1" 'BEGIN { printf "%.1f", r / 1048576 }') MB resident"
}

line "- before closing, window $(has_window && echo open || echo closed), app processes $(icon_count), the app is $(kind), the Dock reads $(dock_item)"
expect "while the window is open Scooch shows in the Dock" in_dock
close_window
sleep 3
line "- closed the window, window $(has_window && echo open || echo closed), app processes $(icon_count), the app is $(kind), the Dock item $(dock_item), the icon $(status where)"
expect "closing the window while Scooch is on leaves the app process" app_alive
expect "closing the window removes the window" window_gone
expect "the icon sits in the menu bar after the window closed" grep -q '^at ' <<<"$(status where)"
expect "with only the menu bar icon Scooch is not in the Dock" not_in_dock
expect "with only the menu bar icon the app runs as a UI element" [ "$(kind)" = UIElement ]
limited 20 screencapture -x -R 0,0,3000,40 "$out/menu-bar.png" && line "- the menu bar kept as menu-bar.png"
quiet_minute "after the window closed"
quiet_enough() { [ "$quiet_read" = yes ] && [ "$quiet_idle" -le 6 ]; }

line
line "#### Open Scooch from the menu"
line "- $(open_menu)"
line "- the menu reads $(status items)"
expect "the menu reads Open Scooch, On checked and Quit Scooch" reads_menu
expect "On is checked while Scooch is on" checked_on
line "- $(status press 'Open Scooch')"
if took=$(wait_for 20 has_window); then line "- the window came back after $took s"; else line "- no window within 20 s"; fi
sleep 2
expect "Open Scooch in the menu opens the window" has_window
expect "with the window open Scooch is in the Dock again" in_dock
expect "with the window open the app runs in the foreground" [ "$(kind)" = Foreground ]
shot from-icon

line
line "#### On in the menu turns Scooch off and on with the window open"
press_item On
wait_for 30 state_off >/dev/null
sleep 3
line "- after On, saved on $(grep -c '"on": true' "$data/state.json"), wallpaper $(wallpaper), the icon $(status where)"
expect "On in the menu turned Scooch off" state_off
expect "off from the menu put the photo back" wallpaper_is_photo
expect "with the window open the icon stays while Scooch is off" grep -q '^at ' <<<"$(status where)"
line "- $(open_menu)"
line "- the menu reads $(status items)"
expect "On is not checked while Scooch is off" eval '! checked_on'
line "- $(status press On)"
wait_for 30 state_on >/dev/null
sleep 2
stand_in "On in the menu, through turn_on"
wait_for 40 rendered >/dev/null
sleep 3
line "- after On again, saved on $(grep -c '"on": true' "$data/state.json"), wallpaper $(wallpaper)"
expect "On in the menu turned Scooch on again" state_on
expect "on from the menu drew the desktop" rendered

line
line "#### Opening Scooch while only the icon runs"
close_window
sleep 2
before=$(icon_pid)
limited 20 open "$app" >/dev/null 2>&1
if took=$(wait_for 20 has_window); then line "- open Scooch.app showed the window after $took s"; else line "- open Scooch.app showed no window within 20 s"; fi
sleep 2
line "- app processes $(icon_count), the same one $([ "$(icon_pid)" = "$before" ] && echo yes || echo no)"
expect "opening Scooch while the icon runs shows the window" has_window
expect "opening Scooch while the icon runs keeps one app process" [ "$(icon_count)" = 1 ]
to_front
before_front=$(front)
"$app/Contents/MacOS/scooch-app" --background >/dev/null 2>&1 &
sleep 6
after_front=$(front)
line "- a second start with the hidden argument while the app runs left $(icon_count) app process, window $(has_window && echo open || echo closed), in front before $before_front and after $after_front"
expect "a background start while the app runs leaves one app process" [ "$(icon_count)" = 1 ]
expect "a background start while the window is open leaves the window open and in front" eval 'has_window && [ "$after_front" = me.sohanbhat.scooch.window ] && [ "$before_front" = "$after_front" ]'
close_window
sleep 2
expect "the window closed again and the icon stays" app_alive

on_again() {
  "$daemon" use "$photo" >>"$out/tray-on.txt" 2>&1
  wait_for 30 state_on >/dev/null
  stand_in "$1"
  wait_for 40 rendered >/dev/null
  wait_for 30 app_alive >/dev/null
  sleep 3
  line "- Scooch on again $1, saved on $(grep -c '"on": true' "$data/state.json"), app processes $(icon_count), the icon $(status where)"
}

line
line "#### Cmd+Q turns Scooch off"
limited 20 open "$app" >/dev/null 2>&1
wait_for 20 has_window >/dev/null
to_front
line "- the window $(has_window && echo open || echo closed), in front $(front)"
limited 15 osascript -e 'tell application "System Events" to keystroke "q" using command down' >/dev/null 2>&1
wait_for 30 app_gone >/dev/null
wait_for 30 state_off >/dev/null
sleep 3
line "- after Cmd+Q, app processes $(icon_count), saved on $(grep -c '"on": true' "$data/state.json"), wallpaper $(wallpaper), login item $([ -f "$plist" ] && echo present || echo gone)"
expect "Cmd+Q turned Scooch off" state_off
expect "Cmd+Q put the photo back" wallpaper_is_photo
expect "Cmd+Q ended the app process" app_gone
on_again "after Cmd+Q"

line
line "#### The app menu's Quit Scooch turns Scooch off"
limited 20 open "$app" >/dev/null 2>&1
wait_for 20 has_window >/dev/null
to_front
line "- the app menu $(limited 30 osascript -e 'tell application "System Events" to tell (first process whose bundle identifier is "me.sohanbhat.scooch.window") to click menu item "Quit Scooch" of menu 1 of menu bar item 2 of menu bar 1' 2>&1 | head -c 200)"
wait_for 30 app_gone >/dev/null
wait_for 30 state_off >/dev/null
sleep 3
line "- after the app menu's Quit Scooch, app processes $(icon_count), saved on $(grep -c '"on": true' "$data/state.json"), wallpaper $(wallpaper)"
expect "the app menu's Quit Scooch turned Scooch off" state_off
expect "the app menu's Quit Scooch put the photo back" wallpaper_is_photo
expect "the app menu's Quit Scooch ended the app process" app_gone
on_again "after the app menu's Quit Scooch"

line
line "#### A quit the system sends with Scooch on brings the icon back"
old_icon=$(icon_pid)
line "- the system's quit $(limited 20 osascript -e 'tell application id "me.sohanbhat.scooch.window" to quit' 2>&1 | head -c 200)"
back_from_quit() { [ -n "$(icon_pid)" ] && [ "$(icon_pid)" != "$old_icon" ] && icon_shown; }
if took=$(wait_for 20 back_from_quit); then line "- the quit ended $old_icon and the new icon process $(icon_pid) was in the menu bar after $took s"; else line "- after the quit, app processes $(pgrep -fl 'MacOS/scooch-app' | tr '\n' ';'), the icon $(status where)"; fi
expect "a quit the system sends ends the process and the icon comes back" back_from_quit
expect "a quit the system sends leaves Scooch on" state_on

line
line "#### Quit Scooch"
press_item 'Quit Scooch'
wait_for 30 app_gone >/dev/null
wait_for 30 state_off >/dev/null
sleep 3
line "- after Quit, app processes $(icon_count), saved on $(grep -c '"on": true' "$data/state.json"), wallpaper $(wallpaper), login item $([ -f "$plist" ] && echo present || echo gone), daemons $(pgrep -f 'MacOS/scooch watch' | wc -l | tr -d ' ')"
expect "Quit Scooch turned Scooch off" state_off
expect "Quit Scooch put the photo back" wallpaper_is_photo
expect "Quit Scooch removed the login item" no_plist
expect "Quit Scooch ended the app process" app_gone

line
line "#### A background start with Scooch off"
started=$(date +%s)
"$app/Contents/MacOS/scooch-app" --background >"$out/background-off.log" 2>&1
ended=$(( $(date +%s) - started ))
line "- the hidden start with Scooch off ended after $ended s, it says $(tr '\n' ' ' <"$out/background-off.log")"
expect "a background start with Scooch off ends at once" [ "$ended" -le 5 ]
expect "a background start with Scooch off leaves no app process" app_gone

line
line "#### Login with Scooch on"
"$daemon" use "$photo" >"$out/tray-on.txt" 2>&1
wait_for 30 state_on >/dev/null
wait_for 30 app_alive >/dev/null
sleep 3
line "- scooch use turned Scooch on, the daemon started the icon $(app_alive && echo yes || echo no)"
expect "a daemon started from the login item starts the icon" app_alive
pkill -f "/Contents/MacOS/scooch-app" >/dev/null 2>&1
launchctl bootout "gui/$(id -u)/$label" >/dev/null 2>&1
wait_for 15 sh -c '! pgrep -f "MacOS/scooch watch" >/dev/null' >/dev/null
wait_for 15 app_gone >/dev/null
line "- logged out in effect, no daemon $(pgrep -f 'MacOS/scooch watch' >/dev/null && echo no || echo yes), no app $(app_gone && echo yes || echo no)"
from=$(wc -l <"$data/watch.log" 2>/dev/null || echo 0)
launchctl bootstrap "gui/$(id -u)" "$plist" >"$out/login.txt" 2>&1
line "- launchd loaded the login item as at login, exit $?"
if took=$(wait_for 30 app_alive); then line "- the icon process came up $took s after the login item"; else line "- no icon process within 30 s of the login item"; fi
close_prompts
sleep 4
icon=$(icon_pid)
watcher=$(pgrep -f 'MacOS/scooch watch' | head -1)
line "- icon process $icon with parent $(ps -o ppid= -p "$icon" | tr -d ' '), daemon $watcher, the daemon says $(sed -n "$((from + 1)),\$p" "$data/watch.log" 2>/dev/null | grep -E ' icon | helper parked ' | head -3 | tr '\n' ' ')"
line "- after the login, window $(has_window && echo open || echo closed), the icon $(status where), the app is $(kind)"
expect "after a login with Scooch on the icon is there" grep -q '^at ' <<<"$(status where)"
expect "after a login with Scooch on no window opens" window_gone
expect "after a login the app is not in the Dock" not_in_dock
logged=$(sed -n "$((from + 1)),\$p" "$data/watch.log" 2>/dev/null)
expect "the daemon's watch.log says when the icon started and how many ms that took" grep -q 'icon started .* in [0-9.]* ms' <<<"$logged"
after_tick() { awk '/ tick / { if (!t) t = NR } / icon started / { if (!i) i = NR } END { exit !(t && i && t < i) }' "$out/watch.log"; }
line "- in the traced daemon's log the first tick is line $(grep -n -m1 ' tick ' "$out/watch.log" | cut -d: -f1) and the first icon start is line $(grep -n -m1 ' icon started ' "$out/watch.log" | cut -d: -f1)"
expect "the traced daemon started the icon after its first tick" after_tick
quiet_minutes "started at the login with no window ever"
if [ "$quiet_least" != 0 ]; then
  line "- no minute of those five was free of wakes, so five more minutes are measured, and a timer of 60 s or less would wake every minute of both"
  quiet_minutes "started at the login, a second time"
fi
expect "the icon started at the login had a minute with no wake from idle, so no timer of 60 s or less runs in it" [ "$quiet_least" = 0 ]
not_child() { [ -n "$icon" ] && [ -n "$watcher" ] && [ "$(ps -o ppid= -p "$icon" | tr -d ' ')" != "$watcher" ]; }
no_zombie() { [ -n "$watcher" ] && kill -0 "$watcher" 2>/dev/null && ! ps -axo ppid=,stat= | awk -v d="$watcher" '$1 == d && $2 ~ /Z/' | grep -q .; }
expect "the icon process is no child of the daemon" not_child
sleep 60
line "- a minute later the daemon's children are $(ps -axo pid=,ppid=,stat=,comm= | awk -v d="$watcher" '$2 == d' | tr '\n' ';' | tr -s ' ')"
expect "a minute after the login the daemon runs with no defunct child" no_zombie
launchctl bootout "gui/$(id -u)/$label" >/dev/null 2>&1
wait_for 15 sh -c '! pgrep -f "MacOS/scooch watch" >/dev/null' >/dev/null
sleep 3
line "- booted the daemon out, the icon process $icon $(kill -0 "$icon" 2>/dev/null && echo still runs || echo ended)"
expect "the icon does not die with the daemon" kill -0 "$icon"
launchctl bootstrap "gui/$(id -u)" "$plist" >/dev/null 2>&1
wait_for 15 sh -c 'pgrep -f "MacOS/scooch watch" >/dev/null' >/dev/null
sleep 8
line "- the login item started the daemon again, app processes $(icon_count), the same icon $([ "$(icon_pid)" = "$icon" ] && echo yes || echo no)"
expect "a daemon that starts again while the icon runs leaves one app process" [ "$(icon_count)" = 1 ]
stand_in "the login, through the login item"
wait_for 40 rendered >/dev/null
sleep 3
line "- Scooch is on with only the icon for the rest of the probe, app processes $(icon_count), window $(has_window && echo open || echo closed)"

line
line "#### A replaced app file hands the icon to the new one"
old_icon=$(icon_pid)
program="$app/Contents/MacOS/scooch-app"
cp -p "$program" "$program.new" && mv -f "$program.new" "$program"
started=$(ms)
touch "$data/probe-nudge" && rm -f "$data/probe-nudge"
gone_at=""
back_at=""
for step in $(seq 1 150); do
  if [ -z "$gone_at" ] && ! kill -0 "$old_icon" 2>/dev/null; then gone_at=$(ms); fi
  if [ -n "$gone_at" ] && [ -n "$(icon_pid)" ] && [ "$(icon_pid)" != "$old_icon" ] && icon_shown; then back_at=$(ms); break; fi
  sleep 0.1
done
line "- after the program file was replaced and the data folder changed, the old process $old_icon ended after $([ -n "$gone_at" ] && echo "$((gone_at - started)) ms" || echo never), the new one $(icon_pid) showed its icon $([ -n "$back_at" ] && echo "$((back_at - gone_at)) ms after the old one ended" || echo never), Scooch on $(state_on && echo yes || echo no)"
expect "a replaced program file with Scooch on ends the old icon process" [ -n "$gone_at" ]
expect "the new program takes over the icon" [ -n "$back_at" ]

line
line "#### The icon lock"
pkill -f "/Contents/MacOS/scooch-app" >/dev/null 2>&1
wait_for 15 app_gone >/dev/null
python3 -c 'import fcntl, sys, time; f = open(sys.argv[1], "a"); fcntl.flock(f, fcntl.LOCK_EX); print("held", flush=True); time.sleep(90)' "$data/icon.lock" >"$out/lock-holder.txt" 2>&1 &
holder=$!
wait_for 10 grep -q held "$out/lock-holder.txt" >/dev/null
started=$(date +%s)
"$app/Contents/MacOS/scooch-app" --background >"$out/background-locked.log" 2>&1
ended=$(( $(date +%s) - started ))
line "- with another process holding the lock, a background start ended after $ended s and says $(tr '\n' ' ' <"$out/background-locked.log")"
expect "a background start without the lock ends at once" [ "$ended" -le 5 ]
expect "a background start without the lock leaves no app process" app_gone
"$app/Contents/MacOS/scooch-app" >"$out/window-locked.log" 2>&1 &
wait_for 40 has_window >/dev/null
sleep 3
line "- a plain start without the lock, window $(has_window && echo open || echo closed), menu bars $(status where), it says $(tr '\n' ' ' <"$out/window-locked.log" | head -c 300)"
expect "a plain start without the lock shows its window" has_window
expect "a plain start without the lock makes no icon" eval '! icon_shown'
close_window
wait_for 15 app_gone >/dev/null
expect "the window without the lock ends its process when it closes" app_gone
kill "$holder" >/dev/null 2>&1
wait "$holder" 2>/dev/null
"$app/Contents/MacOS/scooch-app" --background >"$out/background-free.log" 2>&1 &
wait_for 20 icon_shown >/dev/null
line "- with the lock free again a background start made the icon $(status where)"
expect "with the lock free a background start makes the icon" icon_shown
