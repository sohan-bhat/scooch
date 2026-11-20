#!/usr/bin/env bash
set -u

built="$1"
out="$2"
mkdir -p "$out"
report="$out/report.md"
app=/Applications/Scooch.app
daemon="$app/Contents/MacOS/scooch"
data="$HOME/Library/Application Support/Scooch"
plist="$HOME/Library/LaunchAgents/me.sohanbhat.scooch.plist"
photo="$PWD/assets/photos/seagull-sky.jpg"
label="me.sohanbhat.scooch"

line() { echo "$*" | tee -a "$report"; }
limited() { perl -e 'alarm shift; exec @ARGV' "$@"; }
limit() { line "- limit: $*"; }

wallpaper() {
  limited 15 osascript -e 'with timeout of 10 seconds' -e 'tell application "Finder" to get POSIX path of (desktop picture as alias)' -e 'end timeout' 2>/dev/null
}

window_id() {
  limited 15 osascript -l JavaScript -e '
    ObjC.import("CoreGraphics");
    const list = ObjC.deepUnwrap(ObjC.castRefToObject($.CGWindowListCopyWindowInfo($.kCGWindowListOptionOnScreenOnly, 0))) || [];
    const mine = list.filter((w) => (w.kCGWindowOwnerName === "Scooch" || w.kCGWindowOwnerName === "scooch-app") && w.kCGWindowLayer === 0);
    mine.length ? String(mine[0].kCGWindowNumber) : "";
  ' 2>/dev/null
}

window_size() {
  limited 15 osascript -l JavaScript -e '
    ObjC.import("CoreGraphics");
    const list = ObjC.deepUnwrap(ObjC.castRefToObject($.CGWindowListCopyWindowInfo($.kCGWindowListOptionOnScreenOnly, 0))) || [];
    const mine = list.filter((w) => (w.kCGWindowOwnerName === "Scooch" || w.kCGWindowOwnerName === "scooch-app") && w.kCGWindowLayer === 0);
    mine.length ? `${mine[0].kCGWindowBounds.Width}x${mine[0].kCGWindowBounds.Height}` : "";
  ' 2>/dev/null
}

shot() {
  local id
  id=$(window_id)
  if [ -n "$id" ]; then
    limited 20 screencapture -x -o -l "$id" "$out/$1.png" && line "- window shot $1.png at $(window_size)"
  else
    limited 20 screencapture -x -D 1 "$out/$1-screen.png"
    line "- no window found for $1, took the whole screen instead"
  fi
}

press() {
  limited 90 osascript - "$1" <<'APPLESCRIPT' 2>&1
on run argv
  set wanted to item 1 of argv
  tell application "System Events"
    set scoochApp to first process whose bundle identifier is "me.sohanbhat.scooch.window"
    set frontmost of scoochApp to true
    set everything to entire contents of window 1 of scoochApp
    repeat with element in everything
      try
        if role of element is "AXButton" then
          set seen to ""
          try
            set seen to seen & (name of element as text)
          end try
          try
            set seen to seen & (description of element as text)
          end try
          try
            set seen to seen & (title of element as text)
          end try
          if seen contains wanted then
            click element
            return "pressed " & wanted
          end if
        end if
      end try
    end repeat
  end tell
  return "no button " & wanted
end run
APPLESCRIPT
}

wait_for() {
  local limit="$1"
  shift
  local waited=0
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

state_on() { grep -q '"on": true' "$data/state.json" 2>/dev/null; }
state_off() { ! grep -q '"on": true' "$data/state.json" 2>/dev/null; }
rendered() { grep -q '"fingerprint": "' "$data/state.json" 2>/dev/null; }
has_window() { [ -n "$(window_id)" ]; }
no_plist() { [ ! -f "$plist" ]; }
wallpaper_is_photo() { [ "$(wallpaper)" = "$photo" ]; }

close_prompts() {
  for owner in UserNotificationCenter universalAccessAuthWarn CoreServicesUIAgent; do
    killall "$owner" >/dev/null 2>&1 && line "- closed a consent window by quitting $owner, which answers nothing"
  done
}

stand_in() {
  launchctl bootout "gui/$(id -u)/$label" >/dev/null 2>&1
  close_prompts
  sleep 2
  rm -f "$data/watch.pid"
  SCOOCH_TRACE=1 "$daemon" watch >>"$out/watch.log" 2>&1 &
  line "- booted out the login item job and started $daemon watch from the probe as $!, which inherits the runner's Finder automation"
}

: >"$report"
line "## App probe on $(sw_vers -productName) $(sw_vers -productVersion) $(uname -m)"
line
dmg=$(ls "$built"/*.dmg | head -1)
line "- disk image $(basename "$dmg"), $(wc -c <"$dmg" | tr -d ' ') bytes"

limited 20 osascript -e "tell application \"Finder\" to set desktop picture to POSIX file \"$photo\"" >/dev/null 2>&1
line "- wallpaper before install $(wallpaper)"

mount=$(mktemp -d)
hdiutil attach -nobrowse -quiet -mountpoint "$mount" "$dmg"
ditto "$mount/Scooch.app" "$app"
hdiutil detach -quiet "$mount"
line "- copied Scooch.app to Applications, $(du -sk "$app" | cut -f1) KB"
line "- programs inside $(ls "$app/Contents/MacOS" | tr '\n' ' ')"
codesign -dv "$app" 2>&1 | grep -E "Identifier|Signature|TeamIdentifier" | sed 's/^/- signature /' | tee -a "$report"

xattr -w com.apple.quarantine "0081;$(printf %x "$(date +%s)");Safari;" "$app"
line "- Gatekeeper on a downloaded copy, word for word: $(spctl --assess --type execute -vvv "$app" 2>&1 | tr '\n' ' ')"
limited 15 open "$app" >/dev/null 2>&1 &
sleep 8
limited 20 screencapture -x -D 1 "$out/gatekeeper-open.png"
line "- opened the quarantined copy, screen kept as gatekeeper-open.png, windows now $(limited 20 osascript -e 'tell application "System Events" to get name of every process whose visible is true' 2>&1)"
line "- Gatekeeper log $(limited 60 log show --last 30s --style compact --predicate 'process == "syspolicyd" OR process == "CoreServicesUIAgent"' 2>/dev/null | grep -i -E 'scooch|deny|reject|notariz' | tail -3 | tr '\n' ' ')"
killall CoreServicesUIAgent >/dev/null 2>&1
limited 15 open "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension" >/dev/null 2>&1 &
sleep 10
line "- Privacy and Security says $(limited 60 osascript <<'APPLESCRIPT' 2>&1
tell application "System Events"
  tell process "System Settings"
    set frontmost to true
    set said to ""
    repeat with element in (entire contents of window 1)
      try
        if role of element is "AXStaticText" and (value of element as text) contains "Scooch" then set said to said & (value of element as text) & " | "
      end try
      try
        if role of element is "AXButton" and (name of element as text) contains "Open Anyway" then
          perform action "AXScrollToVisible" of element
          set said to said & "button " & (name of element as text) & " | "
        end if
      end try
    end repeat
    return said
  end tell
end tell
APPLESCRIPT
)"
sleep 2
limited 20 screencapture -x -D 1 "$out/gatekeeper-settings.png"
line "- Privacy and Security after the blocked open kept as gatekeeper-settings.png"
limited 20 osascript -e 'tell application "System Settings" to quit' >/dev/null 2>&1
killall CoreServicesUIAgent >/dev/null 2>&1
pkill -f "Scooch.app/Contents/MacOS/scooch-app" >/dev/null 2>&1
xattr -dr com.apple.quarantine "$app"
xattr -w com.apple.quarantine "0081;$(printf %x "$(date +%s)");Safari;" "$daemon"
limit "the Open Anyway approval needs a person, so the probe clears the quarantine flag of the app and keeps it on the scooch program inside, the case where only the app was approved"

limited 20 open "$app" >/dev/null 2>&1
if took=$(wait_for 40 has_window); then
  line "- window showed after $took s"
else
  line "- no window after 40 s"
fi
sleep 4
shot first-open
if took=$(wait_for 30 state_on); then line "- first open turned Scooch on after $took s"; else line "- first open did not turn Scooch on within 30 s"; fi
if [ -f "$plist" ]; then
  line "- login item points at $(/usr/libexec/PlistBuddy -c 'Print :ProgramArguments:0' "$plist" 2>/dev/null)"
else
  line "- no login item was written"
fi
sleep 3
line "- launchd job $(launchctl print "gui/$(id -u)/$label" 2>&1 | grep -E 'state =|pid =|last exit|spawn type|immediate reason' | tr -s ' ' | tr '\n' ';')"
line "- scooch processes from inside the app $(pgrep -fl "$daemon watch" | tr '\n' ';')"
line "- Gatekeeper log for the program inside $(limited 60 log show --last 2m --style compact --predicate 'process == "syspolicyd" OR process == "launchd"' 2>/dev/null | grep -i 'scooch' | grep -i -E 'deny|reject|quarantin|assess|spawn' | tail -3 | tr '\n' ' ')"
xattr -d com.apple.quarantine "$daemon" 2>/dev/null
sleep 5
shot asking

stand_in
if took=$(wait_for 40 rendered); then line "- Scooch drew the desktop after $took s"; else line "- no render within 40 s"; fi
sleep 3
shot on
line "- wallpaper while on $(wallpaper)"

line "- $(press 'Turn off')"
if took=$(wait_for 30 state_off); then line "- off after $took s"; else line "- still on after 30 s"; fi
sleep 3
shot off
if wallpaper_is_photo; then line "- the wallpaper is the photo again"; else line "- the wallpaper is $(wallpaper), not the photo"; fi
if no_plist; then line "- the login item is gone"; else line "- the login item is still there"; fi

line "- $(press 'Turn on')"
if took=$(wait_for 30 state_on); then line "- on again after $took s"; else line "- not on again within 30 s"; fi
sleep 3
stand_in
wait_for 40 rendered >/dev/null
sleep 3
line "- wallpaper before the Trash $(wallpaper)"
line "- daemon log $(grep 'program watched' "$out/watch.log" | tail -1)"
line "- closing the window $(limited 30 osascript -e 'tell application "System Events" to tell (first process whose bundle identifier is "me.sohanbhat.scooch.window") to click (first button of window 1 whose subrole is "AXCloseButton")' 2>&1)"
if wait_for 15 sh -c '! pgrep -f "Scooch.app/Contents/MacOS/scooch-app" >/dev/null' >/dev/null; then line "- closing the window ended the app process"; else line "- the app process outlived its window"; fi
line "- daemons after close $(pgrep -f 'MacOS/scooch watch' | wc -l | tr -d ' ')"

line "- Trash through Finder $(limited 30 osascript -e 'tell application "Finder" to delete POSIX file "/Applications/Scooch.app"' 2>&1)"
cleared() { wallpaper_is_photo && no_plist && ! pgrep -f 'MacOS/scooch watch' >/dev/null; }
if took=$(wait_for 25 cleared); then
  line "- after the Trash the wallpaper is the photo, the login item is gone and no daemon runs, $took s later"
else
  line "- 25 s after the Trash the wallpaper is $(wallpaper), login item $([ -f "$plist" ] && echo present || echo gone), daemons $(pgrep -f 'MacOS/scooch watch' | wc -l | tr -d ' ')"
fi
line "- daemon log after the Trash $(grep -i -E 'program|trash|moved|off' "$out/watch.log" | tail -4 | tr '\n' ' ')"
pkill -f "Scooch.app/Contents/MacOS/scooch" >/dev/null 2>&1
launchctl bootout "gui/$(id -u)/$label" >/dev/null 2>&1
rm -rf "$HOME/.Trash/Scooch.app"
cp "$data/state.json" "$out/state.json" 2>/dev/null
exit 0
