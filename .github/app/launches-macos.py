import os
import signal
import subprocess
import sys
import time

app, out, data, rounds = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4])
cap = 90000
folder = os.path.join(out, "launches")
os.makedirs(folder, exist_ok=True)
record = open(os.path.join(out, "launches.log"), "a")
windows_script = """
function run(argv) {
  ObjC.import("CoreGraphics");
  const wanted = argv.map(Number);
  const list = ObjC.deepUnwrap(ObjC.castRefToObject($.CGWindowListCopyWindowInfo($.kCGWindowListOptionAll, 0))) || [];
  const mine = list.filter((w) => wanted.includes(w.kCGWindowOwnerPID));
  return mine.map((w) => `${w.kCGWindowOwnerPID} number ${w.kCGWindowNumber} layer ${w.kCGWindowLayer} ${w.kCGWindowIsOnscreen ? "shown" : "hidden"} at ${w.kCGWindowBounds.X} ${w.kCGWindowBounds.Y} ${w.kCGWindowBounds.Width}x${w.kCGWindowBounds.Height}`).join("; ") || "none";
}
"""


def now():
    return time.time() * 1000


def run(command, limit=20):
    try:
        return subprocess.run(command, capture_output=True, text=True, timeout=limit).stdout.strip()
    except subprocess.TimeoutExpired:
        return "no answer within %d s" % limit


def apps():
    return {int(pid) for pid in run(["pgrep", "-f", "MacOS/scooch-app"]).split()}


def described(pids):
    if not pids:
        return "none"
    return " ".join(run(["ps", "-o", "pid=,ppid=,lstart=,args=", "-p", ",".join(str(pid) for pid in sorted(pids))]).split()) or "none"


def holders():
    return run(["lsof", "-t", os.path.join(data, "icon.lock")]).split()


def snapshot():
    pids = sorted(apps())
    windows = run(["osascript", "-l", "JavaScript", "-e", windows_script] + [str(pid) for pid in pids], 15)
    return "app processes %s, icon.lock open in %s, which only its holder keeps open, their windows %s" % (described(pids), " ".join(holders()) or "no process", windows)


def spread(values):
    values = sorted(values)
    if not values:
        return "none"
    return "least %d, median %d, 90th %d, most %d over %d" % (values[0], values[len(values) // 2], values[int(len(values) * 0.9)], values[-1], len(values))


tally = {"stayed": 0, "old": 0, "first gone": 0, "no process": 0, "slow open": 0}
opens, spawns, lives, lines = [], [], [], []
for number in range(1, rounds + 1):
    base = apps()
    owner = holders()
    path = os.path.join(folder, "second-%d.log" % number)
    started = now()
    opener = subprocess.Popen(["open", "-n", "--stderr", path, app], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    opened = None
    seen, gone, args, said = {}, {}, {}, []
    position = 0
    while True:
        moment = now()
        if opened is None and opener.poll() is not None:
            opened = moment
        current = apps()
        for pid in current - base:
            if pid not in seen:
                seen[pid] = moment
                args[pid] = described([pid])
        for pid in seen:
            if pid not in current and pid not in gone:
                gone[pid] = moment
        try:
            with open(path, "rb") as handle:
                handle.seek(position)
                chunk = handle.read()
            position += len(chunk)
            for row in chunk.decode(errors="replace").splitlines():
                if row.strip():
                    said.append((moment - started, row))
        except FileNotFoundError:
            pass
        if opened is not None and seen and all(pid in gone for pid in seen) and moment - max(gone.values()) > 300:
            break
        if opened is not None and not seen and moment - opened > 30000:
            break
        if moment - started > cap:
            break
        time.sleep(0.05)
    if opener.poll() is None:
        opener.kill()
    old_at = min(opened if opened is not None else float("inf"), started + 20000) + 6000
    alive_at_old = [pid for pid in seen if seen[pid] <= old_at and (pid not in gone or gone[pid] > old_at)]
    stayed = [pid for pid in seen if pid not in gone]
    first_left = base & apps()
    misses = []
    if stayed:
        misses.append("the second process stayed %d s" % (cap // 1000))
        tally["stayed"] += 1
    if not seen:
        misses.append("no second process was seen")
        tally["no process"] += 1
    if base and not first_left:
        misses.append("the first process is gone")
        tally["first gone"] += 1
    if alive_at_old:
        tally["old"] += 1
    if opened is None or opened - started > 20000:
        tally["slow open"] += 1
    open_ms = int(opened - started) if opened is not None else -1
    spawn_ms = int(min(seen.values()) - started) if seen else -1
    life_ms = int(max(gone[pid] - seen[pid] for pid in seen if pid in gone)) if gone else -1
    if open_ms >= 0:
        opens.append(open_ms)
    if spawn_ms >= 0:
        spawns.append(spawn_ms)
    if life_ms >= 0:
        lives.append(life_ms)
    first_line = int(said[0][0]) if said else -1
    if first_line >= 0:
        lines.append(first_line)
    verdict = "fine" if not misses else "MISS " + ", ".join(misses)
    print("- round %d %s, open -n returned after %d ms, the second process %s was seen at %d ms and lived %d ms, its first line came at %d ms, the old check at %d ms would have counted it %s, icon.lock held by %s, it says %s" % (
        number, verdict, open_ms, " ".join(str(pid) for pid in seen) or "none", spawn_ms, life_ms, first_line, int(old_at - started) if old_at != float("inf") else -1,
        "alive" if alive_at_old else "gone", " ".join(owner) or "no process", " | ".join("%d %s" % (int(at), row) for at, row in said) or "nothing"), flush=True)
    record.write("== round %d %s base %s seen %s gone %s args %s said %s\n" % (number, verdict, sorted(base), seen, gone, args, said))
    if misses:
        print("  - " + snapshot(), flush=True)
        for pid in stayed:
            try:
                os.kill(pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
        time.sleep(3)
        for pid in stayed:
            try:
                os.kill(pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
    time.sleep(1)
record.close()
print("- experiment, %d second launches, the second process stayed %d times, no second process %d times, the first process gone %d times, the old check at 6 s after open would have counted two %d times, open -n took over 20 s %d times" % (rounds, tally["stayed"], tally["no process"], tally["first gone"], tally["old"], tally["slow open"]), flush=True)
print("- in ms from the start of open -n, it returned %s, the second process showed %s, its first line came %s, and it lived %s" % (spread(opens), spread(spawns), spread(lines), spread(lives)), flush=True)
