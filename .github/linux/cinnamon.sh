#!/usr/bin/env bash
. "$(dirname "$0")/common.sh"

start_xvfb
export XDG_CURRENT_DESKTOP=X-Cinnamon
export XDG_SESSION_DESKTOP=cinnamon
export LIBGL_ALWAYS_SOFTWARE=1

gsettings set org.nemo.desktop desktop-layout 'true::false' || true
gsettings set org.nemo.desktop home-icon-visible true || true
gsettings set org.nemo.desktop trash-icon-visible true || true

cinnamon --replace >"$out/cinnamon.log" 2>&1 &
wait_for_name org.Cinnamon 90 || echo "Cinnamon did not start, so nemo-desktop runs in its fallback mode" >>"$out/cinnamon.log"
nemo-desktop >"$out/nemo.log" 2>&1 &
run_probe 15
