#!/usr/bin/env bash
set -u
out="$1"
mkdir -p "$out"
left="${SCOOCH_SETS_LEFT:-2500}"
pace="${SCOOCH_PACE:-0.5}"
schema=org.cinnamon.desktop.background
dir="$(mktemp -d)"
python3 - "$dir" <<'PY'
import sys
import gi
gi.require_version("GdkPixbuf", "2.0")
from gi.repository import GdkPixbuf
for name, color in (("a.jpg", 0xD64028FF), ("b.jpg", 0x2454D6FF)):
    pixbuf = GdkPixbuf.Pixbuf.new(GdkPixbuf.Colorspace.RGB, False, 8, 1920, 1080)
    pixbuf.fill(color)
    pixbuf.savev(sys.argv[1] + "/" + name, "jpeg", ["quality"], ["95"])
PY
pictures=("$dir/a.jpg" "$dir/b.jpg")

shell_pid() {
    pgrep -u "$(id -u)" -x cinnamon | head -n 1
}

alive() {
    gdbus call --session --timeout 3 --dest org.Cinnamon --object-path /org/Cinnamon --method org.freedesktop.DBus.Properties.GetAll org.Cinnamon >/dev/null 2>&1
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

gdbus introspect --session --dest org.Cinnamon --object-path /org/Cinnamon >"$out/introspect.txt" 2>&1
start_alive=no
alive && start_alive=yes
echo "cinnamon $(shell_pid) $(cinnamon --version 2>/dev/null), answered at the start $start_alive, $left writes one every $pace s" >> "$out/writes.txt"
threads "$(shell_pid)" > "$out/threads-start.txt"
count=0
silent=0
started=$(date +%s)
while [ "$count" -lt "$left" ]; do
    count=$((count + 1))
    target="$dir/set-$count.jpg"
    cp "${pictures[$((count % 2))]}" "$target"
    gsettings set "$schema" picture-uri "'file://$target'"
    rm -f "$dir/set-$((count - 3)).jpg"
    if [ $((count % 500)) = 0 ]; then
        echo "rss $(awk '/^VmRSS/ {print $2}' "/proc/$(shell_pid)/status" 2>/dev/null) kB after write $count" >> "$out/writes.txt"
    fi
    sleep "$pace"
    if [ "$start_alive" = yes ] && ! alive && ! alive; then
        pid="$(shell_pid)"
        silent=1
        echo "silent after write $count, $(( $(date +%s) - started )) s in, cinnamon $pid" >> "$out/writes.txt"
        threads "$pid" > "$out/threads.txt"
        sleep 5
        threads "$pid" >> "$out/threads.txt"
        stacks "$pid" "$out/stacks.txt"
        break
    fi
done
echo "cinnamon log lines $(wc -l < "$out/cinnamon.log" 2>/dev/null)" >> "$out/writes.txt"
echo "writes $count silent $silent" >> "$out/writes.txt"
rm -rf "$dir"
exit 0
