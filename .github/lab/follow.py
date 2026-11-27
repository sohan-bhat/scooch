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
    seconds = float(sys.argv[1])
    bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    known, spans, changes = {}, [], []
    state, since, polls = None, 0.0, 0
    started = time.monotonic()
    while True:
        now = time.monotonic()
        if now - started >= seconds:
            break
        entries = listed(bus)
        polls += 1
        if entries is None:
            current = "no watcher"
        else:
            current = sorted(pid for pid, comm in (owner(bus, known, entry) for entry in entries) if comm == "scooch-app")
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
    print(f"the tray's items, read every 20 ms for {round(seconds)} s in {polls} reads, held no Scooch item for {none} ms and two for {two} ms, the watcher was away for {blind} ms, and changed at {json.dumps(changes)}")


main()
