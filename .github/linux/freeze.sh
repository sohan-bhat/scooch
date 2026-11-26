#!/usr/bin/env bash
set -u
case "$1" in
    gnome) name=gnome-shell ;;
    kde-wayland) name=kwin_wayland ;;
    *) name=Xvfb ;;
esac
pid=""
for _ in $(seq 1 600); do
    pid="$(pgrep -n -x "$name" || true)"
    [ -n "$pid" ] && break
    sleep 1
done
[ -n "$pid" ] || exit 0
sleep "${2:-75}"
mkdir -p linuxprobe
if kill -STOP "$pid"; then
    echo "froze $name $pid at $(date -u +%T)" >> linuxprobe/freeze.txt
fi
