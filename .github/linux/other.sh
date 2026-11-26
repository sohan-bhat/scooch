#!/usr/bin/env bash
set -u
step="$1"
out=linuxprobe
mkdir -p "$out"
if [ "$step" = install ]; then
    if command -v apt-get >/dev/null 2>&1; then
        export DEBIAN_FRONTEND=noninteractive
        apt-get update >"$out/packages.log" 2>&1
        apt-get install -y --no-install-recommends gnome-shell gnome-backgrounds dbus dbus-x11 libglib2.0-bin dconf-cli dconf-service gvfs python3-gi gir1.2-gdkpixbuf-2.0 procps xdg-user-dirs libgl1-mesa-dri fonts-cantarell >>"$out/packages.log" 2>&1 || { tail -30 "$out/packages.log"; exit 1; }
    else
        dnf install -y gnome-shell gnome-backgrounds dbus-daemon dbus-tools glib2 dconf python3-gobject gdk-pixbuf2 procps-ng xdg-user-dirs mesa-dri-drivers abattis-cantarell-fonts util-linux >"$out/packages.log" 2>&1 || { tail -30 "$out/packages.log"; exit 1; }
    fi
    gnome-shell --version >"$out/version.txt" 2>&1
    cat "$out/version.txt"
    exit 0
fi
if [ "$step" = run ]; then
    dbus-uuidgen --ensure 2>/dev/null || true
    mkdir -p /run/dbus
    dbus-daemon --system --fork >/dev/null 2>&1 || true
    id -u tester >/dev/null 2>&1 || useradd -m -u "${HOST_UID:-1001}" tester
    chown -R tester "$out"
    exec runuser -u tester -- env -i HOME=/home/tester USER=tester LOGNAME=tester PATH=/usr/local/bin:/usr/bin:/bin LANG=C.UTF-8 \
        SETS="${SETS:-3000}" SCOOCH_REPLACES="${SCOOCH_REPLACES:-100}" bash .github/linux/other.sh session
fi
total=0
silent=0
sessions=0
empty=0
while [ "$total" -lt "$SETS" ] && [ "$empty" -lt 3 ]; do
    sessions=$((sessions + 1))
    run="$out/session-$sessions"
    mkdir -p "$run"
    runtime="$(mktemp -d)"
    chmod 700 "$runtime"
    first=""
    if [ "$sessions" = 1 ]; then first=1; fi
    XDG_RUNTIME_DIR="$runtime" SCOOCH_NO_DING=1 SCOOCH_WRITES=two SCOOCH_REPLACE_FIRST="$first" SCOOCH_SETS_LEFT=$((SETS - total)) \
        dbus-run-session -- bash .github/linux/gnome.sh "$PWD/.github/linux/other-probe.sh" "$run" >"$run/console.txt" 2>&1 || true
    done_here=0
    gone=0
    if [ -f "$run/count.txt" ]; then read -r done_here gone < "$run/count.txt"; fi
    if [ "$done_here" = 0 ]; then empty=$((empty + 1)); fi
    total=$((total + done_here))
    silent=$((silent + gone))
    echo "session $sessions sets $done_here silent $gone total $total" | tee -a "$out/sessions.txt"
    for pid in $(pgrep -u "$(id -u)"); do
        if tr '\0' '\n' < "/proc/$pid/environ" 2>/dev/null | grep -qx "XDG_RUNTIME_DIR=$runtime"; then
            kill -9 "$pid" 2>/dev/null || true
        fi
    done
    rm -rf "$runtime" 2>/dev/null || true
done
echo "version $(cat "$out/version.txt" 2>/dev/null) sets $total silent $silent sessions $sessions" | tee -a "$out/sessions.txt"
