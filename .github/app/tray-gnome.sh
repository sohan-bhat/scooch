line
line "### No tray in this GNOME"
switches() { cat /proc/"$1"/task/*/status 2>/dev/null | awk '/ctxt_switches/ { s += $2 } END { print s + 0 }'; }
threads() { for task in /proc/"$1"/task/*; do printf '%s:%s ' "$(cat "$task/comm" 2>/dev/null)" "$(awk '/ctxt_switches/ { s += $2 } END { print s + 0 }' "$task/status" 2>/dev/null)"; done; }
state="${XDG_DATA_HOME:-$HOME/.local/share}/scooch/state.json"
mkdir -p "$(dirname "$state")"
saved=no
if [ -f "$state" ]; then cp "$state" "$out/state-before-tray.json"; saved=yes; fi
printf '{\n  "version": 1,\n  "on": true\n}\n' >"$state"
line "- the session bus holds org.kde.StatusNotifierWatcher $(owned org.kde.StatusNotifierWatcher && echo yes || echo no), the saved state now says on"
started=$(date +%s%N)
GDK_BACKEND=wayland scooch-app --background >"$out/app-background.log" 2>&1 &
waiting=$!
sleep 3
first=$(switches "$waiting")
early=$(threads "$waiting")
sleep 24
second=$(switches "$waiting")
late=$(threads "$waiting")
alive=$(kill -0 "$waiting" 2>/dev/null && echo yes || echo no)
wait "$waiting"
ended=$(( ($(date +%s%N) - started) / 1000000 ))
woke=$((second - first))
line "- a background start with Scooch on still ran after 27 s $alive, its threads as name and switches at 3 s $early"
line "- and at 27 s $late, so $woke switches in between"
line "- it ended after $ended ms and says $(tr '\n' ' ' <"$out/app-background.log")"
by_deadline() { [ "$ended" -ge 29000 ] && [ "$ended" -le 36000 ]; }
expect "with no tray a background start waits for one and ends by its 30 s deadline" by_deadline
expect "while it waits for the tray nothing of it wakes beyond its one bus wait" [ "$woke" -le 1 ]
expect "no process of the app stays after the deadline" eval '! pgrep -x scooch-app >/dev/null'
NO_AT_BRIDGE=0 GDK_BACKEND=wayland scooch-app >"$out/app-close.log" 2>&1 &
shown=$!
sleep 15
line "- opened the window with Scooch on and no tray, the app says $(grep 'scooch-app' "$out/app-close.log" | tr '\n' ' ')"
line "- $(python3 "$PWD/.github/app/controls.py" Close press 2>&1 | tail -1)"
gone=""
for step in $(seq 1 20); do
  if ! kill -0 "$shown" 2>/dev/null; then gone=$step; break; fi
  sleep 0.5
done
line "- after pressing Close the app $([ -n "$gone" ] && echo "ended within $((gone / 2)) s" || echo "still runs")"
expect "with no tray closing the window ends the app while Scooch is on" [ -n "$gone" ]
kill "$shown" >/dev/null 2>&1
if [ "$saved" = yes ]; then cp "$out/state-before-tray.json" "$state"; else rm -f "$state"; fi
