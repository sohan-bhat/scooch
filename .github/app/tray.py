import sys
import time

from gi.repository import Gio, GLib

WATCHER = "org.kde.StatusNotifierWatcher"


def bus():
    return Gio.bus_get_sync(Gio.BusType.SESSION, None)


def call(connection, name, path, interface, method, arguments=None, reply=None):
    return connection.call_sync(name, path, interface, method, arguments, GLib.VariantType.new(reply) if reply else None, Gio.DBusCallFlags.NONE, 5000, None).unpack()


def owned(connection, name):
    return call(connection, "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "NameHasOwner", GLib.Variant("(s)", (name,)), "(b)")[0]


def prop(connection, name, path, interface, key):
    return call(connection, name, path, "org.freedesktop.DBus.Properties", "Get", GLib.Variant("(ss)", (interface, key)), "(v)")[0]


def items(connection):
    if not owned(connection, WATCHER):
        return []
    found = []
    for entry in prop(connection, WATCHER, "/StatusNotifierWatcher", WATCHER, "RegisteredStatusNotifierItems"):
        slash = entry.find("/")
        name, path = (entry, "/StatusNotifierItem") if slash < 0 else (entry[:slash], entry[slash:])
        try:
            ident = prop(connection, name, path, "org.kde.StatusNotifierItem", "Id")
            menu = prop(connection, name, path, "org.kde.StatusNotifierItem", "Menu")
            pid = call(connection, "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "GetConnectionUnixProcessID", GLib.Variant("(s)", (name,)), "(u)")[0]
            found.append((name, path, ident, menu, pid))
        except GLib.Error:
            continue
    return found


def ours(connection, pid):
    for name, path, ident, menu, owner in items(connection):
        if pid and owner == int(pid):
            return name, path, ident, menu
    return None


def layout(connection, name, menu):
    try:
        call(connection, name, menu, "com.canonical.dbusmenu", "AboutToShow", GLib.Variant("(i)", (0,)), "(b)")
    except GLib.Error:
        pass
    _, root = call(connection, name, menu, "com.canonical.dbusmenu", "GetLayout", GLib.Variant("(iias)", (0, -1, [])), "(u(ia{sv}av))")
    told = []
    for child in root[2]:
        ident, props, _ = child
        if props.get("visible", True) is False:
            continue
        if props.get("type") == "separator":
            told.append((ident, "separator", "", True))
            continue
        label = props.get("label", "").replace("__", "\x00").replace("_", "").replace("\x00", "_")
        mark = ""
        if props.get("toggle-type") == "checkmark":
            mark = "checked" if props.get("toggle-state", 0) == 1 else "unchecked"
        told.append((ident, label, mark, props.get("enabled", True)))
    return told


def main():
    mode, pid = sys.argv[1], sys.argv[2] if len(sys.argv) > 2 else ""
    connection = bus()
    if mode == "watcher":
        print("yes" if owned(connection, WATCHER) else "no")
        return
    if mode == "items":
        print("; ".join("%s %s id %s menu %s pid %s" % entry for entry in items(connection)) or "none")
        return
    found = ours(connection, pid)
    if found is None:
        print("no item of process %s among %s" % (pid, "; ".join("%s id %s pid %s" % (entry[0], entry[2], entry[4]) for entry in items(connection)) or "none"))
        return
    name, path, ident, menu = found
    if mode == "menu":
        print("|".join(" ".join(part for part in (label, mark, "" if enabled else "dim") if part) for _, label, mark, enabled in layout(connection, name, menu)))
        return
    if mode == "press":
        wanted = sys.argv[3]
        for entry, label, _, _ in layout(connection, name, menu):
            if label == wanted:
                call(connection, name, menu, "com.canonical.dbusmenu", "Event", GLib.Variant("(isvu)", (entry, "clicked", GLib.Variant("i", 0), int(time.time()))))
                print("pressed %s through DBusMenu" % wanted)
                return
        print("no %s in the menu" % wanted)


main()
