#!/usr/bin/env bash
. "$(dirname "$0")/common.sh"

start_xvfb
mkdir -p "$HOME/.config/nemo"
printf '[desktop-monitor-0]\nnemo-icon-view-auto-layout=false\n' >"$HOME/.config/nemo/desktop-metadata"
export XDG_CURRENT_DESKTOP=X-Cinnamon
export XDG_SESSION_DESKTOP=cinnamon
export LIBGL_ALWAYS_SOFTWARE=1

gsettings set org.nemo.desktop desktop-layout 'true::false' || true
gsettings set org.nemo.desktop home-icon-visible true || true
gsettings set org.nemo.desktop trash-icon-visible true || true

cinnamon --replace >"$out/cinnamon.log" 2>&1 &
ready "Cinnamon" 90 owned org.Cinnamon || echo "nemo-desktop runs in its fallback mode" >>"$out/readiness.txt"
python3 "$(dirname "$0")/session_manager.py" >"$out/session-manager.log" 2>&1 &
ready "session manager stand in" 30 owned org.gnome.SessionManager
background="$(command -v csd-background || ls /usr/libexec/csd-background /usr/lib/cinnamon-settings-daemon/csd-background 2>/dev/null | head -1)"
"$background" >"$out/csd-background.log" 2>&1 &
ready "Cinnamon background drawn on the root window" 60 root_pixmap_set
nemo-desktop >"$out/nemo.log" 2>&1 &
ready "Nemo desktop window" 60 window_listed '"Desktop":'
run_probe 10
