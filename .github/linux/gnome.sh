#!/usr/bin/env bash
. "$(dirname "$0")/common.sh"

export XDG_CURRENT_DESKTOP=GNOME
export XDG_SESSION_TYPE=wayland
export XDG_SESSION_DESKTOP=gnome

gsettings set org.gnome.shell disable-user-extensions false
gsettings set org.gnome.shell enabled-extensions "['ding@rastersoft.com']"
gsettings set org.gnome.desktop.interface enable-animations false

gnome-shell --headless --wayland --no-x11 --virtual-monitor 1920x1080 >"$out/shell.log" 2>&1 &

waited=0
until [ -S "$XDG_RUNTIME_DIR/wayland-0" ] || [ "$waited" -ge 60 ]; do
    sleep 1
    waited=$((waited + 1))
done
export WAYLAND_DISPLAY=wayland-0
echo "wayland socket after $waited s"

wait_for_name org.gnome.Shell 60
wait_for_name com.rastersoft.dingextension 90 || gnome-extensions info ding@rastersoft.com >>"$out/shell.log" 2>&1
run_probe 8
