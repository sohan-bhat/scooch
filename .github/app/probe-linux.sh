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
"$HOME/.local/bin/scooch" use "$photo" >"$out/old-use.txt" 2>&1
wait_for 30 rendered >/dev/null
line "- older command install at ~/.local/bin/scooch, as install.sh lays it out, turned Scooch on, login item $(login), daemons $(daemons)"

scooch-app >"$out/app.log" 2>&1 &
if took=$(wait_for 40 has_window); then line "- window showed after $took s"; else line "- no window after 40 s"; fi
if took=$(wait_for 40 state_on); then line "- Scooch is on after $took s"; else line "- Scooch is not on within 40 s"; fi
if took=$(wait_for 40 rendered); then line "- Scooch drew the desktop after $took s"; else line "- no render within 40 s"; fi
sleep 4
hash -r
line "- after the app opened, login item $(login), daemons $(daemons), a terminal finds $(command -v scooch)"
line "- the app says $(grep 'scooch-app' "$out/app.log" | tr '\n' ' ')"
shot on
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
line "- daemons running $(daemons)"

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
"$appimage" >"$out/appimage.log" 2>&1 &
if took=$(wait_for 40 has_window); then
  line "- it started on the runner as it comes, window after $took s"
else
  line "- it did not start as it comes, the log says $(head -c 400 "$out/appimage.log" | tr '\n' ' ')"
  pkill -f "$appimage" >/dev/null 2>&1
  sudo apt-get install -y libfuse2t64 >"$out/fuse.log" 2>&1
  "$appimage" >"$out/appimage.log" 2>&1 &
  if took=$(wait_for 40 has_window); then line "- with libfuse2t64 installed it started, window after $took s"; else line "- it still did not start with libfuse2t64, the log says $(head -c 400 "$out/appimage.log" | tr '\n' ' ')"; fi
fi
sleep 4
line "- the AppImage says $(grep 'scooch-app' "$out/appimage.log" | tr '\n' ' ')"
pid=$(pgrep -x scooch-app | head -1)
if [ -n "$pid" ]; then
  tr '\0' '\n' <"/proc/$pid/environ" >"$out/appimage-env.txt"
  line "- environment the AppImage sets $(grep -E '^(LD_|GIO|GTK|GDK|GSETTINGS|XDG_DATA_DIRS|APPDIR|PYTHON|PERL|QT_)' "$out/appimage-env.txt" | cut -d= -f1 | tr '\n' ' ')"
  xargs -0 -a "/proc/$pid/environ" sh -c 'exec env -i "$@" xfconf-query -c xfce4-desktop -l' sh >"$out/xfconf-in-appimage.txt" 2>&1
  line "- xfconf-query under that environment exit $?, $(head -c 300 "$out/xfconf-in-appimage.txt" | tr '\n' ' ')"
  appdir=$(grep '^APPDIR=' "$out/appimage-env.txt" | cut -d= -f2-)
  line "- the scooch inside is $(file -b "$appdir/usr/bin/scooch" | cut -c1-80), --version says $("$appdir/usr/bin/scooch" --version 2>&1; echo " exit $?")"
fi
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
exit 0
