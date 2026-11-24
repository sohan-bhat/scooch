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
"$app/Contents/MacOS/scooch-app" --background >/dev/null 2>&1 &
sleep 6
line "- a second start with the hidden argument while the app runs left $(icon_count) app process, window $(has_window && echo open || echo closed)"
expect "a background start while the app runs leaves one app process" [ "$(icon_count)" = 1 ]
close_window
sleep 2
expect "the window closed again and the icon stays" app_alive

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
quiet_minute "started at the login with no window ever"
expect "the icon started at the login woke at most 6 times from idle in a quiet minute" quiet_enough
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
