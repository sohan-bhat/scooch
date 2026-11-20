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
  logger -t scooch-remove "turning Scooch off for $user" 2>/dev/null
  set --
  for name in HOME USER LOGNAME DISPLAY WAYLAND_DISPLAY XDG_RUNTIME_DIR DBUS_SESSION_BUS_ADDRESS XDG_CURRENT_DESKTOP XDG_SESSION_DESKTOP XDG_SESSION_TYPE XDG_CONFIG_HOME XDG_DATA_HOME XDG_DATA_DIRS SCOOCH_HOME; do
    value=$(tr '\000' '\n' <"$proc/environ" 2>/dev/null | sed -n "s/^$name=//p" | head -1)
    [ -n "$value" ] && set -- "$@" "$name=$value"
  done
  timeout 60 runuser -u "$user" -- env -i PATH=/usr/local/bin:/usr/bin:/bin "$@" "$program" off --final 2>&1 | logger -t scooch-remove || true
done

exit 0
