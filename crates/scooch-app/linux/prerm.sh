#!/bin/sh

case "${1:-}" in
  upgrade | failed-upgrade | 1) exit 0 ;;
esac

program=/usr/bin/scooch
[ -x "$program" ] || exit 0

for proc in /proc/[0-9]*; do
  exe=$(readlink "$proc/exe" 2>/dev/null) || continue
  case "$exe" in
    "$program" | "$program (deleted)") ;;
    *) continue ;;
  esac
  tr '\000' '\n' <"$proc/cmdline" 2>/dev/null | grep -qx watch || continue
  uid=$(awk '/^Uid:/ { print $2 }' "$proc/status" 2>/dev/null)
  [ -n "$uid" ] || continue
  user=$(getent passwd "$uid" | cut -d: -f1)
  [ -n "$user" ] || continue
  timeout 60 runuser -u "$user" -- xargs -0 -a "$proc/environ" sh -c 'exec env -i "$@" '"$program"' off' sh >/dev/null 2>&1 || true
done

exit 0
