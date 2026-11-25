line
line "### Every control pressed once"
pics="$HOME/probe-pictures"
mkdir -p "$pics"
cp "$PWD/assets/photos/lone-canoe.jpg" "$pics/probe-canoe.jpg"
cp "$PWD/assets/photos/balloon-sky.jpg" "$pics/probe-sky.jpg"
cp "$PWD/assets/photos/fox.jpg" "$pics/probe-pick.jpg"
printf 'these are words and no picture\n' >"$pics/probe-notes.jpg"
cp "$PWD/.github/app/flip-left.png" "$pics/probe-flip.png"
appdata="${XDG_DATA_HOME:-$HOME/.local/share}/me.sohanbhat.scooch.window"
mkdir -p "$appdata"
python3 - "$appdata/pictures.json" "$pics/probe-canoe.jpg" "$pics/probe-sky.jpg" "$photo" <<'PY'
import json, sys
json.dump(sys.argv[2:], open(sys.argv[1], "w"))
PY
unreadable=$(python3 -c 'import json, sys; print(json.load(open(sys.argv[1]))["failure"]["use"])' crates/scooch-app/ui/src/copy/en.json)

ctl() { python3 "$PWD/.github/app/controls.py" "$@" 2>&1 | tail -1; }
chosen() { ctl "$1" states | grep -q -F "$1 on"; }
unchosen() { ctl "$1" states | grep -q -F "$1 off"; }
digest() { md5sum "$(wallpaper)" 2>/dev/null | cut -d' ' -f1; }
ours() { case "$(wallpaper)" in "$data"/out/*) true ;; *) false ;; esac; }
saved() {
  python3 - "$data/state.json" "$1" <<'PY'
import json, sys
state = json.load(open(sys.argv[1]))
value = state
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
state = json.load(open(sys.argv[1]))
screens = state.get("screens", {}).values()
key, want = sys.argv[2], sys.argv[3]
def get(entry):
    value = entry
    for part in key.split("."):
        value = value.get(part) if isinstance(value, dict) else None
    return json.dumps(value)
sys.exit(0 if screens and all(get(entry) == want for entry in screens) else 1)
PY
}
names() { [ "$(sources)" = "$(realpath "$1")" ]; }
fresh_from() { names "$1" && ours && [ "$(digest)" != "$2" ]; }
status_text() { clean /usr/bin/scooch status 2>&1 | tr '\n' '|'; }
alive_on() { status_text | grep -q "^Scooch is on\.|" && [ -n "$(pgrep -x scooch)" ]; }
dialog_of() {
  local pid id
  pid=$(pgrep -x scooch-app | head -1)
  [ -n "$pid" ] || return 1
  for id in $(xdotool search --onlyvisible --pid "$pid" 2>/dev/null); do
    if [ "$(xdotool getwindowname "$id" 2>/dev/null)" != "Scooch" ]; then
      echo "$id"
      return 0
    fi
  done
  return 1
}
pick_file() {
  local file="$1" dialog="" title step
  ctl "Pick a file" press >/dev/null
  for step in $(seq 1 30); do
    dialog=$(dialog_of) && break
    sleep 0.5
  done
  if [ -z "$dialog" ]; then
    echo "no file window opened"
    return 1
  fi
  title=$(xdotool getwindowname "$dialog" 2>/dev/null)
  xdotool windowactivate --sync "$dialog" >/dev/null 2>&1
  sleep 1
  xdotool key --delay 120 ctrl+l
  sleep 0.5
  xdotool type --delay 15 "$file"
  sleep 0.5
  xdotool key Return
  for step in $(seq 1 30); do
    dialog_of >/dev/null || break
    sleep 0.5
  done
  if dialog_of >/dev/null; then
    echo "the file window \"$title\" stayed open after $file"
    return 1
  fi
  echo "the file window \"$title\" took $file and closed"
}
press_switch() { ctl "$1" press >/dev/null; sleep 1; }

clean env NO_AT_BRIDGE=0 scooch-app >"$out/app-controls.log" 2>&1 &
wait_for 40 has_window >/dev/null
sleep 4
line "- the row and switches read $(ctl 'probe canoe,probe sky,great wave,Slide,Clouds,Flip,Quick drop' states)"

line
line "#### A picture in the row and the way back"
before=$(digest)
line "- $(ctl 'probe canoe' press)"
wait_for 40 fresh_from "$pics/probe-canoe.jpg" "$before" >/dev/null
sleep 2
line "- the row reads $(ctl 'probe canoe,great wave' states), the saved state names $(sources), the wallpaper is $(wallpaper)"
expect "pressing probe canoe in the row marks it as chosen" chosen "probe canoe"
expect "the saved state names probe canoe" names "$pics/probe-canoe.jpg"
expect "the desktop shows a new output made from probe canoe" fresh_from "$pics/probe-canoe.jpg" "$before"
before=$(digest)
line "- $(ctl 'great wave' press)"
wait_for 40 fresh_from "$photo" "$before" >/dev/null
sleep 2
line "- the row reads $(ctl 'probe canoe,great wave' states), the saved state names $(sources), the wallpaper is $(wallpaper)"
way_back() { chosen "great wave" && unchosen "probe canoe"; }
expect "the way back marks great wave as chosen and probe canoe no more" way_back
expect "the way back saves great wave" names "$photo"
expect "the way back shows a new output made from great wave" fresh_from "$photo" "$before"

line
line "#### Pick a file"
before=$(digest)
picked=$(pick_file "$pics/probe-pick.jpg")
line "- $picked"
wait_for 40 fresh_from "$pics/probe-pick.jpg" "$before" >/dev/null
sleep 2
line "- the row reads $(ctl 'probe pick' states), the saved state names $(sources), the wallpaper is $(wallpaper)"
expect "Pick a file opened the system's file window, which took the photo and closed" grep -q "took .* and closed" <<<"$picked"
expect "the picked photo joins the row as chosen" chosen "probe pick"
expect "the saved state names the picked photo" names "$pics/probe-pick.jpg"
expect "the desktop shows a new output made from the picked photo" fresh_from "$pics/probe-pick.jpg" "$before"
before=$(digest)
shown_before=$(wallpaper)
picked=$(pick_file "$pics/probe-notes.jpg")
line "- $picked"
sleep 4
said=$(ctl - status)
line "- the window then $said, the copy file says \"$unreadable\", the wallpaper is $(wallpaper)"
expect "the file window also took a file that is no picture" grep -q "took .* and closed" <<<"$picked"
expect "a picked file that is no picture shows the window's sentence for it" grep -q -F "$unreadable" <<<"$said"
same_desktop() { [ "$(wallpaper)" = "$shown_before" ] && [ "$(digest)" = "$before" ] && names "$pics/probe-pick.jpg"; }
expect "a picked file that is no picture leaves the desktop and the saved picture as they were" same_desktop

line
line "#### Clouds on a sky photo, with the icon column full"
desk=$(xdg-user-dir DESKTOP 2>/dev/null || echo "$HOME/Desktop")
mkdir -p "$desk"
for n in $(seq -w 1 40); do : >"$desk/probe-icon-$n.txt"; done
sleep 6
before=$(digest)
line "- $(ctl 'probe sky' press)"
wait_for 40 fresh_from "$pics/probe-sky.jpg" "$before" >/dev/null
sleep 5
with_clouds=$(digest)
line "- clouds saved as $(saved options.scenery), scooch status says $(status_text)"
press_switch Clouds
clouds_off() { [ "$(saved options.scenery)" = '"off"' ] && [ "$(digest)" != "$with_clouds" ]; }
wait_for 40 clouds_off >/dev/null
sleep 2
without_clouds=$(digest)
line "- after one press clouds saved as $(saved options.scenery), the switch reads $(ctl Clouds states), scooch status says $(status_text)"
expect "one press of Clouds saves clouds off" [ "$(saved options.scenery)" = '"off"' ]
expect "the Clouds switch reads off" unchosen Clouds
expect "scooch status drops the scenery line" eval '! status_text | grep -q "Scenery  on"'
expect "the sky photo without clouds is a new output unlike the one with them" [ "$without_clouds" != "$with_clouds" ]
press_switch Clouds
clouds_on() { [ "$(saved options.scenery)" != '"off"' ] && [ "$(digest)" != "$without_clouds" ]; }
wait_for 40 clouds_on >/dev/null
sleep 2
line "- after the second press clouds saved as $(saved options.scenery), the switch reads $(ctl Clouds states), scooch status says $(status_text)"
expect "the second press of Clouds saves clouds on" [ "$(saved options.scenery)" != '"off"' ]
expect "the Clouds switch reads on again" chosen Clouds
expect "scooch status shows the scenery line again" eval 'status_text | grep -q "Scenery  on"'
expect "the sky photo with clouds again is a new output unlike the one without" [ "$(digest)" != "$without_clouds" ]

for switch in "Slide glide" "Quick drop predict"; do
  shown_switch=${switch% *}
  key=${switch##* }
  line
  line "#### $shown_switch"
  if ctl "$shown_switch" states | grep -q -F "$shown_switch missing"; then
    line "- $shown_switch is not shown on this desktop, so it is not pressed here"
    continue
  fi
  was=$(saved "$key")
  press_switch "$shown_switch"
  flipped() { [ "$(saved "$key")" != "$was" ]; }
  wait_for 10 flipped >/dev/null
  sleep 2
  line "- $shown_switch was saved as $was and after one press as $(saved "$key"), the switch reads $(ctl "$shown_switch" states), scooch status says $(status_text)"
  expect "one press of $shown_switch flips its saved value" flipped
  if [ "$was" = true ]; then expect "the $shown_switch switch reads off" unchosen "$shown_switch"; else expect "the $shown_switch switch reads on" chosen "$shown_switch"; fi
  expect "after $shown_switch scooch status still says on and the daemon is alive" alive_on
  press_switch "$shown_switch"
  back() { [ "$(saved "$key")" = "$was" ]; }
  wait_for 10 back >/dev/null
  sleep 2
  line "- after the second press $shown_switch is saved as $(saved "$key"), the switch reads $(ctl "$shown_switch" states), scooch status says $(status_text)"
  expect "the second press of $shown_switch brings its saved value back" back
  if [ "$was" = true ]; then expect "the $shown_switch switch reads on again" chosen "$shown_switch"; else expect "the $shown_switch switch reads off again" unchosen "$shown_switch"; fi
  expect "after the second press of $shown_switch scooch status still says on and the daemon is alive" alive_on
done

line
line "#### Flip with the icon column full"
clean /usr/bin/scooch use "$pics/probe-flip.png" >"$out/flip-use.txt" 2>&1
helps() { names "$pics/probe-flip.png" && ours && screens_say flip_helps true && status_text | grep -q "Run scooch flip"; }
wait_for 60 helps >/dev/null
sleep 2
before=$(digest)
line "- with $(ls "$desk" | grep -c '^probe-icon-') files on the Desktop and the subject on the left, flip_helps $(python3 -c 'import json,sys; print([e.get("flip_helps", False) for e in json.load(open(sys.argv[1]))["screens"].values()])' "$data/state.json"), mirrored $(saved options.mirror), scooch status says $(status_text)"
expect "before Flip the saved state says flipping would help and scooch status shows the flip hint" helps
press_switch Flip
mirrored() { [ "$(saved options.mirror)" = true ] && screens_say placement.mirrored true && [ "$(digest)" != "$before" ]; }
wait_for 60 mirrored >/dev/null
sleep 2
flipped_out=$(digest)
line "- after one press of Flip mirror is saved as $(saved options.mirror), the switch reads $(ctl Flip states), the placement is mirrored $(python3 -c 'import json,sys; print([(e.get("placement") or {}).get("mirrored") for e in json.load(open(sys.argv[1]))["screens"].values()])' "$data/state.json"), scooch status says $(status_text)"
expect "one press of Flip saves flip on" [ "$(saved options.mirror)" = true ]
expect "the Flip switch reads on" chosen Flip
expect "the daemon mirrored the picture into a new output" mirrored
expect "scooch status drops the flip hint" eval '! status_text | grep -q "Run scooch flip"'
press_switch Flip
unmirrored() { [ "$(saved options.mirror)" = false ] && screens_say placement.mirrored false && [ "$(digest)" != "$flipped_out" ]; }
wait_for 60 unmirrored >/dev/null
sleep 2
line "- after the second press mirror is saved as $(saved options.mirror), the switch reads $(ctl Flip states), scooch status says $(status_text)"
expect "the second press of Flip saves flip off" [ "$(saved options.mirror)" = false ]
expect "the Flip switch reads off again" unchosen Flip
expect "the daemon put the picture back unmirrored in a new output" unmirrored
rm -f "$desk"/probe-icon-*.txt
sleep 3
line "- the Desktop files of the Flip case are removed, $(ls "$desk" | grep -c '^probe-icon-') left"

line
line "#### A picture pressed while Scooch is off"
line "- $(ctl 'Turn off' press)"
wait_for 30 state_off >/dev/null
wait_for 30 sh -c '! pgrep -x scooch >/dev/null' >/dev/null
sleep 3
shown_off=$(wallpaper)
state_sum=$(md5sum <"$data/state.json")
line "- off, wallpaper $shown_off, login item $(login), daemons $(daemons)"
line "- $(ctl 'probe canoe' press)"
sleep 5
line "- the row reads $(ctl 'probe canoe,great wave' states), wallpaper $(wallpaper), login item $(login), daemons $(daemons), selection file $(cat "$appdata/selected.json" 2>/dev/null)"
expect "a picture pressed while off is marked as chosen" chosen "probe canoe"
expect "pressed while off the desktop keeps its wallpaper" [ "$(wallpaper)" = "$shown_off" ]
expect "pressed while off no login item is written" [ ! -f "$autostart" ]
expect "pressed while off no daemon runs" no_daemon
expect "pressed while off Scooch's saved state is untouched" [ "$(md5sum <"$data/state.json")" = "$state_sum" ]
close_app
clean env NO_AT_BRIDGE=0 scooch-app >>"$out/app-controls.log" 2>&1 &
wait_for 40 has_window >/dev/null
sleep 4
line "- after closing and opening the window the row reads $(ctl 'probe canoe,great wave' states)"
expect "the selection survives closing and opening the window" chosen "probe canoe"
expect "opening the window again with a selection turned nothing on" no_daemon
before=$(md5sum "$shown_off" 2>/dev/null | cut -d' ' -f1)
line "- $(ctl 'Turn on' press)"
wait_for 40 fresh_from "$pics/probe-canoe.jpg" "$before" >/dev/null
sleep 3
line "- after Turn on the saved state names $(sources), the wallpaper is $(wallpaper), login item $(login), daemons $(daemons)"
expect "Turn on after the press shows that picture" fresh_from "$pics/probe-canoe.jpg" "$before"
expect "Turn on after the press writes the login item" [ -f "$autostart" ]
close_app
line
