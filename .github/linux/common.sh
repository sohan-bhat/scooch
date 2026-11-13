set -u

probe="$1"
out="$2"
mkdir -p "$out"
: >"$out/readiness.txt"

if [ -z "${XDG_RUNTIME_DIR:-}" ] || [ ! -d "$XDG_RUNTIME_DIR" ]; then
    XDG_RUNTIME_DIR="$(mktemp -d)"
    chmod 700 "$XDG_RUNTIME_DIR"
    export XDG_RUNTIME_DIR
fi
dbus-update-activation-environment XDG_RUNTIME_DIR >/dev/null 2>&1 || true
export NO_AT_BRIDGE=1

xdg-user-dirs-update --force >/dev/null 2>&1 || true
desktop_dir="$(xdg-user-dir DESKTOP 2>/dev/null || echo "$HOME/Desktop")"
mkdir -p "$desktop_dir"

owned() {
    dbus-send --session --print-reply --dest=org.freedesktop.DBus /org/freedesktop/DBus org.freedesktop.DBus.NameHasOwner "string:$1" 2>/dev/null | grep -q "boolean true"
}

window_listed() {
    xwininfo -root -tree 2>/dev/null | grep -q -- "$1"
}

root_pixmap_set() {
    xprop -root _XROOTPMAP_ID 2>/dev/null | grep -q "0x"
}

workarea_set() {
    xprop -root _NET_WORKAREA 2>/dev/null | grep -q "="
}

ready() {
    local name="$1" limit="$2" waited=0
    shift 2
    while [ "$waited" -lt "$limit" ]; do
        if "$@"; then
            echo "$name ready after $waited s" | tee -a "$out/readiness.txt"
            return 0
        fi
        sleep 1
        waited=$((waited + 1))
    done
    echo "$name not ready after $limit s, so later checks may measure nothing" | tee -a "$out/readiness.txt"
    return 1
}

split_monitors() {
    {
        xrandr --output screen --primary
        xrandr --setmonitor left 1920/508x1080/286+0+0 screen
        xrandr --setmonitor right 1920/508x1080/286+1920+0 none
        xrandr --listmonitors
    } >>"$out/xrandr.log" 2>&1
    echo "split into $(xrandr --listmonitors 2>/dev/null | tr '\n' ' ')" >>"$out/readiness.txt"
}

start_xvfb() {
    local size="${1:-1920x1080}"
    if [ "${SCOOCH_MONITORS:-1}" = "2" ]; then
        size="3840x1080"
    fi
    Xvfb :99 -screen 0 "${size}x24" -nolisten tcp >"$out/xvfb.log" 2>&1 &
    export DISPLAY=:99
    export XDG_SESSION_TYPE=x11
    dbus-update-activation-environment DISPLAY XDG_SESSION_TYPE >/dev/null 2>&1 || true
    ready "X display" 30 xdpyinfo >/dev/null 2>&1 || true
    if [ "${SCOOCH_MONITORS:-1}" = "2" ]; then
        split_monitors
    fi
}

run_probe() {
    sleep "${1:-5}"
    "$probe" "$out"
    local code=$?
    pkill -P $$ >/dev/null 2>&1 || true
    return "$code"
}
