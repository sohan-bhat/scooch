#!/usr/bin/env bash
set -u

built="$1"
out="$2"
mkdir -p "$out"
report="$out/report.md"
app=/Applications/Scooch.app
daemon="$app/Contents/Helpers/Scooch.app/Contents/MacOS/scooch"
data="$HOME/Library/Application Support/Scooch"
plist="$HOME/Library/LaunchAgents/me.sohanbhat.scooch.plist"
photo="$PWD/assets/photos/seagull-sky.jpg"
label="me.sohanbhat.scooch"
readonly built out report app data plist photo label

line() { echo "$*" | tee -a "$report"; }
. "$(dirname "$0")/expect.sh"
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
        if role of element is in {"AXButton", "AXCheckBox", "AXRadioButton"} then
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

seen_texts() {
  local folder
  folder=$(mktemp -d)
  cat >"$folder/read.swift" <<'SWIFT'
import AppKit
import Vision

let url = URL(fileURLWithPath: CommandLine.arguments[1])
guard let image = NSImage(contentsOf: url), let picture = image.cgImage(forProposedRect: nil, context: nil, hints: nil) else { exit(1) }
let request = VNRecognizeTextRequest()
request.recognitionLevel = .accurate
request.usesLanguageCorrection = false
try VNImageRequestHandler(cgImage: picture).perform([request])
for found in request.results ?? [] {
    if let best = found.topCandidates(1).first { print(best.string) }
}
SWIFT
  limited 180 swift "$folder/read.swift" "$1" 2>/dev/null
  rm -rf "$folder"
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
luma() {
  limited 20 osascript -l JavaScript -e "ObjC.import('AppKit'); const r = \$.NSBitmapImageRep.imageRepWithContentsOfFile('$out/$1.png'); const c = r.colorAtXY(Math.floor(r.pixelsWide / 2), 16).colorUsingColorSpace(\$.NSColorSpace.sRGBColorSpace); c.brightnessComponent" 2>/dev/null || echo 0.5
}
light_shot() { awk -v v="$(luma "$1")" 'BEGIN { exit !(v > 0.6) }'; }
dark_shot() { awk -v v="$(luma "$1")" 'BEGIN { exit !(v < 0.3) }'; }
login_at() { [ "$(/usr/libexec/PlistBuddy -c 'Print :ProgramArguments:0' "$plist" 2>/dev/null)" = "$1" ]; }
no_window_app() { ! pgrep -f "MacOS/scooch-app" >/dev/null; }
app_snapshot() {
  local pids listed holders windows
  pids=$(pgrep -f 'MacOS/scooch-app' | tr '\n' ' ')
  listed=none
  [ -n "${pids// /}" ] && listed=$(ps -o pid=,ppid=,lstart=,args= -p "$(echo $pids | tr ' ' ',')" 2>/dev/null | tr -s ' ' | tr '\n' ';')
  holders=$(lsof -t "$data/icon.lock" 2>/dev/null | tr '\n' ' ')
  windows=$(limited 15 osascript -l JavaScript -e '
    function run(argv) {
      ObjC.import("CoreGraphics");
      const wanted = argv.map(Number);
      const list = ObjC.deepUnwrap(ObjC.castRefToObject($.CGWindowListCopyWindowInfo($.kCGWindowListOptionAll, 0))) || [];
      const mine = list.filter((w) => wanted.includes(w.kCGWindowOwnerPID));
      return mine.map((w) => `${w.kCGWindowOwnerPID} ${w.kCGWindowOwnerName} number ${w.kCGWindowNumber} layer ${w.kCGWindowLayer} ${w.kCGWindowIsOnscreen ? "shown" : "hidden"} at ${w.kCGWindowBounds.X} ${w.kCGWindowBounds.Y} ${w.kCGWindowBounds.Width}x${w.kCGWindowBounds.Height}`).join("; ") || "none";
    }' $pids 2>&1)
  echo "app processes $listed icon.lock open in ${holders:-no process}, which only its holder keeps open, their windows $windows"
}
no_staged() { ! ls -d /Applications/.Scooch.app.new.* "$HOME/Applications/.Scooch.app.new."* >/dev/null 2>&1; }

close_prompts() {
  for owner in UserNotificationCenter universalAccessAuthWarn CoreServicesUIAgent; do
    killall "$owner" >/dev/null 2>&1 && line "- closed a consent window by quitting $owner, which answers nothing"
  done
}

consent_windows() {
  limited 15 osascript -l JavaScript -e '
    ObjC.import("CoreGraphics");
    const list = ObjC.deepUnwrap(ObjC.castRefToObject($.CGWindowListCopyWindowInfo($.kCGWindowListOptionOnScreenOnly, 0))) || [];
    const asking = list.filter((w) => /UserNotificationCenter|universalAccessAuthWarn|CoreServicesUIAgent/.test(w.kCGWindowOwnerName || ""));
    asking.map((w) => `${w.kCGWindowOwnerName} ${w.kCGWindowName || "untitled"} layer ${w.kCGWindowLayer}`).join(", ") || "none";' 2>&1
}

stand_in() {
  local path="${1:-a turn on}" seen="$out/stand-in-$(date +%s)" from survivor status
  mkdir -p "$seen"
  from=$(wc -l <"$data/watch.log" 2>/dev/null || echo 0)
  ps -axo pid,ppid,stat,lstart,comm,args | grep -i -E '[s]cooch' >"$seen/processes-before.txt"
  launchctl print "gui/$(id -u)/$label" >"$seen/launchctl-before.txt" 2>&1
  echo "turn on path: $path" >"$seen/path.txt"
  echo "consent windows at the boot out: $(consent_windows)" >"$seen/consent.txt"
  local item=no
  [ -f "$plist" ] && item=yes
  launchctl bootout "gui/$(id -u)/$label" >"$seen/bootout-stderr.txt" 2>&1
  status=$?
  { echo "exit $status"; echo "stderr:"; cat "$seen/bootout-stderr.txt"; } >"$seen/bootout.txt"
  if [ "$item" = yes ]; then
    expect "the stand-in booted out the login item's job, after $path" [ "$status" = 0 ]
    [ "$status" = 0 ] || line "- the boot out of $label answered $(tr '\n' ' ' <"$seen/bootout-stderr.txt")"
  fi
  rm -f "$seen/bootout-stderr.txt"
  close_prompts
  sleep 2
  if ! wait_for 15 sh -c '! pgrep -f "scooch watch" >/dev/null' >/dev/null; then
    survivor=$(pgrep -f 'scooch watch' | head -1)
    sample "$survivor" 3 -file "$seen/sample.txt" >/dev/null 2>&1
    ps -axo pid,ppid,stat,lstart,comm,args | grep -i -E '[s]cooch' >"$seen/processes-after-15s.txt"
    launchctl print "gui/$(id -u)/$label" >"$seen/launchctl-after-15s.txt" 2>&1
    lsof "$data/watch.lock" >"$seen/lock-holders.txt" 2>&1
    sleep 10
    sample "$survivor" 3 -file "$seen/sample-2.txt" >/dev/null 2>&1
    sed -n "$((from + 1)),\$p" "$data/watch.log" >"$seen/login-item-log.txt" 2>/dev/null
    line "- a daemon still ran 15 s after the boot out, kept in $(basename "$seen"): turn on path $path, $(cat "$seen/consent.txt"), boot out $(head -1 "$seen/bootout.txt"), processes $(pgrep -fl 'scooch watch' | tr '\n' ' '), the lock held by $(lsof -t "$data/watch.lock" 2>/dev/null | while read -r pid; do echo "$pid $(ps -o comm= -p "$pid") parent $(ps -o ppid= -p "$pid" | tr -d ' ') $(ps -o comm= -p "$(ps -o ppid= -p "$pid" | tr -d ' ')")"; done | tr '\n' ';'), the login item's log in those seconds $(tr '\n' '|' <"$seen/login-item-log.txt" | cut -c1-600)"
  else
    rm -rf "$seen"
  fi
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
expect "the disk image opened its window" [ -n "$found" ]
line "- disk image holds $(ls -a "$mount" | tr '\n' ' ')"
ditto "$mount/Scooch.app" "$app"
limited 15 osascript -e 'tell application "Finder" to close every window' >/dev/null 2>&1
hdiutil detach -quiet "$mount" || hdiutil detach -force -quiet "$mount"
line "- copied Scooch.app to Applications, $(du -sk "$app" | cut -f1) KB"
line "- programs inside $(ls "$app/Contents/MacOS" | tr '\n' ' ')"
codesign -dv "$app" 2>&1 | grep -E "Identifier|Signature|TeamIdentifier" | sed 's/^/- signature /' | tee -a "$report"
helper="$app/Contents/Helpers/Scooch.app"
line "- helper bundle $(ls "$helper/Contents" | tr '\n' ' '), its program $(ls "$helper/Contents/MacOS" | tr '\n' ' ')"
line "- codesign --verify --deep --strict on the app $(codesign --verify --deep --strict "$app" 2>&1 && echo passes)"
line "- the helper $(codesign -dv "$helper" 2>&1 | grep -E '^Identifier=|^Info.plist|^Runtime|^CodeDirectory' | tr '\n' ' ')"
line "- the helper names $(/usr/libexec/PlistBuddy -c 'Print :CFBundleDisplayName' "$helper/Contents/Info.plist" 2>&1), version $(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$helper/Contents/Info.plist" 2>&1), sentence $(/usr/libexec/PlistBuddy -c 'Print :NSAppleEventsUsageDescription' "$helper/Contents/Info.plist" 2>&1)"
line "- the helper's entitlements $(codesign -d --entitlements :- "$helper" 2>/dev/null | grep -o 'automation.apple-events' | head -1)"
expect "codesign --verify --deep --strict passes on the app" codesign --verify --deep --strict "$app"
expect "the helper is signed as me.sohanbhat.scooch" sh -c "codesign -dv '$helper' 2>&1 | grep -qx 'Identifier=me.sohanbhat.scooch'"
expect "the helper's Info.plist is bound to its signature" sh -c "codesign -dv '$helper' 2>&1 | grep -q '^Info.plist entries='"
expect "the helper is named Scooch" [ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleDisplayName' "$helper/Contents/Info.plist" 2>&1)" = Scooch ]
expect "the helper carries the purpose sentence" [ "$(/usr/libexec/PlistBuddy -c 'Print :NSAppleEventsUsageDescription' "$helper/Contents/Info.plist" 2>&1)" = "Scooch reads where your desktop icons sit." ]
expect "the helper holds the Apple events entitlement" sh -c "codesign -d --entitlements :- '$helper' 2>/dev/null | grep -q automation.apple-events"
expect "no scooch program is left in the app's own Contents/MacOS" [ ! -e "$app/Contents/MacOS/scooch" ]

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
expect "the older command install turned Scooch on" state_on
line "- reduce motion before $(defaults read com.apple.universalaccess reduceMotion 2>&1)"
defaults write com.apple.universalaccess reduceMotion -bool false >"$out/motion.txt" 2>&1
line "- reduce motion after the probe set it off $(defaults read com.apple.universalaccess reduceMotion 2>&1) $(cat "$out/motion.txt")"
limited 20 open --stderr "$out/app.log" "$app" >/dev/null 2>&1
launch_said() { grep -q 'scooch-app ' "$out/app.log" 2>/dev/null; }
if took=$(wait_for 120 has_window); then
  line "- window showed after $took s, the app says $(tr '\n' ' ' <"$out/app.log" 2>/dev/null)"
else
  line "- no window after 120 s, the app says $(tr '\n' ' ' <"$out/app.log" 2>/dev/null), $(app_snapshot)"
fi
expect "the first launch wrote its own first line, which the window check stands on" wait_for 30 launch_said
expect "the app opened its window within 120 s" has_window
sleep 4
shot first-open
second_log="$out/app-second.log"
before=" $(pgrep -f 'MacOS/scooch-app' | tr '\n' ' ')"
limited 20 open -n --stderr "$second_log" "$app" >/dev/null 2>&1
second_said() { grep -q 'scooch-app ' "$second_log" 2>/dev/null; }
newcomers() { pgrep -f 'MacOS/scooch-app' | while read -r pid; do case "$before " in *" $pid "*) ;; *) echo "$pid" ;; esac; done; }
second_gone() { [ -z "$(newcomers)" ]; }
ended=-1
if said=$(wait_for 60 second_said); then ended=$(wait_for 30 second_gone) || ended=-1; else said=-1; fi
left=$(pgrep -f 'MacOS/scooch-app' | wc -l | tr -d ' ')
line "- a second launch with open -n wrote its first line $said s after open returned and was gone $ended s after that line, leaving $left window process, it says $(tr '\n' ' ' <"$second_log" 2>/dev/null)"
[ "$said" -ge 0 ] && [ "$ended" -ge 0 ] && [ "$left" = 1 ] || line "- after the second launch, $(app_snapshot)"
expect "the second launch wrote its first line within 60 s, which the second launch check stands on" [ "$said" -ge 0 ]
expect "a second launch leaves one window process" eval '[ "$ended" -ge 0 ] && [ "$left" = 1 ]'
if [ "${SCOOCH_EXPERIMENT:-}" = launch ]; then
  line
  line "#### Experiment, a second launch with open -n, many times"
  python3 "$(dirname "$0")/launches-macos.py" "$app" "$out" "$data" 50 | while IFS= read -r row; do line "$row"; done
  cp "$data/watch.log" "$out/watch-data.log" 2>/dev/null
  finish
fi
if took=$(wait_for 30 state_on); then line "- first open turned Scooch on after $took s"; else line "- first open did not turn Scooch on within 30 s"; fi
expect "first open turned Scooch on" state_on
expect "the login item points at the helper's program" login_at "$daemon"
expect "the terminal link points at the helper's program" [ "$(readlink "$HOME/.local/bin/scooch")" = "$daemon" ]
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
line "- the app's Info.plist says $(/usr/libexec/PlistBuddy -c 'Print :NSAppleEventsUsageDescription' "$app/Contents/Info.plist" 2>&1), the program inside carries $(otool -X -s __TEXT __info_plist "$daemon" 2>/dev/null | xxd -r -p 2>/dev/null | grep -A1 NSAppleEvents | tail -1 | sed 's/<[^>]*>//g' | tr -d '\t')"
line "- the system look before the probe, dark $(limited 20 osascript -e 'tell application "System Events" to tell appearance preferences to get dark mode' 2>&1)"
look false
shot look-light-problem
look true
shot look-dark-problem
look false
expect "the window is light in the light look with a problem" light_shot look-light-problem
expect "the window is dark in the dark look with a problem" dark_shot look-dark-problem

consent_owner() {
  for owner in UserNotificationCenter universalAccessAuthWarn CoreServicesUIAgent; do
    count=$(limited 10 osascript -e "tell application \"System Events\" to if exists process \"$owner\" then count windows of process \"$owner\"" 2>/dev/null)
    if [ "${count:-0}" -gt 0 ] 2>/dev/null; then echo "$owner"; return 0; fi
  done
  return 1
}
has_consent() { consent_owner >/dev/null; }
if took=$(wait_for 30 has_consent); then
  owner=$(consent_owner)
  screencapture -x "$out/consent.png"
  words=$(limited 15 osascript -e "tell application \"System Events\" to tell process \"$owner\" to get value of every static text of window 1" 2>&1 | tr '\n' ' ')
  buttons=$(limited 15 osascript -e "tell application \"System Events\" to tell process \"$owner\" to get name of every button of window 1" 2>&1 | tr '\n' ' ')
  sleep 3
  count=$(limited 10 osascript -e "tell application \"System Events\" to count windows of process \"$owner\"" 2>/dev/null)
  screencapture -x "$out/consent.png"
  line "- the consent window came after $took s from $owner, $count window there, screen kept as consent.png"
  line "- it says: $words"
  line "- its buttons: $buttons"
  expect "one consent window" [ "$count" = 1 ]
  expect "the consent window names Scooch" grep -q '“Scooch”' <<<"$words"
  expect "the consent window shows the purpose sentence" grep -q "Scooch reads where your desktop icons sit." <<<"$words"
else
  screencapture -x "$out/consent-none.png"
  line "- no consent window within 30 s, the daemon's log says $(tail -6 "$data/watch.log" 2>/dev/null | tr '\n' ' ')"
  line "- tccd says $(limited 60 log show --last 2m --style compact --predicate 'process == "tccd"' 2>/dev/null | grep -i -E 'scooch|AppleEvents' | tail -5 | tr '\n' ' ')"
  expect "a consent window came from the login item's daemon" false
fi

stand_in "the first open, through turn_on"
if took=$(wait_for 40 rendered); then line "- Scooch drew the desktop after $took s"; else line "- no render within 40 s"; fi
expect "Scooch drew the desktop" rendered
sleep 3
shot on
shot look-light-on
expect "the window is light in the light look while on" light_shot look-light-on
look true
shot look-dark-on
expect "the window is dark in the dark look while on" dark_shot look-dark-on
look false
line "- wallpaper while on $(wallpaper)"
hash -r
line "- after the app opened, login item at $(/usr/libexec/PlistBuddy -c 'Print :ProgramArguments:0' "$plist" 2>/dev/null), daemons $(pgrep -fl 'scooch watch' | awk '{ print $2 }' | tr '\n' ' '), a terminal finds $(command -v scooch) linked to $(readlink "$HOME/.local/bin/scooch")"
line "- the app says $(grep 'scooch-app' "$out/app.log" | tr '\n' ' ')"
glide_before=$(grep -c '"glide": true' "$data/state.json")
line "- $(press 'Slide')"
sleep 2
line "- Slide on before $glide_before, after one press $(grep -c '"glide": true' "$data/state.json")"
expect "one press of Slide flipped it" [ "$glide_before" != "$(grep -c '"glide": true' "$data/state.json")" ]
shot slide

line "- $(press 'Turn off')"
if took=$(wait_for 30 state_off); then line "- off after $took s"; else line "- still on after 30 s"; fi
expect "Turn off turned Scooch off" state_off
sleep 3
shot off
shot look-light-off
expect "the window is light in the light look while off" light_shot look-light-off
look true
shot look-dark-off
expect "the window is dark in the dark look while off" dark_shot look-dark-off
look false
if wallpaper_is_photo; then line "- the wallpaper is the photo again"; else line "- the wallpaper is $(wallpaper), not the photo"; fi
if no_plist; then line "- the login item is gone"; else line "- the login item is still there"; fi
expect "off put the photo back" wallpaper_is_photo
expect "off removed the login item" no_plist

line "- $(press 'Turn on')"
if took=$(wait_for 30 state_on); then line "- on again after $took s"; else line "- not on again within 30 s"; fi
sleep 3
stand_in "Turn on with no selection, through turn_on"
expect "Turn on turned Scooch on again" state_on
wait_for 40 rendered >/dev/null
sleep 3
. "$(dirname "$0")/presses-macos.sh"
line "- wallpaper before the Trash $(wallpaper)"
line "- daemon log $(grep 'program watched' "$out/watch.log" | tail -1)"
. "$(dirname "$0")/tray-macos.sh"
line "- daemons with only the icon $(pgrep -f 'MacOS/scooch watch' | wc -l | tr -d ' ')"

moved=/Applications/Tools/Scooch.app
mkdir -p /Applications/Tools
mv "$app" "$moved"
points_at_move() { [ "$(/usr/libexec/PlistBuddy -c 'Print :ProgramArguments:0' "$plist" 2>/dev/null)" = "$moved/Contents/Helpers/Scooch.app/Contents/MacOS/scooch" ] && [ "$(readlink "$HOME/.local/bin/scooch")" = "$moved/Contents/Helpers/Scooch.app/Contents/MacOS/scooch" ]; }
if took=$(wait_for 25 points_at_move); then line "- moved the app to /Applications/Tools, the login item and the terminal link followed after $took s"; else line "- moved the app to /Applications/Tools, login item $(/usr/libexec/PlistBuddy -c 'Print :ProgramArguments:0' "$plist" 2>/dev/null), link $(readlink "$HOME/.local/bin/scooch") after 25 s"; fi
expect "the login item and the terminal link followed a move within /Applications" points_at_move
line "- Trash through Finder $(limited 30 osascript -e "tell application \"Finder\" to delete POSIX file \"$moved\"" 2>&1)"
cleared() { wallpaper_is_photo && no_plist && ! pgrep -f 'MacOS/scooch watch' >/dev/null && [ ! -L "$HOME/.local/bin/scooch" ] && [ ! -e "$HOME/.local/bin/scooch" ]; }
if took=$(wait_for 25 cleared); then
  expect "the Trash put the photo back, removed the login item and the link and ended the daemon" true
  line "- after the Trash the wallpaper is the photo, the login item and the terminal link are gone and no daemon runs, $took s later"
else
  line "- 25 s after the Trash the wallpaper is $(wallpaper), login item $([ -f "$plist" ] && echo present || echo gone), link $(readlink "$HOME/.local/bin/scooch" || echo gone), daemons $(pgrep -f 'MacOS/scooch watch' | wc -l | tr -d ' ')"
  expect "the Trash put the photo back, removed the login item and the link and ended the daemon" false
fi
if took=$(wait_for 15 no_window_app); then line "- the icon process ended $took s after the Trash had turned Scooch off"; else line "- the icon process still runs after the Trash, $(pgrep -fl 'MacOS/scooch-app' | tr '\n' ';')"; fi
expect "the Trash ended the icon process" no_window_app
line "- daemon log after the Trash $(grep -i -E 'program|trash|moved|off' "$out/watch.log" | tail -4 | tr '\n' ' ')"
pkill -f "Scooch.app/Contents/MacOS/scooch" >/dev/null 2>&1
launchctl bootout "gui/$(id -u)/$label" >/dev/null 2>&1
rm -rf "$HOME/.Trash/Scooch.app" /Applications/Tools
cp "$data/state.json" "$out/state.json" 2>/dev/null

from_where() { ps -axo command= | grep "MacOS/scooch-app" | grep -v grep | head -1 | sed 's|/Contents/MacOS/scooch-app.*||'; }
has_window() { [ -n "$(window_id)" ]; }
quarantine_of() { xattr -p com.apple.quarantine "$1" 2>/dev/null | cut -d';' -f1,3 || echo none; }
open_from_image() {
  pkill -f "MacOS/scooch-app" >/dev/null 2>&1
  sleep 2
  limited 20 open "$mount/Scooch.app" >/dev/null 2>&1
  wait_for 40 has_window >/dev/null
  sleep 4
}
window_texts() {
  limited 90 osascript <<'APPLESCRIPT' 2>&1
tell application "System Events"
  set scoochApp to first process whose bundle identifier is "me.sohanbhat.scooch.window"
  set texts to {}
  repeat with element in (entire contents of window 1 of scoochApp)
    try
      set shown to value of element
      if class of shown is text and shown is not "" then set end of texts to shown
    end try
    try
      set shown to name of element
      if class of shown is text and shown is not "" then set end of texts to shown
    end try
    try
      set shown to description of element
      if class of shown is text and shown is not "" then set end of texts to shown
    end try
    try
      set shown to title of element
      if class of shown is text and shown is not "" then set end of texts to shown
    end try
  end repeat
end tell
set AppleScript's text item delimiters to linefeed
return texts as text
APPLESCRIPT
}
press_move() {
  before=$(pgrep -f "MacOS/scooch-app" | head -1)
  line "- $(press 'Move to Applications')"
  fresh() { pgrep -f "MacOS/scooch-app" | grep -v -x "$before" | grep -q .; }
  if took=$(wait_for 40 fresh); then
    sleep 5
    line "- a new window process started after $took s and runs from $(from_where)"
  else
    line "- no new window process within 40 s, still $(from_where)"
  fi
}

line
line "### Move from a downloaded disk image"
pkill -f "MacOS/scooch-app" >/dev/null 2>&1
xattr -w com.apple.quarantine "00c3;$(printf '%x' "$(date +%s)");Safari;$(uuidgen)" "$dmg"
line "- the disk image carries quarantine $(quarantine_of "$dmg"), a browser download the person has already approved with Open Anyway, since the probe cannot press that button"
mount=$(hdiutil attach -nobrowse "$dmg" | awk -F'\t' '/\/Volumes\// { print $NF }' | tail -1)
line "- mounted at $mount, the app inside carries quarantine $(quarantine_of "$mount/Scooch.app")"
open_from_image
line "- opened from the disk image, the window runs from $(from_where)"
shot move-before
press_move
line "- /Applications/Scooch.app carries quarantine $(quarantine_of /Applications/Scooch.app)"
expect "after Move the new window runs from /Applications/Scooch.app" [ "$(from_where)" = /Applications/Scooch.app ]
expect "the moved copy carries no quarantine" [ -z "$(xattr -p com.apple.quarantine /Applications/Scooch.app 2>/dev/null)" ]
expect "Move left no staged copy" no_staged
shot move-after
pkill -f "MacOS/scooch-app" >/dev/null 2>&1
sleep 2
rm -rf /Applications/Scooch.app
ditto "$mount/Scooch.app" /Applications/Scooch.app
limited 20 open /Applications/Scooch.app >/dev/null 2>&1
wait_for 40 has_window >/dev/null
sleep 4
line "- without the clear, a copy that keeps quarantine $(quarantine_of /Applications/Scooch.app) runs from $(from_where)"
expect "a copy that kept the quarantine would run translocated, which is why Move clears it" grep -q AppTranslocation <<<"$(from_where)"

line
line "### Move over an old app holding a file that cannot be deleted"
pkill -f "MacOS/scooch-app" >/dev/null 2>&1
sleep 2
rm -rf /Applications/Scooch.app
ditto "$mount/Scooch.app" /Applications/Scooch.app
xattr -dr com.apple.quarantine /Applications/Scooch.app
mkdir -p /Applications/Scooch.app/Contents/Resources/stuck
echo stuck >/Applications/Scooch.app/Contents/Resources/stuck/file
chflags uchg /Applications/Scooch.app/Contents/Resources/stuck/file
line "- the old app holds a file this account cannot delete, rm says $(rm -f /Applications/Scooch.app/Contents/Resources/stuck/file 2>&1 | head -1)"
open_from_image
press_move
whole_app() { codesign --verify --deep --strict /Applications/Scooch.app && [ ! -e /Applications/Scooch.app/Contents/Resources/stuck ]; }
line "- old apps set aside $(ls -d /Applications/.Scooch.app.old.* 2>/dev/null | tr '\n' ' ')"
expect "after a Move over an old app with a stuck file the window runs from /Applications/Scooch.app" [ "$(from_where)" = /Applications/Scooch.app ]
expect "the app in /Applications is whole and holds nothing of the old one" whole_app
expect "the replace left no staged copy" no_staged
pkill -f "MacOS/scooch-app" >/dev/null 2>&1
sleep 2
for old in /Applications/.Scooch.app.old.*; do
  [ -e "$old" ] || continue
  chflags -R nouchg "$old"
  rm -rf "$old"
done

line
line "### Move when the app in /Applications belongs to another account"
pkill -f "MacOS/scooch-app" >/dev/null 2>&1
sleep 2
rm -rf /Applications/Scooch.app
sudo ditto "$mount/Scooch.app" /Applications/Scooch.app
sudo chown -R root:wheel /Applications/Scooch.app
sudo chmod -R go-w /Applications/Scooch.app
line "- /Applications/Scooch.app now belongs to $(stat -f %Su /Applications/Scooch.app), removing it as this account fails: $(rm -rf /Applications/Scooch.app/Contents/Info.plist 2>&1 | head -1)"
open_from_image
press_move
expect "Move fell back to ~/Applications when the old app could not be replaced" [ "$(from_where)" = "$HOME/Applications/Scooch.app" ]
expect "the failed replace left no staged copy" no_staged
pkill -f "MacOS/scooch-app" >/dev/null 2>&1
sleep 2
sudo rm -rf /Applications/Scooch.app "$HOME/Applications/Scooch.app"

line
line "### Move when /Applications cannot be written"
mode=$(stat -f %Lp /Applications)
pkill -f "MacOS/scooch-app" >/dev/null 2>&1
rm -rf /Applications/Scooch.app
sudo chmod 755 /Applications
line "- /Applications mode $mode set to $(stat -f %Lp /Applications) for the probe"
open_from_image
press_move
line "- ~/Applications/Scooch.app is there $([ -d "$HOME/Applications/Scooch.app" ] && echo yes || echo no)"
expect "with /Applications locked Move went to ~/Applications" [ "$(from_where)" = "$HOME/Applications/Scooch.app" ]
ln -sf /Applications/Tools/Scooch.app/Contents/Helpers/Scooch.app/Contents/MacOS/scooch "$HOME/.local/bin/scooch"
line "- a link at ~/.local/bin/scooch points at a bundle that is gone, $(readlink "$HOME/.local/bin/scooch")"
line "- $(press 'Turn on')"
wait_for 30 state_on >/dev/null
sleep 5
line "- after Turn on the link points at $(readlink "$HOME/.local/bin/scooch"), the login item at $(/usr/libexec/PlistBuddy -c 'Print :ProgramArguments:0' "$plist" 2>/dev/null)"
expect "a link to a gone Scooch bundle was linked again to the helper in ~/Applications" [ "$(readlink "$HOME/.local/bin/scooch")" = "$HOME/Applications/Scooch.app/Contents/Helpers/Scooch.app/Contents/MacOS/scooch" ]
expect "the login item points at the helper in ~/Applications" login_at "$HOME/Applications/Scooch.app/Contents/Helpers/Scooch.app/Contents/MacOS/scooch"
daemon="$HOME/Applications/Scooch.app/Contents/Helpers/Scooch.app/Contents/MacOS/scooch"
stand_in "Turn on from ~/Applications with no selection, through turn_on"
wait_for 40 rendered >/dev/null
sleep 5
limited 30 osascript -e 'tell application "System Events" to tell (first process whose bundle identifier is "me.sohanbhat.scooch.window") to click (first button of window 1 whose subrole is "AXCloseButton")' >/dev/null 2>&1
sleep 3
line "- Trash through Finder $(limited 30 osascript -e "tell application \"Finder\" to delete POSIX file \"$HOME/Applications/Scooch.app\"" 2>&1)"
if took=$(wait_for 25 cleared); then
  expect "the Trash from ~/Applications turned Scooch off and cleared the login item and the link" true
  line "- from ~/Applications the Trash also turned Scooch off, the login item and the link are gone and no daemon runs, $took s later"
else
  line "- 25 s after the Trash from ~/Applications the wallpaper is $(wallpaper), login item $([ -f "$plist" ] && echo present || echo gone), link $(readlink "$HOME/.local/bin/scooch" || echo gone), daemons $(pgrep -f 'MacOS/scooch watch' | wc -l | tr -d ' ')"
  expect "the Trash from ~/Applications turned Scooch off and cleared the login item and the link" false
fi

line
line "### Move when no Applications folder can be written"
pkill -f "Scooch.app/Contents/MacOS/scooch" >/dev/null 2>&1
launchctl bootout "gui/$(id -u)/$label" >/dev/null 2>&1
rm -rf "$HOME/.Trash/Scooch.app" "$HOME/Applications/Scooch.app"
chmod 555 "$HOME/Applications"
open_from_image
pressed=$(press 'Move to Applications')
line "- $pressed"
sleep 6
line "- the window still runs from $(from_where)"
sentence=$(ruby -rjson -e 'puts JSON.parse(File.read(ARGV[0]))["failure"]["move"]' crates/scooch-app/ui/src/copy/en.json)
strings="$out/copy-strings.txt"
ruby -rjson -e 'def flat(node); node.is_a?(Hash) ? node.values.flat_map { |v| flat(v) } : [node.to_s]; end; puts flat(JSON.parse(File.read(ARGV[0])))' crates/scooch-app/ui/src/copy/en.json >"$strings"
shot move-failed
said=$(window_texts)
seen=$(seen_texts "$out/move-failed.png")
shown=$(printf '%s\n%s\n' "$said" "$seen")
line "- the window's accessibility texts read $(printf '%s' "$said" | tr '\n' '|')"
line "- the window's own pixels read $(printf '%s' "$seen" | tr '\n' '|')"
line "- the copy file says \"$sentence\""
expect "the Move button was pressed" grep -q "^pressed Move to Applications" <<<"$pressed"
copy_sentence() {
  [ -n "$sentence" ] && grep -qxF -- "$sentence" <<<"$shown" || return 1
  ! grep -F -- "$sentence" <<<"$shown" | grep -qvxF -- "$sentence" || return 1
  ! grep -qiE 'error|denied|not permitted|errno|no such file|read-only|os error' <<<"$shown"
}
expect "the window shows the copy sentence for a failed Move as its whole line and no system text" copy_sentence
expect "with no folder to take it the window stays where it was" grep -q -E 'AppTranslocation|^/Volumes/' <<<"$(from_where)"
expect "the failed Move left no staged copy" no_staged
chmod 755 "$HOME/Applications"
sudo chmod "$mode" /Applications
line "- /Applications mode back to $(stat -f %Lp /Applications)"
pkill -f "MacOS/scooch-app" >/dev/null 2>&1
hdiutil detach -quiet "$mount" || hdiutil detach -force -quiet "$mount"
finish
