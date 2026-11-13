#!/usr/bin/env bash
. "$(dirname "$0")/common.sh"

export XDG_CURRENT_DESKTOP=GNOME
export XDG_SESSION_TYPE=wayland
export XDG_SESSION_DESKTOP=gnome
dbus-update-activation-environment XDG_CURRENT_DESKTOP XDG_SESSION_TYPE >/dev/null 2>&1 || true

gsettings set org.gnome.shell disable-user-extensions false
gsettings set org.gnome.shell enabled-extensions "['ding@rastersoft.com']"
gsettings set org.gnome.desktop.interface enable-animations false

monitors=(--virtual-monitor 1920x1080)
if [ "${SCOOCH_MONITORS:-1}" = "2" ]; then
    monitors+=(--virtual-monitor 1920x1080)
fi
gnome-shell --headless --wayland --no-x11 "${monitors[@]}" >"$out/shell.log" 2>&1 &

wayland_socket() {
    [ -S "$XDG_RUNTIME_DIR/wayland-0" ]
}

ready "Wayland socket" 60 wayland_socket
export WAYLAND_DISPLAY=wayland-0
ready "GNOME Shell" 60 owned org.gnome.Shell
ready "Desktop Icons NG" 90 owned com.rastersoft.dingextension || gnome-extensions info ding@rastersoft.com >>"$out/shell.log" 2>&1
run_probe 8
