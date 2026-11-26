#!/usr/bin/env bash
set -u
out="$1"
mkdir -p "$out"
way="${SCOOCH_MEMORY_WAY:-rename}"
count="${SCOOCH_MEMORY_COUNT:-300}"
pace="${SCOOCH_MEMORY_PACE:-10}"
rest="${SCOOCH_MEMORY_REST:-300}"
schema=org.gnome.desktop.background
dir="$(mktemp -d)"
pictures=(/usr/share/backgrounds/gnome/adwaita-l.jpg /usr/share/backgrounds/gnome/adwaita-d.jpg)

shell_pid() {
    pgrep -u "$(id -u)" -x gnome-shell | head -n 1
}

rss() {
    awk '/^VmRSS/ {print $2}' "/proc/$(shell_pid)/status" 2>/dev/null
}

alive() {
    dbus-send --session --print-reply --reply-timeout=3000 --dest=org.gnome.Shell /org/gnome/Shell org.freedesktop.DBus.Properties.Get string:org.gnome.Shell string:ShellVersion >/dev/null 2>&1 && echo yes || echo no
}

note() {
    echo "$(date -u +%T) $*" | tee -a "$out/memory.txt"
}

fixed="$dir/picture.jpg"
cp "${pictures[0]}" "$fixed"
gsettings set "$schema" picture-uri "'file://$fixed'"
gsettings set "$schema" picture-uri-dark "'file://$fixed'"
sleep 30
note "way $way, $count changes one every $pace s, then $rest s at rest, $(gnome-shell --version), pictures $(ls -l "${pictures[@]}" | awk '{print $5}' | tr '\n' ' ')"
note "rss $(rss) kB before change 1, alive $(alive)"
for i in $(seq 1 "$count"); do
    source="${pictures[$((i % 2))]}"
    case "$way" in
        rename)
            cp "$source" "$dir/.next.jpg"
            mv -f "$dir/.next.jpg" "$fixed"
            ;;
        *)
            target="$dir/set-$i.jpg"
            cp "$source" "$target"
            gsettings set "$schema" picture-uri "'file://$target'"
            gsettings set "$schema" picture-uri-dark "'file://$target'"
            rm -f "$dir/set-$((i - 3)).jpg"
            ;;
    esac
    if [ $((i % 25)) = 0 ]; then
        note "rss $(rss) kB after change $i, alive $(alive)"
    fi
    sleep "$pace"
done
note "rss $(rss) kB after the last change $count, alive $(alive)"
for minute in $(seq 1 $((rest / 60))); do
    sleep 60
    note "rss $(rss) kB after $minute min at rest, alive $(alive)"
done
note "ding $(gnome-extensions info ding@rastersoft.com 2>/dev/null | grep -i state | tr -s ' ' | tr '\n' ' ')"
rm -rf "$dir"
exit 0
