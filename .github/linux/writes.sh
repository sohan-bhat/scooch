#!/usr/bin/env bash
set -u

out="$1"
mkdir -p "$out"
variant="${SCOOCH_WRITES:-two}"
pace="${SCOOCH_PACE:-0.5}"
left="${SCOOCH_SETS_LEFT:-1000}"
schema=org.gnome.desktop.background
dir="$(mktemp -d)"
pictures=(/usr/share/backgrounds/gnome/adwaita-l.jpg /usr/share/backgrounds/gnome/adwaita-d.jpg)

alive() {
    dbus-send --session --print-reply --reply-timeout=3000 --dest=org.gnome.Shell /org/gnome/Shell org.freedesktop.DBus.Properties.Get string:org.gnome.Shell string:ShellVersion >/dev/null 2>&1
}

shell_pid() {
    pgrep -u "$(id -u)" -x gnome-shell | head -n 1
}

threads() {
    local pid="$1"
    for task in /proc/"$pid"/task/*; do
        printf '%s %s %s\n' "${task##*/}" "$(cat "$task/comm" 2>/dev/null)" "$(awk '{sub(/.*\) /, ""); print "state", $1, "utime", $12, "stime", $13}' "$task/stat" 2>/dev/null)"
    done
}

stacks() {
    local pid="$1" file="$2"
    cat > "$dir/stacks.py" <<'PY'
import gdb
for thread in gdb.selected_inferior().threads():
    if thread.num == 1 or thread.name == "dconf worker":
        thread.switch()
        print("== full stack of thread %d %s" % (thread.num, thread.name))
        gdb.execute("bt full 30")
PY
    sudo -n env DEBUGINFOD_URLS=https://debuginfod.ubuntu.com DEBUGINFOD_CACHE_PATH=/var/tmp/debuginfod DEBUGINFOD_TIMEOUT=600 \
        timeout -k 10 1500 gdb -batch -nx -iex 'set debuginfod enabled on' -iex 'set pagination off' -iex 'set print frame-arguments scalars' -p "$pid" \
        -ex 'info threads' -ex 'thread apply all bt 40' -ex "source $dir/stacks.py" > "$file" 2>&1
    sudo -n chmod a+r "$file" 2>/dev/null || true
}

dbus-monitor --session "type='signal',interface='ca.desrt.dconf.Writer',member='Notify'" > "$out/notify.txt" 2>&1 &
monitor=$!

count=0
silent=0
started=$(date +%s)
while [ "$count" -lt "$left" ]; do
    count=$((count + 1))
    target="$dir/set-$count.jpg"
    cp "${pictures[$((count % 2))]}" "$target"
    uri="file://$target"
    case "$variant" in
        none)
            ;;
        three)
            gsettings set "$schema" picture-uri "'$uri'"
            gsettings set "$schema" picture-uri-dark "'$uri'"
            gsettings set "$schema" picture-options "'zoom'"
            ;;
        one)
            printf "[/]\npicture-uri='%s'\npicture-uri-dark='%s'\n" "$uri" "$uri" | dconf load /org/gnome/desktop/background/
            ;;
        *)
            gsettings set "$schema" picture-uri "'$uri'"
            gsettings set "$schema" picture-uri-dark "'$uri'"
            ;;
    esac
    rm -f "$dir/set-$((count - 3)).jpg"
    if [ "$count" = 20 ]; then
        echo "notifications for the first 20 sets $(grep -c 'member=Notify' "$out/notify.txt")" >> "$out/writes.txt"
        kill "$monitor" 2>/dev/null || true
    fi
    sleep "$pace"
    if ! alive && ! alive; then
        pid="$(shell_pid)"
        silent=1
        echo "silent after set $count of this session, $(( $(date +%s) - started )) s in, gnome-shell $pid" >> "$out/writes.txt"
        threads "$pid" > "$out/threads.txt"
        sleep 5
        threads "$pid" >> "$out/threads.txt"
        stacks "$pid" "$out/stacks.txt"
        break
    fi
done
kill "$monitor" 2>/dev/null || true
echo "sets $count silent $silent variant $variant" >> "$out/writes.txt"
echo "$count $silent" > "$out/count.txt"
rm -rf "$dir"
exit 0
