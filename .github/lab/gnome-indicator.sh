#!/usr/bin/env bash
. "$(dirname "$0")/../linux/common.sh"

export XDG_CURRENT_DESKTOP=GNOME
export XDG_SESSION_TYPE=wayland
export XDG_SESSION_DESKTOP=gnome
dbus-update-activation-environment XDG_CURRENT_DESKTOP XDG_SESSION_TYPE >/dev/null 2>&1 || true

gsettings set org.gnome.shell disable-user-extensions false
gsettings set org.gnome.shell enabled-extensions "['ubuntu-appindicators@ubuntu.com']"
gsettings set org.gnome.desktop.interface enable-animations false
gnome-shell --headless --wayland --no-x11 --virtual-monitor 1920x1080 >"$out/shell.log" 2>&1 &

wayland_socket() {
    [ -S "$XDG_RUNTIME_DIR/wayland-0" ]
}

ready "Wayland socket" 60 wayland_socket
export WAYLAND_DISPLAY=wayland-0
ready "GNOME Shell" 60 owned org.gnome.Shell
ready "tray watcher of the AppIndicator extension" 60 owned org.kde.StatusNotifierWatcher || gnome-extensions info ubuntu-appindicators@ubuntu.com >>"$out/readiness.txt" 2>&1
run_probe 8
