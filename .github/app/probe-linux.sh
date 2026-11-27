#!/usr/bin/env bash
set -u

out="$1"
mkdir -p "$out"
report="$out/report.md"
data="${XDG_DATA_HOME:-$HOME/.local/share}/scooch"
autostart="${XDG_CONFIG_HOME:-$HOME/.config}/autostart/scooch.desktop"
photo="$PWD/assets/photos/great-wave.jpg"
readonly out report data autostart photo
export WEBKIT_DISABLE_COMPOSITING_MODE=1
export LIBGL_ALWAYS_SOFTWARE=1

line() { echo "$*" | tee -a "$report"; }
. "$(dirname "$0")/expect.sh"
luma() { convert "$out/$1.png" -crop 6x6+4+4 -resize 1x1\! -format '%[fx:luminance]' info: 2>/dev/null || echo 0.5; }
light_shot() { awk -v v="$(luma "$1")" 'BEGIN { exit !(v > 0.55) }'; }
dark_shot() { awk -v v="$(luma "$1")" 'BEGIN { exit !(v < 0.35) }'; }
gone_app() { [ -z "$(pgrep -x scooch-app)" ]; }
entry_names() { grep -q -F "exec '$1' watch" "$autostart" 2>/dev/null || grep -qx "Exec=\"$1\" watch" "$autostart" 2>/dev/null; }
no_daemon() { [ -z "$(pgrep -x scooch)" ]; }
runtime_names="APPIMAGE APPDIR OWD ARGV0"
judged() {
  python3 - "$1" "$2" <<'PY'
import sys

mount = sys.argv[2].encode()
for item in open(sys.argv[1], "rb").read().split(b"\0"):
    if b"=" not in item:
        continue
    name, value = item.split(b"=", 1)
    print(name.decode(errors="replace"), "inside" if mount and mount in value else "outside")
PY
}
none_inside() { [ -n "$mount" ] && [ -s "$1" ] && ! grep -q ' inside$' "$1"; }
no_runtime_name() { [ -s "$1" ] && ! grep -q -E "^($(echo $runtime_names | tr ' ' '|')) " "$1"; }
session_backend() { [ -s "$1" ] && ! grep -q '^GDK_BACKEND ' "$1"; }

allowed="HOME USER LOGNAME PATH LANG DISPLAY XAUTHORITY XDG_RUNTIME_DIR DBUS_SESSION_BUS_ADDRESS XDG_CURRENT_DESKTOP XDG_SESSION_DESKTOP XDG_SESSION_TYPE NO_AT_BRIDGE WEBKIT_DISABLE_COMPOSITING_MODE LIBGL_ALWAYS_SOFTWARE"
readonly allowed
clean() {
  local pass=() name
  for name in $allowed; do
    if [ -n "${!name:-}" ]; then pass+=("$name=${!name}"); fi
  done
  env -i "${pass[@]}" "$@"
}
limit() { line "- limit: $*"; }

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

window() { xdotool search --name '^Scooch$' 2>/dev/null | head -1; }
has_window() { [ -n "$(window)" ]; }
app_snapshot() {
  local pid id holders="" windows=""
  for pid in $(pgrep -x scooch-app); do
    ls -l "/proc/$pid/fd" 2>/dev/null | grep -q 'icon\.lock' && holders="$holders $pid"
    for id in $(xdotool search --pid "$pid" 2>/dev/null); do
      windows="$windows$pid '$(xdotool getwindowname "$id" 2>/dev/null)' $(xwininfo -id "$id" 2>/dev/null | sed -n 's/.*Map State: //p') at $(xwininfo -id "$id" 2>/dev/null | awk '/-geometry/ { print $2 }'); "
    done
  done
  echo "app processes $(ps -o pid=,ppid=,lstart=,args= -C scooch-app | tr -s ' ' | tr '\n' ';') icon.lock open in${holders:- no process}, which only its holder keeps open, their windows ${windows:-none}"
}
state_on() { grep -q '"on": true' "$data/state.json" 2>/dev/null; }
state_off() { ! grep -q '"on": true' "$data/state.json" 2>/dev/null; }
rendered() { grep -q '"fingerprint": "' "$data/state.json" 2>/dev/null; }

wallpaper() {
  for property in $(xfconf-query -c xfce4-desktop -l 2>/dev/null | grep 'last-image$'); do
    xfconf-query -c xfce4-desktop -p "$property" 2>/dev/null
    return
  done
}

set_wallpaper() {
  for property in $(xfconf-query -c xfce4-desktop -l 2>/dev/null | grep 'last-image$'); do
    xfconf-query -c xfce4-desktop -p "$property" -s "$1"
  done
}

shot() {
  local id
  id=$(window)
  if [ -n "$id" ]; then
    import -window "$id" "$out/$1.png" 2>/dev/null && line "- window shot $1.png at $(xdotool getwindowgeometry "$id" | awk '/Geometry/ { print $2 }')"
  else
    import -window root "$out/$1-screen.png" 2>/dev/null
    line "- no window found for $1, took the whole screen instead"
  fi
}

close_app() {
  press_pill alt+F4
  wait_for 15 sh -c '! xdotool search --name "^Scooch$" >/dev/null 2>&1' >/dev/null
  wait_for 5 gone_app >/dev/null
  pkill -x scooch-app >/dev/null 2>&1
  wait_for 10 gone_app >/dev/null
}

press_pill() {
  local id
  id=$(window)
  [ -n "$id" ] || return 1
  xdotool windowactivate --sync "$id" >/dev/null 2>&1
  xdotool windowfocus --sync "$id" >/dev/null 2>&1
  sleep 0.5
  pressed_at=$(date +%s%3N)
  xdotool key --delay 120 "$@"
}

: >"$report"
export PATH="$HOME/.local/bin:$PATH"
daemons() { for pid in $(pgrep -x scooch); do readlink "/proc/$pid/exe"; done | sort | uniq -c | tr -s ' ' | tr '\n' ';'; }
login() { grep '^Exec=' "$autostart" 2>/dev/null; }
line "## App probe on $(. /etc/os-release && echo "$PRETTY_NAME") with ${XDG_CURRENT_DESKTOP:-no desktop}"
line
line "- installed packages $(dpkg -l | awk '$2 == "scooch" { print $2, $3 }')"
depends=$(dpkg-deb -f "$(ls "$PWD"/built/*.deb | head -1)" Depends)
line "- the deb depends on $depends, the library the tray loads is installed $(dpkg -l libayatana-appindicator3-1 2>/dev/null | awk '/^ii/ { print $3 }')"
expect "the deb names the app indicator library as a dependency" grep -q 'libayatana-appindicator3-1' <<<"$depends"
line "- programs $(ls -la /usr/bin/scooch /usr/bin/scooch-app 2>&1 | awk '{ print $NF, $5 }' | tr '\n' ' ')"
set_wallpaper "$photo"
line "- wallpaper before the app $(wallpaper)"

mkdir -p "$HOME/.local/bin"
cp /usr/bin/scooch "$HOME/.local/bin/scooch"
clean "$HOME/.local/bin/scooch" use "$photo" >"$out/old-use.txt" 2>&1
wait_for 30 rendered >/dev/null
line "- older command install at ~/.local/bin/scooch, as install.sh lays it out, turned Scooch on, login item $(login), daemons $(daemons)"
expect "the older command install turned Scooch on and drew" rendered

clean scooch-app >"$out/app.log" 2>&1 &
launch_said() { grep -q 'scooch-app ' "$out/app.log" 2>/dev/null; }
if took=$(wait_for 120 has_window); then
  line "- window showed after $took s, the app says $(tr '\n' ' ' <"$out/app.log")"
else
  line "- no window after 120 s, the app says $(tr '\n' ' ' <"$out/app.log"), $(app_snapshot)"
fi
expect "the first launch wrote its own first line, which the window check stands on" wait_for 30 launch_said
expect "the app opened its window within 120 s" has_window
clean scooch-app >"$out/app-second.log" 2>&1 &
second=$!
second_said() { grep -q 'scooch-app ' "$out/app-second.log" 2>/dev/null; }
second_gone() { ! ps -o stat= -p "$second" 2>/dev/null | grep -qv Z; }
ended=-1
if said=$(wait_for 60 second_said); then ended=$(wait_for 30 second_gone) || ended=-1; else said=-1; fi
left=$(pgrep -xc scooch-app)
line "- a second launch wrote its first line after $said s and was gone $ended s after that line, leaving $left window process, it says $(tr '\n' ' ' <"$out/app-second.log")"
[ "$said" -ge 0 ] && [ "$ended" -ge 0 ] && [ "$left" = 1 ] || line "- after the second launch, $(app_snapshot)"
expect "the second launch wrote its first line within 60 s, which the second launch check stands on" [ "$said" -ge 0 ]
expect "a second launch leaves one window process" eval '[ "$ended" -ge 0 ] && [ "$left" = 1 ]'
if took=$(wait_for 40 state_on); then line "- Scooch is on after $took s"; else line "- Scooch is not on within 40 s"; fi
if took=$(wait_for 40 rendered); then line "- Scooch drew the desktop after $took s"; else line "- no render within 40 s"; fi
expect "first open turned Scooch on and drew" rendered
sleep 4
hash -r
line "- after the app opened, login item $(login), daemons $(daemons), a terminal finds $(command -v scooch)"
expect "the login item points at /usr/bin/scooch" entry_names /usr/bin/scooch
expect "the deb removed the older command install from ~/.local/bin" [ ! -e "$HOME/.local/bin/scooch" ]
line "- the app says $(grep 'scooch-app' "$out/app.log" | tr '\n' ' ')"
shot on
pgrep -x xfsettingsd >/dev/null || { xfsettingsd >"$out/xfsettingsd.log" 2>&1 & sleep 2; }
look() { xfconf-query -c xsettings -p /Net/ThemeName -n -t string -s "$1" >/dev/null 2>&1; sleep 3; }
line "- the desktop look before the probe $(xfconf-query -c xsettings -p /Net/ThemeName 2>/dev/null || echo unset)"
look Adwaita
shot look-light-on
expect "the window is light in the light look while on" light_shot look-light-on
look Adwaita-dark
shot look-dark-on
expect "the window is dark in the dark look while on" dark_shot look-dark-on
pkill -x scooch
sleep 2
xdotool windowactivate --sync "$(window)" >/dev/null 2>&1
sleep 3
line "- killed the daemon so the window shows a problem, state on $(grep -c '"on": true' "$data/state.json")"
shot look-dark-problem
expect "the window is dark in the dark look with a problem" dark_shot look-dark-problem
look Adwaita
shot look-light-problem
expect "the window is light in the light look with a problem" light_shot look-light-problem
clean /usr/bin/scooch >"$out/restart.txt" 2>&1
wait_for 30 sh -c 'pgrep -x scooch >/dev/null' >/dev/null
xdotool windowactivate --sync "$(window)" >/dev/null 2>&1
sleep 3
line "- wallpaper while on $(wallpaper)"

press_pill Tab Tab Tab Tab Return
if took=$(wait_for 30 state_off); then line "- the keyboard turned Scooch off after $took s"; else line "- still on after the key presses"; fi
expect "the keyboard turned Scooch off" state_off
sleep 3
shot off
shot look-light-off
expect "the window is light in the light look while off" light_shot look-light-off
look Adwaita-dark
shot look-dark-off
expect "the window is dark in the dark look while off" dark_shot look-dark-off
look Adwaita
line "- wallpaper after off $(wallpaper), back to the photo $([ "$(wallpaper)" = "$photo" ] && echo yes || echo no)"
line "- login item after off $([ -f "$autostart" ] && echo present || echo gone)"
expect "off put the photo back" [ "$(wallpaper)" = "$photo" ]
expect "off removed the login item" [ ! -f "$autostart" ]

press_pill Return
if took=$(wait_for 30 state_on); then line "- on again after $took s"; else line "- not on again within 30 s"; fi
wait_for 30 rendered >/dev/null
before_close=$(pgrep -x scooch-app | head -1)
press_pill alt+F4
. "$PWD/.github/app/tray-linux.sh"

line
line "### Large text"
clean env NO_AT_BRIDGE=0 scooch-app >"$out/app-text.log" 2>&1 &
wait_for 40 has_window >/dev/null
sleep 3
controls="Slide,Clouds,Flip,Quick drop,Turn off,Turn on,Start again,Pick a file"
height_of() { sed -n 's/.*, status line \([0-9]*\) high,.*/\1/p' <<<"$1"; }
grew() { [ "${base:-0}" -gt 0 ] && [ "${grown:-0}" -gt "$((base * 5 / 4))" ]; }
base=""
for scale in 1.0 1.5 2.0; do
  gsettings set org.gnome.desktop.interface text-scaling-factor "$scale"
  for size in "640 480 smallest" "900 620 default"; do
    set -- $size
    xdotool windowsize "$(window)" "$1" "$2" >/dev/null 2>&1
    xdotool windowminimize "$(window)" >/dev/null 2>&1
    sleep 1
    xdotool windowactivate --sync "$(window)" >/dev/null 2>&1
    sleep 3
    said=$(python3 "$PWD/.github/app/controls.py" "$controls" 2>&1 | tail -1)
    line "- text at $scale, $3 size, read back $(gsettings get org.gnome.desktop.interface text-scaling-factor) $said"
    if [ "$scale" = 1.0 ]; then
      [ "$3" = default ] && base=$(height_of "$said")
      continue
    fi
    shot "text-$scale-$3"
    expect "every control is whole inside the window at text $scale, $3 size" grep -q "every control whole inside the window True, found [5-9]" <<<"$said"
    if [ "$3" = default ]; then
      grown=$(height_of "$said")
      line "- the status line stands ${base:-unread} high at text 1.0 and ${grown:-unread} at $scale"
      expect "the text scale $scale reached the window, the status line grew taller" grew
    fi
  done
done
gsettings reset org.gnome.desktop.interface text-scaling-factor
close_app
line

. "$PWD/.github/app/presses-linux.sh"

before=$(pgrep -x scooch | head -1)
sed -i 's/"daemon": "[^"]*"/"daemon": "0.0.0"/' "$data/state.json"
line "- the saved state now says $(grep -o '"daemon": "[^"]*"' "$data/state.json"), as after an upgrade in place, daemon $before"
clean scooch-app >"$out/app-upgrade.log" 2>&1 &
wait_for 40 has_window >/dev/null
replaced() { now=$(pgrep -x scooch | head -1); [ -n "$now" ] && [ "$now" != "$before" ]; }
if took=$(wait_for 30 replaced); then line "- reopening the app replaced the daemon after $took s, now $(pgrep -x scooch | head -1), saved $(grep -o '"daemon": "[^"]*"' "$data/state.json")"; else line "- reopening the app left daemon $(pgrep -x scooch | tr '\n' ' '), saved $(grep -o '"daemon": "[^"]*"' "$data/state.json")"; fi
expect "reopening the app replaced a daemon of an older version" replaced
close_app

others() {
  python3 - "$data/state.json" <<'PY'
import json, sys
try:
    state = json.load(open(sys.argv[1]))
except Exception:
    state = {}
print(" ".join(sorted(key for key in state.get("screens", {}) if key != "probe-absent")))
PY
}
put_back() { [ "$(wallpaper)" = "$photo" ] && [ -z "$(others)" ]; }
missed() { ! put_back; }
drawn() { [ -n "$(wallpaper)" ] && [ "$(wallpaper)" != "$photo" ]; }
cookie_daemon() {
  local pid
  pid=$(pgrep -x scooch | head -1)
  [ -n "$pid" ] && [ -n "${XAUTHORITY:-}" ] && tr '\0' '\n' <"/proc/$pid/environ" | grep -qxF "XAUTHORITY=$XAUTHORITY"
}
refused() { ! env -u XAUTHORITY xdpyinfo >/dev/null 2>&1; }
absent_on() {
  pkill -x scooch
  sleep 2
  mkdir -p "$data/out"
  cp "$photo" "$data/out/probe-absent-1.jpg"
  python3 - "$data/state.json" "$photo" "$data/out/probe-absent-1.jpg" <<'PY'
import json, sys
path, picture, output = sys.argv[1:4]
state = json.load(open(path))
state["on"] = True
state.setdefault("screens", {})["probe-absent"] = {"source": picture, "original": picture, "output": output}
json.dump(state, open(path, "w"), indent=2)
PY
  clean /usr/bin/scooch >"$out/absent-on-$1.txt" 2>&1
  wait_for 30 sh -c 'pgrep -x scooch >/dev/null' >/dev/null
  wait_for 30 drawn >/dev/null
  sleep 3
}
clear_absent() {
  python3 - "$data/state.json" <<'PY'
import json, sys
path = sys.argv[1]
state = json.load(open(path))
state.get("screens", {}).pop("probe-absent", None)
json.dump(state, open(path, "w"), indent=2)
PY
}

line "### Removal with the X cookie in a file of its own"
line "- the X server takes its cookie from a file that is not ~/.Xauthority $([ -n "${XAUTHORITY:-}" ] && [ "$XAUTHORITY" != "$HOME/.Xauthority" ] && echo yes || echo no), ~/.Xauthority holds a cookie for this display $(xauth -f "$HOME/.Xauthority" list "$DISPLAY" 2>/dev/null | grep -q . && echo yes || echo no)"
expect "the X server refuses a client without the cookie file" refused
absent_on deb
line "- the saved state holds probe-absent $(grep -c '"probe-absent"' "$data/state.json") and the connected screens $(others), daemons $(daemons), login item $(login)"
expect "the saved state holds a record for a screen nobody shows before the removal" grep -q '"probe-absent"' "$data/state.json"
expect "the saved state holds a connected screen before the removal" [ -n "$(others)" ]
expect "the daemon runs with XAUTHORITY at the cookie file" cookie_daemon
cp "$data/state.json" "$out/state-before-control.json"
control="$(mktemp -d)/prerm-without-xauthority.sh"
sed 's/ XAUTHORITY//' crates/scooch-app/linux/prerm.sh >"$control"
line "- control, prerm.sh with XAUTHORITY taken out of its list passes $(grep -o 'for name in [A-Z_ ]*' "$control" | cut -c13- | tr -s ' ')"
sudo sh "$control" remove >"$out/control.log" 2>&1
sleep 3
line "- after the control off, the photo is back $([ "$(wallpaper)" = "$photo" ] && echo yes || echo no), connected screens still recorded $(others), daemons $(daemons)"
expect "without XAUTHORITY the final off leaves Scooch's picture or parks the connected screen, so the check below catches a missing XAUTHORITY" missed
pkill -x scooch
sleep 1
cp "$out/state-before-control.json" "$data/state.json"
clean /usr/bin/scooch >"$out/control-on.txt" 2>&1
wait_for 30 sh -c 'pgrep -x scooch >/dev/null' >/dev/null
sleep 3
line "- Scooch on again after the control, daemons $(daemons), login item $(login)"
cp /usr/bin/scooch "$HOME/.stale-scooch"
wait_for 30 app_alive >/dev/null
icon_before=$(app_pid)
line "- the icon process before the package removal ${icon_before:-none}"
expect "the icon runs before the package removal" [ -n "$icon_before" ]
sudo apt-get remove -y scooch >"$out/remove.log" 2>&1
removed=$?
line "- package removal exit $removed"
expect "the package removal succeeded" [ "$removed" = 0 ]
line "- removal log $(journalctl -t scooch-remove --no-pager -o cat 2>/dev/null | tail -5 | tr '\n' ' ')"
sleep 3
line "- wallpaper after removal $(wallpaper), back to the photo $([ "$(wallpaper)" = "$photo" ] && echo yes || echo no), connected screens still recorded $(others)"
line "- login item after removal $([ -f "$autostart" ] && echo present || echo gone), daemons running $(daemons)"
expect "removal put the photo back and parked no screen that was connected" put_back
expect "removal left no login item" [ ! -f "$autostart" ]
expect "removal left no daemon" no_daemon
wait_for 15 sh -c "! kill -0 ${icon_before:-0} 2>/dev/null" >/dev/null
expect "the package removal ended the icon, which ended by itself when Scooch went off" eval '[ -n "$icon_before" ] && ! kill -0 "$icon_before" 2>/dev/null'
cp "$data/state.json" "$out/state.json" 2>/dev/null
expect "the final off kept the record of the screen nobody shows" grep -q '"probe-absent"' "$data/state.json"
clear_absent

line
line "### rpm removal with the X cookie in a file of its own"
rpm_file=$(ls "$PWD"/built/*.rpm 2>/dev/null | head -1)
rpm_name=$(rpm -qp --qf '%{NAME}' "$rpm_file" 2>/dev/null)
sudo rpm --initdb >/dev/null 2>&1 || true
sudo rpm -i --nodeps --force-debian "$rpm_file" >"$out/rpm-install.log" 2>&1 || sudo rpm -i --nodeps "$rpm_file" >>"$out/rpm-install.log" 2>&1
line "- rpm $(basename "$rpm_file"), package $rpm_name, installed $([ -x /usr/bin/scooch ] && echo yes || echo no), $(head -c 200 "$out/rpm-install.log" | tr '\n' ' ')"
expect "the rpm installed /usr/bin/scooch" [ -x /usr/bin/scooch ]
clean /usr/bin/scooch >"$out/rpm-on.txt" 2>&1
wait_for 30 drawn >/dev/null
line "- the rpm's scooch turned Scooch on, wallpaper $(wallpaper), daemons $(daemons)"
absent_on rpm
line "- the saved state holds probe-absent $(grep -c '"probe-absent"' "$data/state.json") and the connected screens $(others), daemons $(daemons), login item $(login)"
expect "the saved state holds a connected screen before the rpm removal" [ -n "$(others)" ]
expect "the daemon from the rpm runs with XAUTHORITY at the cookie file" cookie_daemon
wait_for 30 app_alive >/dev/null
icon_before=$(app_pid)
line "- the icon process before the rpm removal ${icon_before:-none}"
expect "the icon runs before the rpm removal" [ -n "$icon_before" ]
sudo rpm -e "${rpm_name:-scooch}" >"$out/rpm-remove.log" 2>&1
removed=$?
line "- rpm removal exit $removed"
expect "the rpm removal succeeded" [ "$removed" = 0 ]
sleep 3
line "- wallpaper after the rpm removal $(wallpaper), back to the photo $([ "$(wallpaper)" = "$photo" ] && echo yes || echo no), connected screens still recorded $(others), login item $([ -f "$autostart" ] && echo present || echo gone), daemons $(daemons)"
expect "the rpm removal put the photo back and parked no screen that was connected" put_back
expect "the rpm removal left no login item" [ ! -f "$autostart" ]
expect "the rpm removal left no daemon" no_daemon
wait_for 15 sh -c "! kill -0 ${icon_before:-0} 2>/dev/null" >/dev/null
expect "the rpm removal ended the icon, which ended by itself when Scooch went off" eval '[ -n "$icon_before" ] && ! kill -0 "$icon_before" 2>/dev/null'
expect "the rpm final off kept the record of the screen nobody shows" grep -q '"probe-absent"' "$data/state.json"
clear_absent
line "- cleared the parked record of probe-absent so the AppImage passes start with no screen that cannot be reached"

line
line "### AppImage"
sudo apt-get remove -y libayatana-appindicator3-1 >"$out/indicator-remove.log" 2>&1
line "- removed the system's libayatana-appindicator3-1 with exit $?, the system still lists $(ldconfig -p | grep -c 'libayatana-appindicator3.so') copies of the library"
mkdir -p "$HOME/bin"
mv "$HOME/.stale-scooch" "$HOME/bin/scooch"
line "- an older command install sits at ~/bin/scooch, $(file -b "$HOME/bin/scooch" | cut -c1-30), answering $("$HOME/bin/scooch" --version 2>&1)"
mkdir -p "$HOME/Downloads"
cp "$(ls "$PWD"/built/*.AppImage 2>/dev/null | head -1)" "$HOME/Downloads/"
appimage=$(ls "$HOME"/Downloads/*.AppImage | head -1)
line "- file ~/Downloads/$(basename "$appimage"), $(wc -c <"$appimage" | tr -d ' ') bytes, on the home file system $([ "$(stat -c %d "$appimage")" = "$(stat -c %d "$HOME")" ] && echo yes || echo no)"
chmod +x "$appimage"
clean "$appimage" >"$out/appimage.log" 2>&1 &
if took=$(wait_for 40 has_window); then
  line "- it started on the runner as it comes, window after $took s"
else
  line "- it did not start as it comes, the log says $(head -c 400 "$out/appimage.log" | tr '\n' ' ')"
  pkill -f "$appimage" >/dev/null 2>&1
  sudo apt-get install -y libfuse2t64 >"$out/fuse.log" 2>&1
  clean "$appimage" >"$out/appimage.log" 2>&1 &
  if took=$(wait_for 40 has_window); then line "- with libfuse2t64 installed it started, window after $took s"; else line "- it still did not start with libfuse2t64, the log says $(head -c 400 "$out/appimage.log" | tr '\n' ' ')"; fi
fi
sleep 4
line "- the AppImage says $(grep 'scooch-app' "$out/appimage.log" | tr '\n' ' ')"
mkdir -p "$out/carried"
(cd "$out/carried" && "$appimage" --appimage-extract 'usr/lib/libayatana*' >/dev/null 2>&1; "$appimage" --appimage-extract 'usr/lib/libdbusmenu*' >/dev/null 2>&1)
line "- the AppImage carries $(ls "$out/carried/squashfs-root/usr/lib" 2>/dev/null | tr '\n' ' ')"
expect "with the system library removed the AppImage made its tray from the library it carries" grep -q 'scooch-app tray made' "$out/appimage.log"
pid=$(pgrep -x scooch-app | head -1)
if [ -n "$pid" ]; then
  tr '\0' '\n' <"/proc/$pid/environ" | cut -d= -f1 | sort >"$out/appimage-env.txt"
  extra=$(grep -v -x -E "$(echo $allowed | tr ' ' '|')" "$out/appimage-env.txt" | grep -v -E '^(LD_|GIO|GTK|GDK|GSETTINGS|XDG_DATA_DIRS|APPDIR|APPIMAGE|ARGV0|OWD|PYTHON|PERL|QT_|GI_TYPELIB_PATH|GST_PLUGIN_|PWD$|SHLVL$|_$)')
  expect "the AppImage's environment was read" [ -s "$out/appimage-env.txt" ]
  expect "no name outside the allowed list reached the app" [ -z "$extra" ]
  line "- names in the app's environment beyond the allowed list and the AppImage's own $(grep -v -x -E "$(echo $allowed | tr ' ' '|')" "$out/appimage-env.txt" | grep -v -E '^(LD_|GIO|GTK|GDK|GSETTINGS|XDG_DATA_DIRS|APPDIR|APPIMAGE|ARGV0|OWD|PYTHON|PERL|QT_)' | tr '\n' ' ')"
  line "- environment the AppImage sets $(grep -E '^(LD_|GIO|GTK|GDK|GSETTINGS|XDG_DATA_DIRS|APPDIR|PYTHON|PERL|QT_)' "$out/appimage-env.txt" | tr '\n' ' ')"
  xargs -0 -a "/proc/$pid/environ" sh -c 'exec env -i "$@" xfconf-query -c xfce4-desktop -l' sh >"$out/xfconf-in-appimage.txt" 2>&1
  line "- xfconf-query under that environment exit $?, $(head -c 300 "$out/xfconf-in-appimage.txt" | tr '\n' ' ')"
  appdir=$(tr '\0' '\n' <"/proc/$pid/environ" | sed -n 's/^APPDIR=//p')
  line "- the scooch inside is $(file -b "$appdir/usr/bin/scooch" | cut -c1-80), --version says $("$appdir/usr/bin/scooch" --version 2>&1; echo " exit $?")"
fi
expect "the AppImage opened its window" has_window
line "- opened with Scooch off, ~/.local/bin/scooch $([ -e "$HOME/.local/bin/scooch" ] && echo present || echo absent), ~/bin/scooch $(if [ -L "$HOME/bin/scooch" ]; then echo a link; else file -b "$HOME/bin/scooch" | cut -c1-20; fi), login item $(login)"
close_app
sleep 2
expect "opening the AppImage with Scooch off wrote no ~/.local/bin/scooch" [ ! -e "$HOME/.local/bin/scooch" ]
expect "opening the AppImage with Scooch off left ~/bin/scooch a plain file" test -f "$HOME/bin/scooch" -a ! -L "$HOME/bin/scooch"
expect "opening the AppImage with Scooch off wrote no login item" [ ! -f "$autostart" ]
line "- after closing it, ~/.local/bin/scooch $([ -e "$HOME/.local/bin/scooch" ] && echo present || echo absent), files Scooch wrote outside its folders $(find "$HOME/.local" "$HOME/bin" "$HOME/.config" -newer "$appimage" \( -type f -o -type l \) -not -path "$data/*" -not -path "$HOME/.local/share/me.sohanbhat.scooch.window/*" -not -path "$HOME/.cache/*" -not -path "$HOME/.config/me.sohanbhat.scooch.window/*" -not -path "$HOME/.local/share/gvfs-metadata/*" -not -path "$HOME/.config/xfce4/*" -not -path "$HOME/work/*" -not -path "$HOME/Downloads/*" 2>/dev/null | head -5 | tr '\n' ' ')"

mkdir -p "$HOME/probe-bin"
cat >"$HOME/probe-bin/xdg-open" <<'OPEN'
#!/bin/sh
pwd -P >"$HOME/.probe-link-folder"
env -0 >"$HOME/.probe-link-env"
OPEN
chmod +x "$HOME/probe-bin/xdg-open"
rm -f "$HOME/.probe-link-env" "$HOME/.probe-link-folder"
clean env NO_AT_BRIDGE=0 XDG_CURRENT_DESKTOP=Nowhere PATH="$HOME/probe-bin:$PATH" "$appimage" >>"$out/appimage.log" 2>&1 &
wait_for 40 has_window >/dev/null
sleep 4
mount=$(tr '\0' '\n' <"/proc/$(pgrep -x scooch-app | head -1)/environ" 2>/dev/null | sed -n 's/^APPDIR=//p')
folder=$(readlink "/proc/$(pgrep -x scooch-app | head -1)/cwd" 2>/dev/null)
outside_mount() { [ -n "$mount" ] && [ -n "$folder" ] && case "$folder/" in "$mount"/*) false ;; *) true ;; esac; }
line "- the app itself works in a folder inside the mount $(outside_mount && echo no || echo yes), as WebKit finds its helper programs from there"
line "- on a desktop named Nowhere $(python3 "$PWD/.github/app/controls.py" "Ask for Nowhere" press 2>&1 | tail -1)"
wait_for 15 test -s "$HOME/.probe-link-env" >/dev/null
[ -s "$HOME/.probe-link-env" ] && judged "$HOME/.probe-link-env" "$mount" >"$out/link-env.txt"
folder=$(cat "$HOME/.probe-link-folder" 2>/dev/null)
rm -f "$HOME/.probe-link-env" "$HOME/.probe-link-folder"
line "- the link opener works in a folder inside the mount $(outside_mount && echo no || echo yes)"
expect "the link opened from the AppImage works in a folder outside the mount" outside_mount
line "- the link opener got $(wc -l <"$out/link-env.txt" 2>/dev/null || echo 0) names, inside the mount $(grep ' inside$' "$out/link-env.txt" 2>/dev/null | cut -d' ' -f1 | tr '\n' ' ')"
expect "the link opened from the AppImage got no value inside the mount" none_inside "$out/link-env.txt"
expect "the link opened from the AppImage got none of the runtime's names" no_runtime_name "$out/link-env.txt"
expect "the link opened from the AppImage got GDK_BACKEND as the session has it, unset" session_backend "$out/link-env.txt"
close_app
sleep 2
mkdir -p "$out/apprun"
(cd "$out/apprun" && "$appimage" --appimage-extract AppRun >/dev/null 2>&1; "$appimage" --appimage-extract 'apprun-hooks/*' >/dev/null 2>&1)
line "- AppRun and its hooks export $(cat "$out/apprun/squashfs-root/AppRun" "$out/apprun/squashfs-root/apprun-hooks/"* 2>/dev/null | grep -o -E '^[[:space:]]*export [A-Za-z_][A-Za-z0-9_]*' | awk '{ print $2 }' | sort -u | tr '\n' ' ')"

clean "$appimage" >>"$out/appimage.log" 2>&1 &
wait_for 40 has_window >/dev/null
sleep 3
shot appimage-off
press_pill Tab Tab Tab Tab Return
if took=$(wait_for 30 state_on); then line "- the keyboard turned Scooch on after $took s"; else line "- not on after the key presses"; fi
expect "the keyboard turned Scooch on from the AppImage" state_on
wait_for 30 rendered >/dev/null
sleep 3
shot appimage-on
hash -r
line "- ~/.local/bin/scooch is $(file -b "$HOME/.local/bin/scooch" 2>/dev/null | cut -c1-40) answering $("$HOME/.local/bin/scooch" --version 2>&1)"
line "- login item $(login), daemons $(daemons), a terminal finds $(command -v scooch)"
line "- origin in the saved state $(grep -o '"origin": "[^"]*"' "$data/state.json")"
mount=$(tr '\0' '\n' <"/proc/$(pgrep -x scooch-app | head -1)/environ" 2>/dev/null | sed -n 's/^APPDIR=//p')
watcher=$(pgrep -f '.local/bin/scooch watch' | head -1)
[ -n "$watcher" ] && [ -n "$mount" ] && judged "/proc/$watcher/environ" "$mount" >"$out/daemon-env.txt"
line "- the daemon the AppImage started has $(wc -l <"$out/daemon-env.txt" 2>/dev/null || echo 0) names, inside the mount $(grep ' inside$' "$out/daemon-env.txt" 2>/dev/null | cut -d' ' -f1 | tr '\n' ' ')"
expect "the daemon started from the AppImage has no value inside the mount" none_inside "$out/daemon-env.txt"
expect "the daemon started from the AppImage has none of the runtime's names" no_runtime_name "$out/daemon-env.txt"
expect "the daemon started from the AppImage has GDK_BACKEND as the session has it, unset" session_backend "$out/daemon-env.txt"
line "- ~/bin/scooch is now $(if [ -L "$HOME/bin/scooch" ]; then echo "a link to $(readlink "$HOME/bin/scooch")"; else file -b "$HOME/bin/scooch" | cut -c1-30; fi)"
expect "turning on copied the program to ~/.local/bin/scooch" "$HOME/.local/bin/scooch" --version
expect "the login item points at the copy" entry_names "$HOME/.local/bin/scooch"
expect "origin names the AppImage" grep -q "\"origin\": \"$appimage\"" "$data/state.json"
expect "the stale ~/bin/scooch became a link to the copy" [ "$(readlink "$HOME/bin/scooch")" = "$HOME/.local/bin/scooch" ]
press_pill Return
if took=$(wait_for 30 state_off); then line "- off again after $took s"; else line "- not off within 30 s"; fi
sleep 3
line "- after off, the copy ~/.local/bin/scooch $([ -e "$HOME/.local/bin/scooch" ] && echo present || echo gone)"
expect "turning off removed the copy" [ ! -e "$HOME/.local/bin/scooch" ]
press_pill Return
if took=$(wait_for 30 state_on); then line "- on again after $took s"; else line "- not on again within 30 s"; fi
press_pill alt+F4
wait_for 15 sh -c '! xdotool search --name "^Scooch$" >/dev/null 2>&1' >/dev/null
sleep 3
line "- closed the AppImage window while on, app processes $(pgrep -xc scooch-app), left running $(pgrep -fa 'AppImage|scooch' | grep -v probe | cut -c1-120 | tr '\n' ';')"
only_icon() { [ "$(pgrep -xc scooch-app)" = 1 ] && ! has_window; }
expect "closing the AppImage window while on leaves only its icon" only_icon
mounted() { awk '$2 ~ /\/\.mount_/ { print $2 }' /proc/mounts; }
first_icon=$(pgrep -x scooch-app | head -1)
line "- after the first close the icon process is $first_icon, the AppImage is mounted at $(mounted | tr '\n' ' ')"
for round in 2 3 4 5; do
  clean "$appimage" >>"$out/appimage.log" 2>&1 &
  wait_for 40 has_window >/dev/null
  sleep 2
  press_pill alt+F4
  wait_for 15 sh -c '! xdotool search --name "^Scooch$" >/dev/null 2>&1' >/dev/null
  sleep 3
done
wait_for 15 only_icon >/dev/null
sleep 2
fifth_icon=$(pgrep -x scooch-app | head -1)
line "- after five closes of the AppImage window the icon process is ${fifth_icon:-none}, app processes $(pgrep -xc scooch-app), the AppImage is mounted at $(mounted | tr '\n' ' '), the starts in between said $(grep -c 'shows the window' "$out/appimage.log") times that the icon's process shows the window"
expect "after five closes of the AppImage window only its icon is left" only_icon
expect "five closes of the AppImage window gave the icon to another process" eval '[ -n "$fifth_icon" ] && [ "$fifth_icon" != "$first_icon" ]'
expect "after five closes exactly one mount of the AppImage exists" [ "$(mounted | wc -l)" = 1 ]
for daemon in $(pgrep -f '.local/bin/scooch watch'); do
  line "- the daemon holds $(ls -l "/proc/$daemon/fd" 2>/dev/null | grep -c '.mount_') files inside the AppImage $(ls -l "/proc/$daemon/fd" 2>/dev/null | grep '.mount_' | awk '{ print $NF }' | head -3 | tr '\n' ' ')"
done
now() { date +%s.%N; }
since() { awk -v a="$1" -v b="$(now)" 'BEGIN { printf "%.1f", b - a }'; }
origin_is() { grep -q "\"origin\": \"$1\"" "$data/state.json" 2>/dev/null; }

line
line "### AppImage renamed"
mkdir -p "$HOME/Apps"
renamed="$HOME/Apps/Scooch.AppImage"
mv "$appimage" "$renamed"
started=$(now)
followed=""
for step in $(seq 1 60); do
  if origin_is "$renamed"; then followed=$(since "$started"); break; fi
  sleep 0.5
done
if [ -n "$followed" ]; then line "- renamed to ~/Apps/Scooch.AppImage, origin followed after $followed s"; else line "- renamed to ~/Apps/Scooch.AppImage, origin still $(grep -o '"origin": "[^"]*"' "$data/state.json") after 30 s"; fi
expect "origin followed the rename" [ -n "$followed" ]
line "- login item $(login), daemons $(daemons)"

line
line "### AppImage in the Trash"
line "- before, wallpaper $(wallpaper), login item $(login), copy $([ -e "$HOME/.local/bin/scooch" ] && echo present), link $(readlink "$HOME/bin/scooch"), daemons $(daemons)"
gio trash "$renamed" >"$out/trash.log" 2>&1
line "- gio trash exit $?, the file is now $(ls "$HOME/.local/share/Trash/files" 2>/dev/null | tr '\n' ' ')"
started=$(now)
back="" item="" copy="" link="" gone=""
for step in $(seq 1 80); do
  [ -z "$back" ] && [ "$(wallpaper)" = "$photo" ] && back=$(since "$started")
  [ -z "$item" ] && [ ! -e "$autostart" ] && item=$(since "$started")
  [ -z "$copy" ] && [ ! -e "$HOME/.local/bin/scooch" ] && copy=$(since "$started")
  [ -z "$link" ] && [ ! -L "$HOME/bin/scooch" ] && [ ! -e "$HOME/bin/scooch" ] && link=$(since "$started")
  [ -z "$gone" ] && ! pgrep -x scooch >/dev/null && gone=$(since "$started")
  if [ -n "$back" ] && [ -n "$item" ] && [ -n "$copy" ] && [ -n "$link" ] && [ -n "$gone" ]; then break; fi
  sleep 0.5
done
line "- wallpaper back after ${back:-never} s, login item gone after ${item:-never} s, copy gone after ${copy:-never} s, ~/bin link gone after ${link:-never} s, no daemon after ${gone:-never} s, all within 40 s"
line "- after, wallpaper $(wallpaper), state on $(grep -c '"on": true' "$data/state.json")"
expect "the Trash put the photo back" [ -n "$back" ]
expect "the Trash removed the login item" [ -n "$item" ]
expect "the Trash removed the copy" [ -n "$copy" ]
expect "the Trash removed the ~/bin link" [ -n "$link" ]
expect "the Trash ended the daemon" [ -n "$gone" ]
if took=$(wait_for 15 gone_app); then line "- the AppImage's icon ended $took s after the Trash"; else line "- the AppImage's icon still runs after the Trash"; fi
expect "the Trash ended the AppImage's icon" gone_app

line
line "### AppImage in the Trash while off"
cp "$(ls "$PWD"/built/*.AppImage | head -1)" "$HOME/Downloads/Scooch-look.AppImage"
look="$HOME/Downloads/Scooch-look.AppImage"
chmod +x "$look"
clean "$look" >"$out/appimage-look.log" 2>&1 &
wait_for 40 has_window >/dev/null
sleep 3
close_app
gio trash "$look" >>"$out/trash.log" 2>&1
sleep 15
line "- opened once with Scooch off, closed and put in the Trash, then ~/.local/bin/scooch $([ -e "$HOME/.local/bin/scooch" ] && echo present || echo absent), ~/bin/scooch $([ -e "$HOME/bin/scooch" ] || [ -L "$HOME/bin/scooch" ] && echo present || echo absent), login item $([ -f "$autostart" ] && echo present || echo absent), daemons $(daemons)"
expect "an AppImage only looked at leaves no copy after the Trash" [ ! -e "$HOME/.local/bin/scooch" ]
expect "an AppImage only looked at leaves no login item after the Trash" [ ! -f "$autostart" ]
expect "an AppImage only looked at leaves no daemon after the Trash" no_daemon

line
line "### Someone else's scooch in ~/.local/bin"
printf '#!/bin/sh\necho not the Scooch program\n' >"$HOME/.local/bin/scooch"
chmod +x "$HOME/.local/bin/scooch"
mine=$(sha256sum "$HOME/.local/bin/scooch" | cut -d' ' -f1)
cp "$(ls "$PWD"/built/*.AppImage | head -1)" "$HOME/Downloads/Scooch-other.AppImage"
other="$HOME/Downloads/Scooch-other.AppImage"
chmod +x "$other"
clean "$other" >"$out/appimage-other.log" 2>&1 &
wait_for 40 has_window >/dev/null
sleep 3
press_pill Tab Tab Tab Tab Return
if took=$(wait_for 30 state_on); then line "- turned on after $took s"; else line "- not on within 30 s"; fi
sleep 4
line "- ~/.local/bin/scooch kept its bytes $([ "$(sha256sum "$HOME/.local/bin/scooch" | cut -d' ' -f1)" = "$mine" ] && echo yes || echo no), it says $("$HOME/.local/bin/scooch" 2>&1)"
line "- the copy went to ~/.local/share/scooch/bin/scooch $([ -x "$data/bin/scooch" ] && echo yes || echo no), answering $("$data/bin/scooch" --version 2>&1), login item $(login), daemons $(daemons)"
expect "someone else's ~/.local/bin/scooch kept its bytes" [ "$(sha256sum "$HOME/.local/bin/scooch" | cut -d' ' -f1)" = "$mine" ]
expect "the copy went to Scooch's own folder" "$data/bin/scooch" --version
expect "the login item points at Scooch's own copy" entry_names "$data/bin/scooch"
press_pill Return
wait_for 30 state_off >/dev/null
close_app
gio trash "$other" >>"$out/trash.log" 2>&1
finish
