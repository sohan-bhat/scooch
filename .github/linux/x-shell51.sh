#!/usr/bin/env bash
set -u
step="$1"
out=linuxprobe
mkdir -p "$out"
if [ "$step" = install ]; then
    dnf install -y gnome-shell gnome-backgrounds dbus-daemon dbus-tools glib2 dconf python3-gobject gdk-pixbuf2 procps-ng xdg-user-dirs mesa-dri-drivers abattis-cantarell-fonts util-linux >"$out/packages.log" 2>&1 || { tail -30 "$out/packages.log"; exit 1; }
    gnome-shell --version >"$out/version.txt" 2>&1
    rpm -q gnome-shell mutter gjs glib2 glycin-libs >>"$out/version.txt" 2>&1
    cat "$out/version.txt"
    exit 0
fi
if [ "$step" = run ]; then
    rm -rf /run/systemd
    dbus-uuidgen --ensure 2>/dev/null || true
    mkdir -p /run/dbus
    dbus-daemon --system --fork >/dev/null 2>&1 || true
    id -u tester >/dev/null 2>&1 || useradd -m -u "${HOST_UID:-1001}" tester
    chown -R tester "$out"
    exec runuser -u tester -- env -i HOME=/home/tester USER=tester LOGNAME=tester PATH=/usr/local/bin:/usr/bin:/bin LANG=C.UTF-8 COUNT="${COUNT:-100}" bash .github/linux/x-shell51.sh session
fi
if [ "$step" = session ]; then
    extension="$HOME/.local/share/gnome-shell/extensions/capture@scooch"
    mkdir -p "$extension"
    cp .github/linux/x-capture/metadata.json .github/linux/x-capture/extension.js "$extension/"
    runtime="$(mktemp -d)"
    chmod 700 "$runtime"
    XDG_RUNTIME_DIR="$runtime" dbus-run-session -- bash .github/linux/x-shell51.sh inside >"$out/console.txt" 2>&1 || true
    exit 0
fi
export XDG_CURRENT_DESKTOP=GNOME
export XDG_SESSION_TYPE=wayland
gsettings set org.gnome.shell disable-user-extensions false
gsettings set org.gnome.shell enabled-extensions "['capture@scooch']"
gsettings set org.gnome.desktop.interface enable-animations false
gsettings set org.gnome.shell welcome-dialog-last-shown-version '9999'
gnome-shell --headless --wayland --no-x11 --virtual-monitor 1920x1080 >"$out/shell.log" 2>&1 &
owned() {
    dbus-send --session --print-reply --dest=org.freedesktop.DBus /org/freedesktop/DBus org.freedesktop.DBus.NameHasOwner "string:$1" 2>/dev/null | grep -q "boolean true"
}
for name in org.gnome.Shell org.scooch.Capture; do
    waited=0
    while [ "$waited" -lt 90 ] && ! owned "$name"; do
        sleep 1
        waited=$((waited + 1))
    done
    echo "$name owned $(owned "$name" && echo yes || echo no) after $waited s" | tee -a "$out/readiness.txt"
done
export WAYLAND_DISPLAY=wayland-0
gnome-extensions info capture@scooch >>"$out/readiness.txt" 2>&1
python3 .github/linux/x-capture51.py "$out" "$COUNT" >"$out/replace-console.txt" 2>&1
exit 0
