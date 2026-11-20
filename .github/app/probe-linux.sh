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

allowed="HOME USER LOGNAME PATH LANG DISPLAY XDG_RUNTIME_DIR DBUS_SESSION_BUS_ADDRESS XDG_CURRENT_DESKTOP XDG_SESSION_DESKTOP XDG_SESSION_TYPE NO_AT_BRIDGE WEBKIT_DISABLE_COMPOSITING_MODE LIBGL_ALWAYS_SOFTWARE"
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
  xdotool windowfocus --sync "$id" >/dev/null 2>&1
  sleep 0.5
  xdotool key --delay 120 "$@"
}

: >"$report"
export PATH="$HOME/.local/bin:$PATH"
daemons() { for pid in $(pgrep -x scooch); do readlink "/proc/$pid/exe"; done | sort | uniq -c | tr -s ' ' | tr '\n' ';'; }
login() { grep '^Exec=' "$autostart" 2>/dev/null; }
line "## App probe on $(. /etc/os-release && echo "$PRETTY_NAME") with ${XDG_CURRENT_DESKTOP:-no desktop}"
line
line "- installed packages $(dpkg -l | awk '$2 == "scooch" { print $2, $3 }')"
line "- programs $(ls -la /usr/bin/scooch /usr/bin/scooch-app 2>&1 | awk '{ print $NF, $5 }' | tr '\n' ' ')"
set_wallpaper "$photo"
line "- wallpaper before the app $(wallpaper)"

mkdir -p "$HOME/.local/bin"
cp /usr/bin/scooch "$HOME/.local/bin/scooch"
clean "$HOME/.local/bin/scooch" use "$photo" >"$out/old-use.txt" 2>&1
wait_for 30 rendered >/dev/null
line "- older command install at ~/.local/bin/scooch, as install.sh lays it out, turned Scooch on, login item $(login), daemons $(daemons)"

clean scooch-app >"$out/app.log" 2>&1 &
if took=$(wait_for 40 has_window); then line "- window showed after $took s"; else line "- no window after 40 s"; fi
clean scooch-app >"$out/app-second.log" 2>&1 &
second=$!
sleep 6
line "- a second launch left $(pgrep -xc scooch-app) window process, the second one $(kill -0 "$second" 2>/dev/null && echo still runs || echo ended)"
if took=$(wait_for 40 state_on); then line "- Scooch is on after $took s"; else line "- Scooch is not on within 40 s"; fi
if took=$(wait_for 40 rendered); then line "- Scooch drew the desktop after $took s"; else line "- no render within 40 s"; fi
sleep 4
hash -r
line "- after the app opened, login item $(login), daemons $(daemons), a terminal finds $(command -v scooch)"
line "- the app says $(grep 'scooch-app' "$out/app.log" | tr '\n' ' ')"
shot on
pgrep -x xfsettingsd >/dev/null || { xfsettingsd >"$out/xfsettingsd.log" 2>&1 & sleep 2; }
look() { xfconf-query -c xsettings -p /Net/ThemeName -n -t string -s "$1" >/dev/null 2>&1; sleep 3; }
line "- the desktop look before the probe $(xfconf-query -c xsettings -p /Net/ThemeName 2>/dev/null || echo unset)"
look Adwaita
shot look-light-on
look Adwaita-dark
shot look-dark-on
pkill -x scooch
sleep 2
xdotool windowactivate --sync "$(window)" >/dev/null 2>&1
sleep 3
line "- killed the daemon so the window shows a problem, state on $(grep -c '"on": true' "$data/state.json")"
shot look-dark-problem
look Adwaita
shot look-light-problem
clean /usr/bin/scooch >"$out/restart.txt" 2>&1
wait_for 30 sh -c 'pgrep -x scooch >/dev/null' >/dev/null
xdotool windowactivate --sync "$(window)" >/dev/null 2>&1
sleep 3
line "- wallpaper while on $(wallpaper)"

press_pill Tab Tab Tab Tab Return
if took=$(wait_for 30 state_off); then line "- the keyboard turned Scooch off after $took s"; else line "- still on after the key presses"; fi
sleep 3
shot off
shot look-light-off
look Adwaita-dark
shot look-dark-off
look Adwaita
line "- wallpaper after off $(wallpaper), back to the photo $([ "$(wallpaper)" = "$photo" ] && echo yes || echo no)"
line "- login item after off $([ -f "$autostart" ] && echo present || echo gone)"

press_pill Return
if took=$(wait_for 30 state_on); then line "- on again after $took s"; else line "- not on again within 30 s"; fi
wait_for 30 rendered >/dev/null
press_pill alt+F4
if wait_for 15 sh -c '! pgrep -x scooch-app >/dev/null' >/dev/null; then line "- closing the window ended the app process"; else line "- the app process outlived its window"; fi
line "- daemons running $(daemons)"

line
line "### Large text"
clean env NO_AT_BRIDGE=0 scooch-app >"$out/app-text.log" 2>&1 &
wait_for 40 has_window >/dev/null
sleep 3
controls="Slide,Clouds,Flip,Quick drop,Turn off,Turn on,Start again,Pick a file"
for scale in 1.5 2.0; do
  gsettings set org.gnome.desktop.interface text-scaling-factor "$scale"
  for size in "640 480 smallest" "900 620 default"; do
    set -- $size
    xdotool windowsize "$(window)" "$1" "$2" >/dev/null 2>&1
    xdotool windowminimize "$(window)" >/dev/null 2>&1
    sleep 1
    xdotool windowactivate --sync "$(window)" >/dev/null 2>&1
    sleep 3
    shot "text-$scale-$3"
    line "- text at $scale, $3 size, read back $(gsettings get org.gnome.desktop.interface text-scaling-factor) $(python3 "$PWD/.github/app/controls.py" "$controls" 2>&1 | tail -1)"
  done
done
gsettings reset org.gnome.desktop.interface text-scaling-factor
press_pill alt+F4
wait_for 15 sh -c '! pgrep -x scooch-app >/dev/null' >/dev/null
line

before=$(pgrep -x scooch | head -1)
sed -i 's/"daemon": "[^"]*"/"daemon": "0.0.0"/' "$data/state.json"
line "- the saved state now says $(grep -o '"daemon": "[^"]*"' "$data/state.json"), as after an upgrade in place, daemon $before"
clean scooch-app >"$out/app-upgrade.log" 2>&1 &
wait_for 40 has_window >/dev/null
replaced() { now=$(pgrep -x scooch | head -1); [ -n "$now" ] && [ "$now" != "$before" ]; }
if took=$(wait_for 30 replaced); then line "- reopening the app replaced the daemon after $took s, now $(pgrep -x scooch | head -1), saved $(grep -o '"daemon": "[^"]*"' "$data/state.json")"; else line "- reopening the app left daemon $(pgrep -x scooch | tr '\n' ' '), saved $(grep -o '"daemon": "[^"]*"' "$data/state.json")"; fi
press_pill alt+F4
wait_for 15 sh -c '! pgrep -x scooch-app >/dev/null' >/dev/null

cp /usr/bin/scooch "$HOME/.stale-scooch"
sudo apt-get remove -y scooch >"$out/remove.log" 2>&1
line "- package removal exit $?"
line "- removal log $(journalctl -t scooch-remove --no-pager -o cat 2>/dev/null | tail -5 | tr '\n' ' ')"
sleep 3
line "- wallpaper after removal $(wallpaper), back to the photo $([ "$(wallpaper)" = "$photo" ] && echo yes || echo no)"
line "- login item after removal $([ -f "$autostart" ] && echo present || echo gone), daemons running $(daemons)"
cp "$data/state.json" "$out/state.json" 2>/dev/null

line
line "### AppImage"
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
pid=$(pgrep -x scooch-app | head -1)
if [ -n "$pid" ]; then
  tr '\0' '\n' <"/proc/$pid/environ" | cut -d= -f1 | sort >"$out/appimage-env.txt"
  line "- names in the app's environment beyond the allowed list and the AppImage's own $(grep -v -x -E "$(echo $allowed | tr ' ' '|')" "$out/appimage-env.txt" | grep -v -E '^(LD_|GIO|GTK|GDK|GSETTINGS|XDG_DATA_DIRS|APPDIR|APPIMAGE|ARGV0|OWD|PYTHON|PERL|QT_)' | tr '\n' ' ')"
  line "- environment the AppImage sets $(grep -E '^(LD_|GIO|GTK|GDK|GSETTINGS|XDG_DATA_DIRS|APPDIR|PYTHON|PERL|QT_)' "$out/appimage-env.txt" | tr '\n' ' ')"
  xargs -0 -a "/proc/$pid/environ" sh -c 'exec env -i "$@" xfconf-query -c xfce4-desktop -l' sh >"$out/xfconf-in-appimage.txt" 2>&1
  line "- xfconf-query under that environment exit $?, $(head -c 300 "$out/xfconf-in-appimage.txt" | tr '\n' ' ')"
  appdir=$(tr '\0' '\n' <"/proc/$pid/environ" | sed -n 's/^APPDIR=//p')
  line "- the scooch inside is $(file -b "$appdir/usr/bin/scooch" | cut -c1-80), --version says $("$appdir/usr/bin/scooch" --version 2>&1; echo " exit $?")"
fi
line "- opened with Scooch off, ~/.local/bin/scooch $([ -e "$HOME/.local/bin/scooch" ] && echo present || echo absent), ~/bin/scooch $(if [ -L "$HOME/bin/scooch" ]; then echo a link; else file -b "$HOME/bin/scooch" | cut -c1-20; fi), login item $(login)"
press_pill alt+F4
wait_for 15 sh -c '! pgrep -x scooch-app >/dev/null' >/dev/null
sleep 2
line "- after closing it, ~/.local/bin/scooch $([ -e "$HOME/.local/bin/scooch" ] && echo present || echo absent), files Scooch wrote outside its folders $(find "$HOME/.local" "$HOME/bin" "$HOME/.config" -newer "$appimage" \( -type f -o -type l \) -not -path "$data/*" -not -path "$HOME/.local/share/me.sohanbhat.scooch.window/*" -not -path "$HOME/.cache/*" -not -path "$HOME/.config/me.sohanbhat.scooch.window/*" -not -path "$HOME/.local/share/gvfs-metadata/*" -not -path "$HOME/.config/xfce4/*" -not -path "$HOME/work/*" -not -path "$HOME/Downloads/*" 2>/dev/null | head -5 | tr '\n' ' ')"
clean "$appimage" >>"$out/appimage.log" 2>&1 &
wait_for 40 has_window >/dev/null
sleep 3
shot appimage-off
press_pill Tab Tab Tab Tab Return
if took=$(wait_for 30 state_on); then line "- the keyboard turned Scooch on after $took s"; else line "- not on after the key presses"; fi
wait_for 30 rendered >/dev/null
sleep 3
shot appimage-on
hash -r
line "- ~/.local/bin/scooch is $(file -b "$HOME/.local/bin/scooch" 2>/dev/null | cut -c1-40) answering $("$HOME/.local/bin/scooch" --version 2>&1)"
line "- login item $(login), daemons $(daemons), a terminal finds $(command -v scooch)"
line "- origin in the saved state $(grep -o '"origin": "[^"]*"' "$data/state.json")"
line "- ~/bin/scooch is now $(if [ -L "$HOME/bin/scooch" ]; then echo "a link to $(readlink "$HOME/bin/scooch")"; else file -b "$HOME/bin/scooch" | cut -c1-30; fi)"
press_pill Return
if took=$(wait_for 30 state_off); then line "- off again after $took s"; else line "- not off within 30 s"; fi
sleep 3
line "- after off, the copy ~/.local/bin/scooch $([ -e "$HOME/.local/bin/scooch" ] && echo present || echo gone)"
press_pill Return
if took=$(wait_for 30 state_on); then line "- on again after $took s"; else line "- not on again within 30 s"; fi
press_pill alt+F4
if wait_for 15 sh -c '! pgrep -x scooch-app >/dev/null' >/dev/null; then line "- closing the window ended the app, left running $(pgrep -fa 'AppImage|scooch' | grep -v probe | cut -c1-120 | tr '\n' ';')"; else line "- the app outlived its window"; fi
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

line
line "### AppImage in the Trash while off"
cp "$(ls "$PWD"/built/*.AppImage | head -1)" "$HOME/Downloads/Scooch-look.AppImage"
look="$HOME/Downloads/Scooch-look.AppImage"
chmod +x "$look"
clean "$look" >"$out/appimage-look.log" 2>&1 &
wait_for 40 has_window >/dev/null
sleep 3
press_pill alt+F4
wait_for 15 sh -c '! pgrep -x scooch-app >/dev/null' >/dev/null
gio trash "$look" >>"$out/trash.log" 2>&1
sleep 15
line "- opened once with Scooch off, closed and put in the Trash, then ~/.local/bin/scooch $([ -e "$HOME/.local/bin/scooch" ] && echo present || echo absent), ~/bin/scooch $([ -e "$HOME/bin/scooch" ] || [ -L "$HOME/bin/scooch" ] && echo present || echo absent), login item $([ -f "$autostart" ] && echo present || echo absent), daemons $(daemons)"

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
press_pill Return
wait_for 30 state_off >/dev/null
press_pill alt+F4
wait_for 15 sh -c '! pgrep -x scooch-app >/dev/null' >/dev/null
gio trash "$other" >>"$out/trash.log" 2>&1
exit 0
