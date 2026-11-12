set -u

probe="$1"
out="$2"
mkdir -p "$out"

runtime="$(mktemp -d)"
chmod 700 "$runtime"
export XDG_RUNTIME_DIR="$runtime"
export NO_AT_BRIDGE=1

xdg-user-dirs-update --force >/dev/null 2>&1 || true
desktop_dir="$(xdg-user-dir DESKTOP 2>/dev/null || echo "$HOME/Desktop")"
mkdir -p "$desktop_dir"

wait_for_name() {
    local name="$1" limit="$2" waited=0
    while [ "$waited" -lt "$limit" ]; do
        if dbus-send --session --print-reply --dest=org.freedesktop.DBus /org/freedesktop/DBus org.freedesktop.DBus.NameHasOwner "string:$name" 2>/dev/null | grep -q "boolean true"; then
            echo "$name appeared after $waited s"
            return 0
        fi
        sleep 1
        waited=$((waited + 1))
    done
    echo "$name did not appear within $limit s"
    return 1
}

start_xvfb() {
    Xvfb :99 -screen 0 1920x1080x24 -nolisten tcp >"$out/xvfb.log" 2>&1 &
    export DISPLAY=:99
    local waited=0
    until xdpyinfo >/dev/null 2>&1 || [ "$waited" -ge 30 ]; do
        sleep 1
        waited=$((waited + 1))
    done
    export XDG_SESSION_TYPE=x11
}

run_probe() {
    sleep "${1:-5}"
    "$probe" "$out"
    local code=$?
    pkill -P $$ >/dev/null 2>&1 || true
    return "$code"
}
