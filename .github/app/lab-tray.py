import json
import sys
import time

from gi.repository import Gio, GLib

WATCHER = "org.kde.StatusNotifierWatcher"
STEP = 0.02


def listed(bus):
    try:
        answer = bus.call_sync(WATCHER, "/StatusNotifierWatcher", "org.freedesktop.DBus.Properties", "Get", GLib.Variant("(ss)", (WATCHER, "RegisteredStatusNotifierItems")), GLib.VariantType.new("(v)"), Gio.DBusCallFlags.NONE, 1000, None)
        return list(answer.unpack()[0])
    except GLib.Error:
        return None


def owner(bus, known, entry):
    slash = entry.find("/")
    name = entry if slash < 0 else entry[:slash]
    if name not in known:
        try:
            pid = bus.call_sync("org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "GetConnectionUnixProcessID", GLib.Variant("(s)", (name,)), GLib.VariantType.new("(u)"), Gio.DBusCallFlags.NONE, 1000, None).unpack()[0]
            with open(f"/proc/{pid}/comm") as handle:
                known[name] = (pid, handle.read().strip())
        except (GLib.Error, OSError):
            return (0, "")
    return known[name]


def main():
    seconds, path = float(sys.argv[1]), sys.argv[2]
    bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    known, spans, changes = {}, [], []
    state, since, polls, slowest = None, 0.0, 0, 0.0
    started = time.monotonic()
    while True:
        now = time.monotonic()
        if now - started >= seconds:
            break
        entries = listed(bus)
        took = time.monotonic() - now
        slowest = max(slowest, took)
        polls += 1
        if entries is None:
            current = "no watcher"
        else:
            owners = [owner(bus, known, entry) for entry in entries]
            current = sorted(pid for pid, comm in owners if comm == "scooch-app")
        if current != state:
            if state is not None:
                spans.append((state, since, now))
            changes.append((round((now - started) * 1000), current))
            state, since = current, now
        rest = STEP - (time.monotonic() - now)
        if rest > 0:
            time.sleep(rest)
    spans.append((state, since, time.monotonic()))
    none = round(sum((end - start) * 1000 for held, start, end in spans if held == []))
    two = round(sum((end - start) * 1000 for held, start, end in spans if isinstance(held, list) and len(held) >= 2))
    blind = round(sum((end - start) * 1000 for held, start, end in spans if held == "no watcher"))
    first, last = changes[0][1], changes[-1][1]
    moved = isinstance(first, list) and isinstance(last, list) and first != last
    steps = ", ".join(f"{at} ms {'no watcher' if held == 'no watcher' else 'items of ' + (' and '.join(str(pid) for pid in held) or 'no process')}" for at, held in changes)
    print(f"- the tray's registered items, read every {round(STEP * 1000)} ms for {round(seconds)} s in {polls} reads with the slowest at {round(slowest * 1000)} ms, held no Scooch item for {none} ms and two for {two} ms, the watcher was away for {blind} ms, the icon went to another process {'yes' if moved else 'no'}, and changed at {steps}")
    with open(path, "w") as handle:
        json.dump({"seconds": seconds, "polls": polls, "slowest_ms": round(slowest * 1000, 1), "none_ms": none, "two_ms": two, "blind_ms": blind, "moved": moved, "changes": changes}, handle)


main()
