#!/usr/bin/env bash
set -u

out="$1"
mkdir -p "$out"
report="$out/report.md"
data="${XDG_DATA_HOME:-$HOME/.local/share}/scooch"
autostart="${XDG_CONFIG_HOME:-$HOME/.config}/autostart/scooch.desktop"
photo="$PWD/assets/photos/great-wave.jpg"
export WEBKIT_DISABLE_COMPOSITING_MODE=1
export LIBGL_ALWAYS_SOFTWARE=1

line() { echo "$*" | tee -a "$report"; }
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

press_pill() {
  local id
  id=$(window)
  [ -n "$id" ] || return 1
  xdotool windowactivate --sync "$id" >/dev/null 2>&1
  xdotool key --window "$id" "$@"
}

: >"$report"
line "## App probe on $(. /etc/os-release && echo "$PRETTY_NAME") with ${XDG_CURRENT_DESKTOP:-no desktop}"
line
line "- installed packages $(dpkg -l | awk '$2 == "scooch" { print $2, $3 }')"
line "- programs $(ls -la /usr/bin/scooch /usr/bin/scooch-app 2>&1 | awk '{ print $NF, $5 }' | tr '\n' ' ')"
set_wallpaper "$photo"
line "- wallpaper before the app $(wallpaper)"

scooch-app >"$out/app.log" 2>&1 &
if took=$(wait_for 40 has_window); then line "- window showed after $took s"; else line "- no window after 40 s"; fi
if took=$(wait_for 40 state_on); then line "- first open turned Scooch on after $took s"; else line "- first open did not turn Scooch on within 40 s"; fi
if took=$(wait_for 40 rendered); then line "- Scooch drew the desktop after $took s"; else line "- no render within 40 s"; fi
sleep 4
shot on
line "- login item $(grep '^Exec=' "$autostart" 2>/dev/null)"
line "- wallpaper while on $(wallpaper)"

press_pill Tab Tab Tab Tab Return
if took=$(wait_for 30 state_off); then line "- the keyboard turned Scooch off after $took s"; else line "- still on after the key presses"; fi
sleep 3
shot off
line "- wallpaper after off $(wallpaper), back to the photo $([ "$(wallpaper)" = "$photo" ] && echo yes || echo no)"
line "- login item after off $([ -f "$autostart" ] && echo present || echo gone)"

press_pill Return
if took=$(wait_for 30 state_on); then line "- on again after $took s"; else line "- not on again within 30 s"; fi
wait_for 30 rendered >/dev/null
press_pill alt+F4
if wait_for 15 sh -c '! pgrep -x scooch-app >/dev/null' >/dev/null; then line "- closing the window ended the app process"; else line "- the app process outlived its window"; fi
line "- daemons running $(pgrep -fc '/usr/bin/scooch watch')"

sudo apt-get remove -y scooch >"$out/remove.log" 2>&1
line "- package removal exit $?"
sleep 3
line "- wallpaper after removal $(wallpaper), back to the photo $([ "$(wallpaper)" = "$photo" ] && echo yes || echo no)"
line "- login item after removal $([ -f "$autostart" ] && echo present || echo gone), daemons running $(pgrep -fc '/usr/bin/scooch watch')"
cp "$data/state.json" "$out/state.json" 2>/dev/null
exit 0
