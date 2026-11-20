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
        if role of element is in {"AXButton", "AXCheckBox"} then
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

mount=$(hdiutil attach "$dmg" | awk -F'\t' '/\/Volumes\// { print $NF }' | tail -1)
line "- the disk image mounted at $mount"
limited 15 open "$mount" >/dev/null 2>&1
sleep 5
finder_window() {
  limited 15 osascript -l JavaScript -e '
    ObjC.import("CoreGraphics");
    const list = ObjC.deepUnwrap(ObjC.castRefToObject($.CGWindowListCopyWindowInfo($.kCGWindowListOptionOnScreenOnly, 0))) || [];
    const mine = list.filter((w) => w.kCGWindowOwnerName === "Finder" && w.kCGWindowName === "Scooch" && w.kCGWindowLayer === 0);
    mine.length ? `${mine[0].kCGWindowNumber} ${mine[0].kCGWindowBounds.Width}x${mine[0].kCGWindowBounds.Height}` : "";
  ' 2>/dev/null
}
found=$(finder_window)
if [ -n "$found" ]; then
  limited 20 screencapture -x -o -l "${found%% *}" "$out/disk-image.png" && line "- disk image window shot disk-image.png at ${found#* }"
else
  limited 20 screencapture -x -D 1 "$out/disk-image-screen.png"
  line "- no disk image window found, took the whole screen instead"
fi
line "- disk image holds $(ls -a "$mount" | tr '\n' ' ')"
ditto "$mount/Scooch.app" "$app"
limited 15 osascript -e 'tell application "Finder" to close every window' >/dev/null 2>&1
hdiutil detach -quiet "$mount" || hdiutil detach -force -quiet "$mount"
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
killall CoreServicesUIAgent >/dev/null 2>&1
pkill -f "Scooch.app/Contents/MacOS/scooch-app" >/dev/null 2>&1
xattr -dr com.apple.quarantine "$app"
xattr -w com.apple.quarantine "0081;$(printf %x "$(date +%s)");Safari;" "$daemon"
limit "the Open Anyway approval needs a person, so the probe clears the quarantine flag of the app and keeps it on the scooch program inside, the case where only the app was approved"

mkdir -p "$HOME/.local/bin"
cp -X "$daemon" "$HOME/.local/bin/scooch"
export PATH="$HOME/.local/bin:$PATH"
"$HOME/.local/bin/scooch" use "$photo" >"$out/old-use.txt" 2>&1
line "- older command install at ~/.local/bin/scooch, as install.sh lays it out, turned Scooch on, login item at $(/usr/libexec/PlistBuddy -c 'Print :ProgramArguments:0' "$plist" 2>/dev/null), daemons $(pgrep -fl 'scooch watch' | awk '{ print $2 }' | tr '\n' ' ')"
line "- reduce motion before $(defaults read com.apple.universalaccess reduceMotion 2>&1)"
defaults write com.apple.universalaccess reduceMotion -bool false >"$out/motion.txt" 2>&1
line "- reduce motion after the probe set it off $(defaults read com.apple.universalaccess reduceMotion 2>&1) $(cat "$out/motion.txt")"
limited 20 open --stderr "$out/app.log" "$app" >/dev/null 2>&1
if took=$(wait_for 40 has_window); then
  line "- window showed after $took s"
else
  line "- no window after 40 s"
fi
sleep 4
shot first-open
limited 20 open -n "$app" >/dev/null 2>&1
sleep 6
line "- a second launch with open -n left $(pgrep -f 'MacOS/scooch-app' | wc -l | tr -d ' ') window process"
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
look() { limited 20 osascript -e "tell application \"System Events\" to tell appearance preferences to set dark mode to $1" >/dev/null 2>&1; sleep 3; }
line "- the system look before the probe, dark $(limited 20 osascript -e 'tell application "System Events" to tell appearance preferences to get dark mode' 2>&1)"
look false
shot look-light-problem
look true
shot look-dark-problem
look false

consent_owner() {
  for owner in UserNotificationCenter universalAccessAuthWarn CoreServicesUIAgent; do
    count=$(limited 10 osascript -e "tell application \"System Events\" to if exists process \"$owner\" then count windows of process \"$owner\"" 2>/dev/null)
    if [ "${count:-0}" -gt 0 ] 2>/dev/null; then echo "$owner"; return 0; fi
  done
  return 1
}
if took=$(wait_for 30 consent_owner); then
  owner=$(consent_owner)
  screencapture -x "$out/consent.png"
  words=$(limited 15 osascript -e "tell application \"System Events\" to tell process \"$owner\" to get value of every static text of window 1" 2>&1 | tr '\n' ' ')
  buttons=$(limited 15 osascript -e "tell application \"System Events\" to tell process \"$owner\" to get name of every button of window 1" 2>&1 | tr '\n' ' ')
  line "- the consent window came after $took s from $owner, screen kept as consent.png"
  line "- it says: $words"
  line "- its buttons: $buttons"
else
  screencapture -x "$out/consent-none.png"
  line "- no consent window within 30 s, the daemon's log says $(tail -6 "$data/watch.log" 2>/dev/null | tr '\n' ' ')"
fi

stand_in
if took=$(wait_for 40 rendered); then line "- Scooch drew the desktop after $took s"; else line "- no render within 40 s"; fi
sleep 3
shot on
shot look-light-on
look true
shot look-dark-on
look false
line "- wallpaper while on $(wallpaper)"
hash -r
line "- after the app opened, login item at $(/usr/libexec/PlistBuddy -c 'Print :ProgramArguments:0' "$plist" 2>/dev/null), daemons $(pgrep -fl 'scooch watch' | awk '{ print $2 }' | tr '\n' ' '), a terminal finds $(command -v scooch) linked to $(readlink "$HOME/.local/bin/scooch")"
line "- the app says $(grep 'scooch-app' "$out/app.log" | tr '\n' ' ')"
glide_before=$(grep -c '"glide": true' "$data/state.json")
line "- $(press 'Slide')"
sleep 2
line "- Slide on before $glide_before, after one press $(grep -c '"glide": true' "$data/state.json")"
shot slide

line "- $(press 'Turn off')"
if took=$(wait_for 30 state_off); then line "- off after $took s"; else line "- still on after 30 s"; fi
sleep 3
shot off
shot look-light-off
look true
shot look-dark-off
look false
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

moved=/Applications/Tools/Scooch.app
mkdir -p /Applications/Tools
mv "$app" "$moved"
points_at_move() { [ "$(/usr/libexec/PlistBuddy -c 'Print :ProgramArguments:0' "$plist" 2>/dev/null)" = "$moved/Contents/MacOS/scooch" ] && [ "$(readlink "$HOME/.local/bin/scooch")" = "$moved/Contents/MacOS/scooch" ]; }
if took=$(wait_for 25 points_at_move); then line "- moved the app to /Applications/Tools, the login item and the terminal link followed after $took s"; else line "- moved the app to /Applications/Tools, login item $(/usr/libexec/PlistBuddy -c 'Print :ProgramArguments:0' "$plist" 2>/dev/null), link $(readlink "$HOME/.local/bin/scooch") after 25 s"; fi
line "- Trash through Finder $(limited 30 osascript -e "tell application \"Finder\" to delete POSIX file \"$moved\"" 2>&1)"
cleared() { wallpaper_is_photo && no_plist && ! pgrep -f 'MacOS/scooch watch' >/dev/null && [ ! -L "$HOME/.local/bin/scooch" ] && [ ! -e "$HOME/.local/bin/scooch" ]; }
if took=$(wait_for 25 cleared); then
  line "- after the Trash the wallpaper is the photo, the login item and the terminal link are gone and no daemon runs, $took s later"
else
  line "- 25 s after the Trash the wallpaper is $(wallpaper), login item $([ -f "$plist" ] && echo present || echo gone), link $(readlink "$HOME/.local/bin/scooch" || echo gone), daemons $(pgrep -f 'MacOS/scooch watch' | wc -l | tr -d ' ')"
fi
line "- daemon log after the Trash $(grep -i -E 'program|trash|moved|off' "$out/watch.log" | tail -4 | tr '\n' ' ')"
pkill -f "Scooch.app/Contents/MacOS/scooch" >/dev/null 2>&1
launchctl bootout "gui/$(id -u)/$label" >/dev/null 2>&1
rm -rf "$HOME/.Trash/Scooch.app" /Applications/Tools
cp "$data/state.json" "$out/state.json" 2>/dev/null

status_text() {
  limited 60 osascript <<'APPLESCRIPT' 2>&1 | tr '\n' ' '
tell application "System Events"
  set scoochApp to first process whose bundle identifier is "me.sohanbhat.scooch.window"
  set found to {}
  repeat with element in (entire contents of window 1 of scoochApp)
    try
      if role of element is "AXStaticText" then
        set seen to value of element as text
        if seen ends with "." then copy seen to end of found
      end if
    end try
  end repeat
  set AppleScript's text item delimiters to " | "
  return found as text
end tell
APPLESCRIPT
}
from_where() { ps -axo command= | grep "MacOS/scooch-app" | grep -v grep | head -1 | sed 's|/Contents/MacOS/scooch-app.*||'; }
has_window() { [ -n "$(window_id)" ]; }
outside_mount() { where=$(from_where); [ -n "$where" ] && [ "${where#"$mount"}" = "$where" ]; }
quarantine_of() { xattr -p com.apple.quarantine "$1" 2>/dev/null | cut -d';' -f1,3 || echo none; }
open_from_image() {
  pkill -f "MacOS/scooch-app" >/dev/null 2>&1
  sleep 2
  limited 20 open "$mount/Scooch.app" >/dev/null 2>&1
  wait_for 40 has_window >/dev/null
  sleep 4
}
press_move() {
  line "- $(press 'Move to Applications')"
  if took=$(wait_for 40 outside_mount); then line "- a new window runs from $(from_where) after $took s"; else line "- no window outside the disk image within 40 s, still $(from_where)"; fi
  sleep 5
}

line
line "### Move from a downloaded disk image"
pkill -f "MacOS/scooch-app" >/dev/null 2>&1
xattr -w com.apple.quarantine "0083;$(printf '%x' "$(date +%s)");Safari;$(uuidgen)" "$dmg"
line "- the disk image carries quarantine $(quarantine_of "$dmg"), as a browser download does"
mount=$(hdiutil attach -nobrowse "$dmg" | awk -F'\t' '/\/Volumes\// { print $NF }' | tail -1)
line "- mounted at $mount, the app inside carries quarantine $(quarantine_of "$mount/Scooch.app")"
open_from_image
line "- opened from the disk image, the window runs from $(from_where) and says $(status_text)"
shot move-before
press_move
line "- /Applications/Scooch.app carries quarantine $(quarantine_of /Applications/Scooch.app), the window says $(status_text)"
shot move-after

line
line "### Move when /Applications cannot be written"
mode=$(stat -f %Lp /Applications)
pkill -f "MacOS/scooch-app" >/dev/null 2>&1
rm -rf /Applications/Scooch.app
sudo chmod 755 /Applications
line "- /Applications mode $mode set to $(stat -f %Lp /Applications) for the probe"
open_from_image
press_move
line "- ~/Applications/Scooch.app is there $([ -d "$HOME/Applications/Scooch.app" ] && echo yes || echo no), the window says $(status_text)"
ln -sf /Applications/Tools/Scooch.app/Contents/MacOS/scooch "$HOME/.local/bin/scooch"
line "- a link at ~/.local/bin/scooch points at a bundle that is gone, $(readlink "$HOME/.local/bin/scooch")"
line "- $(press 'Turn on')"
wait_for 30 state_on >/dev/null
sleep 5
line "- after Turn on the link points at $(readlink "$HOME/.local/bin/scooch"), the login item at $(/usr/libexec/PlistBuddy -c 'Print :ProgramArguments:0' "$plist" 2>/dev/null)"
daemon="$HOME/Applications/Scooch.app/Contents/MacOS/scooch"
stand_in
wait_for 40 rendered >/dev/null
sleep 5
limited 30 osascript -e 'tell application "System Events" to tell (first process whose bundle identifier is "me.sohanbhat.scooch.window") to click (first button of window 1 whose subrole is "AXCloseButton")' >/dev/null 2>&1
sleep 3
line "- Trash through Finder $(limited 30 osascript -e "tell application \"Finder\" to delete POSIX file \"$HOME/Applications/Scooch.app\"" 2>&1)"
if took=$(wait_for 25 cleared); then
  line "- from ~/Applications the Trash also turned Scooch off, the login item and the link are gone and no daemon runs, $took s later"
else
  line "- 25 s after the Trash from ~/Applications the wallpaper is $(wallpaper), login item $([ -f "$plist" ] && echo present || echo gone), link $(readlink "$HOME/.local/bin/scooch" || echo gone), daemons $(pgrep -f 'MacOS/scooch watch' | wc -l | tr -d ' ')"
fi

line
line "### Move when no Applications folder can be written"
pkill -f "Scooch.app/Contents/MacOS/scooch" >/dev/null 2>&1
launchctl bootout "gui/$(id -u)/$label" >/dev/null 2>&1
rm -rf "$HOME/.Trash/Scooch.app" "$HOME/Applications/Scooch.app"
chmod 555 "$HOME/Applications"
open_from_image
line "- $(press 'Move to Applications')"
sleep 6
line "- the window still runs from $(from_where) and says $(status_text)"
shot move-failed
chmod 755 "$HOME/Applications"
sudo chmod "$mode" /Applications
line "- /Applications mode back to $(stat -f %Lp /Applications)"
pkill -f "MacOS/scooch-app" >/dev/null 2>&1
hdiutil detach -quiet "$mount" || hdiutil detach -force -quiet "$mount"
exit 0
