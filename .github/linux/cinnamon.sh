#!/usr/bin/env bash
. "$(dirname "$0")/common.sh"

start_xvfb
{
    xrandr --newmode "1920x1080_60.00" 173.00 1920 2048 2248 2576 1080 1083 1088 1120 -hsync +vsync
    xrandr --addmode screen "1920x1080_60.00"
    xrandr --output screen --mode "1920x1080_60.00"
    xrandr
} >"$out/xrandr.log" 2>&1 || echo "a mode with a refresh rate could not be set on Xvfb" >>"$out/readiness.txt"
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
nemo-desktop >"$out/nemo.log" 2>&1 &
ready "Nemo desktop window" 60 window_listed '"Desktop":'
run_probe 10
