line
line "### Every control pressed once"
pics="$HOME/probe-pictures"
mkdir -p "$pics"
cp "$PWD/assets/photos/lone-canoe.jpg" "$pics/probe-canoe.jpg"
cp "$PWD/assets/photos/balloon-sky.jpg" "$pics/probe-sky.jpg"
cp "$PWD/assets/photos/fox.jpg" "$pics/probe-pick.jpg"
printf 'these are words and no picture\n' >"$pics/probe-notes.jpg"
cp "$PWD/.github/app/flip-right.png" "$pics/probe-flip.png"
appdata="$HOME/Library/Application Support/me.sohanbhat.scooch.window"
mkdir -p "$appdata"
python3 - "$appdata/pictures.json" "$pics/probe-canoe.jpg" "$pics/probe-sky.jpg" "$photo" <<'PY'
import json, sys
json.dump(sys.argv[2:], open(sys.argv[1], "w"))
PY
unreadable=$(python3 -c 'import json, sys; print(json.load(open(sys.argv[1]))["failure"]["use"])' crates/scooch-app/ui/src/copy/en.json)

states() {
  limited 120 osascript - "$1" <<'APPLESCRIPT' 2>&1
on run argv
  set AppleScript's text item delimiters to ","
  set wanted to text items of (item 1 of argv)
  set told to {}
  tell application "System Events"
    set scoochApp to first process whose bundle identifier is "me.sohanbhat.scooch.window"
    set everything to entire contents of window 1 of scoochApp
    repeat with label in wanted
      set found to "missing"
      repeat with element in everything
        try
          if role of element is in {"AXCheckBox", "AXRadioButton"} then
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
            if seen contains (label as text) then
              set raw to value of element
              if (raw as text) is in {"1", "true"} then
                set found to "on"
              else
                set found to "off"
              end if
              exit repeat
            end if
          end if
        end try
      end repeat
      set end of told to (label as text) & " " & found
    end repeat
  end tell
  set AppleScript's text item delimiters to "; "
  return "states " & (told as text)
end run
APPLESCRIPT
}
chosen() { states "$1" | grep -q -F "$1 on"; }
unchosen() { states "$1" | grep -q -F "$1 off"; }
digest() { md5 -q "$(wallpaper)" 2>/dev/null; }
ours() { case "$(wallpaper)" in "$data"/out/*) true ;; *) false ;; esac; }
real() { python3 -c 'import os, sys; print(os.path.realpath(sys.argv[1]))' "$1"; }
saved() {
  python3 - "$data/state.json" "$1" <<'PY'
import json, sys
value = json.load(open(sys.argv[1]))
for part in sys.argv[2].split("."):
    value = value.get(part) if isinstance(value, dict) else None
print(json.dumps(value))
PY
}
sources() {
  python3 - "$data/state.json" <<'PY'
import json, sys
state = json.load(open(sys.argv[1]))
print(" ".join(sorted({entry.get("source", "") for entry in state.get("screens", {}).values()})))
PY
}
screens_say() {
  python3 - "$data/state.json" "$1" "$2" <<'PY'
import json, sys
screens = list(json.load(open(sys.argv[1])).get("screens", {}).values())
def get(entry):
    value = entry
    for part in sys.argv[2].split("."):
        value = value.get(part) if isinstance(value, dict) else None
    return json.dumps(value)
sys.exit(0 if screens and all(get(entry) == sys.argv[3] for entry in screens) else 1)
PY
}
names() { [ "$(sources)" = "$(real "$1")" ]; }
fresh_from() { names "$1" && ours && [ "$(digest)" != "$2" ]; }
status_text() { "$daemon" status 2>&1 | tr '\n' '|'; }
watching() { pgrep -f 'scooch watch' >/dev/null; }
no_watch() { ! pgrep -f 'scooch watch' >/dev/null; }
alive_on() { status_text | grep -q "^Scooch is on\.|" && watching; }
panel_shown() {
  limited 15 osascript -l JavaScript -e '
    ObjC.import("CoreGraphics");
    const list = ObjC.deepUnwrap(ObjC.castRefToObject($.CGWindowListCopyWindowInfo($.kCGWindowListOptionOnScreenOnly, 0))) || [];
    const owners = ["Scooch", "scooch-app"];
    const panels = list.filter((w) => w.kCGWindowLayer === 0 && (/save panel|openandsavepanel/i.test(w.kCGWindowOwnerName || "") || (owners.includes(w.kCGWindowOwnerName) && (w.kCGWindowName || "") !== "Scooch")));
    panels.map((w) => `${w.kCGWindowOwnerName} ${w.kCGWindowName || "untitled"}`).join(", ");' 2>/dev/null
}
pick_file() {
  local file="$1" shown="" after step
  press 'Pick a file' >/dev/null
  for step in $(seq 1 20); do
    shown=$(panel_shown)
    [ -n "$shown" ] && break
    sleep 0.5
  done
  limited 60 osascript - "$file" <<'APPLESCRIPT' >/dev/null 2>&1
on run argv
  set wanted to item 1 of argv
  tell application "System Events"
    tell (first process whose bundle identifier is "me.sohanbhat.scooch.window")
      set frontmost to true
      delay 0.5
      keystroke "g" using {command down, shift down}
      delay 1.5
      keystroke wanted
      delay 1.5
      key code 36
      delay 2
      key code 36
    end tell
  end tell
end run
APPLESCRIPT
  for step in $(seq 1 20); do
    after=$(panel_shown)
    [ -z "$after" ] && break
    sleep 0.5
  done
  if [ -n "$shown" ] && [ -z "$after" ]; then
    echo "the file window, $shown, took $file and closed"
  else
    echo "the file window did not show as expected, seen [$shown], after [$after], for $file"
  fi
}
press_switch() { press "$1" >/dev/null; sleep 1; }
close_window() { limited 30 osascript -e 'tell application "System Events" to tell (first process whose bundle identifier is "me.sohanbhat.scooch.window") to click (first button of window 1 whose subrole is "AXCloseButton")' >/dev/null 2>&1; wait_for 15 window_gone >/dev/null; if state_off; then wait_for 15 no_window_app >/dev/null; fi; }
window_gone() { [ -z "$(window_id)" ]; }
open_window() { limited 20 open "$app" >/dev/null 2>&1; wait_for 40 has_window >/dev/null; sleep 4; }

close_window
open_window
line "- the row and switches read $(states 'probe canoe,probe sky,seagull sky,Slide,Clouds,Flip,Quick drop')"

line
line "#### A picture in the row and the way back"
before=$(digest)
line "- $(press 'probe canoe')"
wait_for 40 fresh_from "$pics/probe-canoe.jpg" "$before" >/dev/null
sleep 2
line "- the row reads $(states 'probe canoe,seagull sky'), the saved state names $(sources), the wallpaper is $(wallpaper)"
expect "pressing probe canoe in the row marks it as chosen" chosen "probe canoe"
expect "the saved state names probe canoe" names "$pics/probe-canoe.jpg"
expect "the desktop shows a new output made from probe canoe" fresh_from "$pics/probe-canoe.jpg" "$before"
before=$(digest)
line "- $(press 'seagull sky')"
wait_for 40 fresh_from "$photo" "$before" >/dev/null
sleep 2
line "- the row reads $(states 'probe canoe,seagull sky'), the saved state names $(sources), the wallpaper is $(wallpaper)"
way_back() { chosen "seagull sky" && unchosen "probe canoe"; }
expect "the way back marks seagull sky as chosen and probe canoe no more" way_back
expect "the way back saves seagull sky" names "$photo"
expect "the way back shows a new output made from seagull sky" fresh_from "$photo" "$before"

line
line "#### Pick a file"
before=$(digest)
picked=$(pick_file "$pics/probe-pick.jpg")
line "- $picked"
wait_for 40 fresh_from "$pics/probe-pick.jpg" "$before" >/dev/null
sleep 2
line "- the row reads $(states 'probe pick'), the saved state names $(sources), the wallpaper is $(wallpaper)"
expect "Pick a file opened the system's file window, which took the photo and closed" grep -q "took .* and closed" <<<"$picked"
expect "the picked photo joins the row as chosen" chosen "probe pick"
expect "the saved state names the picked photo" names "$pics/probe-pick.jpg"
expect "the desktop shows a new output made from the picked photo" fresh_from "$pics/probe-pick.jpg" "$before"
before=$(digest)
shown_before=$(wallpaper)
picked=$(pick_file "$pics/probe-notes.jpg")
line "- $picked"
sleep 4
shot unreadable-pick
seen=$(seen_texts "$out/unreadable-pick.png")
line "- the window's pixels read $(printf '%s' "$seen" | tr '\n' '|'), the copy file says \"$unreadable\", the wallpaper is $(wallpaper)"
expect "the file window also took a file that is no picture" grep -q "took .* and closed" <<<"$picked"
expect "a picked file that is no picture shows the window's sentence for it" grep -qxF "$unreadable" <<<"$seen"
same_desktop() { [ "$(wallpaper)" = "$shown_before" ] && [ "$(digest)" = "$before" ] && names "$pics/probe-pick.jpg"; }
expect "a picked file that is no picture leaves the desktop and the saved picture as they were" same_desktop

line
line "#### Clouds on a sky photo, with the icon column full"
desk="$HOME/Desktop"
mkdir -p "$desk"
for n in $(seq -w 1 90); do : >"$desk/probe-icon-$n.txt"; done
sleep 6
before=$(digest)
line "- $(press 'probe sky')"
wait_for 40 fresh_from "$pics/probe-sky.jpg" "$before" >/dev/null
sleep 5
with_clouds=$(digest)
line "- clouds saved as $(saved options.scenery), scooch status says $(status_text)"
press_switch Clouds
clouds_off() { [ "$(saved options.scenery)" = '"off"' ] && [ "$(digest)" != "$with_clouds" ]; }
wait_for 40 clouds_off >/dev/null
sleep 2
without_clouds=$(digest)
line "- after one press clouds saved as $(saved options.scenery), the switch reads $(states Clouds), scooch status says $(status_text)"
expect "one press of Clouds saves clouds off" [ "$(saved options.scenery)" = '"off"' ]
expect "the Clouds switch reads off" unchosen Clouds
expect "scooch status drops the scenery line" eval '! status_text | grep -q "Scenery  on"'
expect "the sky photo without clouds is a new output unlike the one with them" [ "$without_clouds" != "$with_clouds" ]
press_switch Clouds
clouds_on() { [ "$(saved options.scenery)" != '"off"' ] && [ "$(digest)" != "$without_clouds" ]; }
wait_for 40 clouds_on >/dev/null
sleep 2
line "- after the second press clouds saved as $(saved options.scenery), the switch reads $(states Clouds), scooch status says $(status_text)"
expect "the second press of Clouds saves clouds on" [ "$(saved options.scenery)" != '"off"' ]
expect "the Clouds switch reads on again" chosen Clouds
expect "scooch status shows the scenery line again" eval 'status_text | grep -q "Scenery  on"'
expect "the sky photo with clouds again is a new output unlike the one without" [ "$(digest)" != "$without_clouds" ]

for switch in "Slide glide" "Quick drop predict"; do
  shown_switch=${switch% *}
  key=${switch##* }
  line
  line "#### $shown_switch"
  if states "$shown_switch" | grep -q -F "$shown_switch missing"; then
    line "- $shown_switch is not shown on this system, so it is not pressed here"
    continue
  fi
  was=$(saved "$key")
  press_switch "$shown_switch"
  flipped() { [ "$(saved "$key")" != "$was" ]; }
  wait_for 10 flipped >/dev/null
  sleep 2
  line "- $shown_switch was saved as $was and after one press as $(saved "$key"), the switch reads $(states "$shown_switch"), scooch status says $(status_text)"
  expect "one press of $shown_switch flips its saved value" flipped
  if [ "$was" = true ]; then expect "the $shown_switch switch reads off" unchosen "$shown_switch"; else expect "the $shown_switch switch reads on" chosen "$shown_switch"; fi
  expect "after $shown_switch scooch status still says on and the daemon is alive" alive_on
  press_switch "$shown_switch"
  back() { [ "$(saved "$key")" = "$was" ]; }
  wait_for 10 back >/dev/null
  sleep 2
  line "- after the second press $shown_switch is saved as $(saved "$key"), the switch reads $(states "$shown_switch"), scooch status says $(status_text)"
  expect "the second press of $shown_switch brings its saved value back" back
  if [ "$was" = true ]; then expect "the $shown_switch switch reads on again" chosen "$shown_switch"; else expect "the $shown_switch switch reads off again" unchosen "$shown_switch"; fi
  expect "after the second press of $shown_switch scooch status still says on and the daemon is alive" alive_on
done

line
line "#### Flip with the icon column full"
"$daemon" use "$pics/probe-flip.png" >"$out/flip-use.txt" 2>&1
helps() { names "$pics/probe-flip.png" && ours && screens_say flip_helps true && status_text | grep -q "Run scooch flip"; }
wait_for 60 helps >/dev/null
sleep 2
before=$(digest)
flipcase="$out/flip-case"
mkdir -p "$flipcase"
cp "$pics/probe-flip.png" "$flipcase/picture.png"
log_from=$(wc -l <"$out/watch.log")
cp "$data/state.json" "$flipcase/state-before.json"
"$daemon" render "$pics/probe-flip.png" --out "$flipcase/render-now.jpg" --json "$flipcase/scene.json" >"$flipcase/render.txt" 2>&1
system_profiler SPDisplaysDataType >"$flipcase/displays.txt" 2>&1
limited 15 osascript -l JavaScript -e 'ObjC.import("AppKit"); const s = $.NSScreen.mainScreen; `frame ${s.frame.size.width}x${s.frame.size.height} scale ${s.backingScaleFactor}`' >"$flipcase/screen.txt" 2>&1
echo "glide $(saved glide), predict $(saved predict), scenery $(saved options.scenery)" >"$flipcase/switches.txt"
ps -axo pid,ppid,stat,comm,args | grep -i -E '[s]cooch' >"$flipcase/processes-before.txt"
line "- with $(ls "$desk" | grep -c '^probe-icon-') files on the Desktop and the subject on the right, flip_helps $(python3 -c 'import json,sys; print([e.get("flip_helps", False) for e in json.load(open(sys.argv[1]))["screens"].values()])' "$data/state.json"), mirror $(saved options.mirror), scooch status says $(status_text)"
expect "before Flip the saved state says flipping would help and scooch status shows the flip hint" helps
press_switch Flip
mirrored() { [ "$(saved options.mirror)" = true ] && screens_say placement.mirrored true && [ "$(digest)" != "$before" ]; }
wait_for 60 mirrored >/dev/null
sleep 2
flipped_out=$(digest)
cp "$data/state.json" "$flipcase/state-after-first-press.json"
ps -axo pid,ppid,stat,comm,args | grep -i -E '[s]cooch' >"$flipcase/processes-after-first-press.txt"
line "- after one press of Flip mirror is saved as $(saved options.mirror), the switch reads $(states Flip), the placement is mirrored $(python3 -c 'import json,sys; print([(e.get("placement") or {}).get("mirrored") for e in json.load(open(sys.argv[1]))["screens"].values()])' "$data/state.json"), scooch status says $(status_text)"
expect "one press of Flip saves flip on" [ "$(saved options.mirror)" = true ]
expect "the Flip switch reads on" chosen Flip
expect "the daemon mirrored the picture into a new output" mirrored
expect "scooch status drops the flip hint" eval '! status_text | grep -q "Run scooch flip"'
press_switch Flip
unmirrored() { [ "$(saved options.mirror)" = false ] && screens_say placement.mirrored false && [ "$(digest)" != "$flipped_out" ]; }
wait_for 60 unmirrored >/dev/null
sleep 2
line "- after the second press mirror is saved as $(saved options.mirror), the switch reads $(states Flip), scooch status says $(status_text)"
expect "the second press of Flip saves flip off" [ "$(saved options.mirror)" = false ]
expect "the Flip switch reads off again" unchosen Flip
expect "the daemon put the picture back unmirrored in a new output" unmirrored
cp "$data/state.json" "$flipcase/state-after-second-press.json"
sed -n "$((log_from + 1)),\$p" "$out/watch.log" >"$flipcase/watch.log"
line "- the Flip case is kept in flip-case: $(ls "$flipcase" | tr '\n' ' '), $(cat "$flipcase/screen.txt"), $(cat "$flipcase/switches.txt"), daemon processes at the first press $(awk '{ print $4 }' "$flipcase/processes-after-first-press.txt" | sort | uniq -c | tr -s ' ' | tr '\n' ';'), ticks $(grep -o '"[^"]*@", [A-Za-z]*' "$flipcase/watch.log" | awk '{ print $2 }' | tr -d ')' | tr '\n' ' ')"
rm -f "$desk"/probe-icon-*.txt
sleep 3
line "- the Desktop files of the Flip case are removed, $(ls "$desk" | grep -c '^probe-icon-') left"

line
line "#### A picture pressed while Scooch is off"
line "- $(press 'Turn off')"
wait_for 30 state_off >/dev/null
wait_for 30 no_watch >/dev/null
sleep 3
shown_off=$(wallpaper)
state_sum=$(md5 -q "$data/state.json")
line "- off, wallpaper $shown_off, login item $([ -f "$plist" ] && echo present || echo gone), daemons $(pgrep -f 'scooch watch' | wc -l | tr -d ' ')"
line "- $(press 'probe canoe')"
sleep 5
line "- the row reads $(states 'probe canoe,seagull sky'), wallpaper $(wallpaper), login item $([ -f "$plist" ] && echo present || echo gone), daemons $(pgrep -f 'scooch watch' | wc -l | tr -d ' '), selection file $(cat "$appdata/selected.json" 2>/dev/null)"
expect "a picture pressed while off is marked as chosen" chosen "probe canoe"
expect "pressed while off the desktop keeps its wallpaper" [ "$(wallpaper)" = "$shown_off" ]
expect "pressed while off no login item is written" no_plist
expect "pressed while off no daemon runs" no_watch
expect "pressed while off Scooch's saved state is untouched" [ "$(md5 -q "$data/state.json")" = "$state_sum" ]
close_window
open_window
line "- after closing and opening the window the row reads $(states 'probe canoe,seagull sky')"
expect "the selection survives closing and opening the window" chosen "probe canoe"
expect "opening the window again with a selection turned nothing on" no_watch
before=$(md5 -q "$shown_off" 2>/dev/null)
line "- $(press 'Turn on')"
wait_for 30 state_on >/dev/null
wait_for 30 test -f "$plist" >/dev/null
wait_for 30 launchctl print "gui/$(id -u)/$label" >/dev/null
sleep 3
stand_in "Turn on with a selection, through use_picture"
wait_for 40 fresh_from "$pics/probe-canoe.jpg" "$before" >/dev/null
sleep 3
line "- after Turn on the saved state names $(sources), the wallpaper is $(wallpaper), login item $([ -f "$plist" ] && echo present || echo gone)"
expect "Turn on after the press shows that picture" fresh_from "$pics/probe-canoe.jpg" "$before"
expect "Turn on after the press writes the login item" [ -f "$plist" ]
line
