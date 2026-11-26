import os
import statistics
import subprocess
import sys
import time

import gi

gi.require_version("Gio", "2.0")
gi.require_version("GdkPixbuf", "2.0")
from gi.repository import GdkPixbuf, Gio, GLib

out = sys.argv[1]
count = int(sys.argv[2])
folder = os.path.abspath(os.path.join(out, "replace"))
os.makedirs(folder, exist_ok=True)
swatches = [(214, 64, 40), (36, 84, 214)]
bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
owned = []
Gio.bus_own_name_on_connection(bus, "org.gnome.Screenshot", Gio.BusNameOwnerFlags.NONE, lambda *a: owned.append(True), None)
context = GLib.MainContext.default()
deadline = time.time() + 5
while not owned and time.time() < deadline:
    context.iteration(False)
    time.sleep(0.05)


def log(text):
    with open(os.path.join(out, "replace.txt"), "a") as file:
        file.write(text + "\n")
    print(text, flush=True)


def picture(color, path):
    pixbuf = GdkPixbuf.Pixbuf.new(GdkPixbuf.Colorspace.RGB, False, 8, 1920, 1080)
    pixbuf.fill((color[0] << 24) | (color[1] << 16) | (color[2] << 8) | 0xFF)
    pixbuf.savev(path, "jpeg", ["quality"], ["95"])
    with open(path, "rb") as file:
        return file.read()


def look():
    shot = os.path.join(folder, "shot.png")
    try:
        reply = bus.call_sync("org.gnome.Shell.Screenshot", "/org/gnome/Shell/Screenshot", "org.gnome.Shell.Screenshot", "Screenshot", GLib.Variant("(bbs)", (False, False, shot)), GLib.VariantType("(bs)"), Gio.DBusCallFlags.NONE, 10000, None)
    except GLib.Error as error:
        return "error " + error.message
    done, used = reply.unpack()
    if not done:
        return "refused"
    pixbuf = GdkPixbuf.Pixbuf.new_from_file(used)
    width, height, stride, channels = pixbuf.get_width(), pixbuf.get_height(), pixbuf.get_rowstride(), pixbuf.get_n_channels()
    pixels = pixbuf.get_pixels()
    sums, n = [0, 0, 0], 0
    for y in range(height // 4, 3 * height // 4, 16):
        for x in range(width // 4, 3 * width // 4, 16):
            at = y * stride + x * channels
            for c in range(3):
                sums[c] += pixels[at + c]
            n += 1
    mean = [value / max(n, 1) for value in sums]
    near = [sum((mean[c] - swatch[c]) ** 2 for c in range(3)) ** 0.5 for swatch in swatches]
    best = 0 if near[0] <= near[1] else 1
    return best if near[best] < 40 else 2


def gsettings(key, value):
    subprocess.run(["gsettings", "set", "org.gnome.desktop.background", key, value], check=False)


data = [picture(swatches[0], os.path.join(folder, "a.jpg")), picture(swatches[1], os.path.join(folder, "b.jpg"))]
fixed = os.path.join(folder, "picture.jpg")
with open(fixed, "wb") as file:
    file.write(data[0])
gsettings("picture-uri", "'file://%s'" % fixed)
gsettings("picture-uri-dark", "'file://%s'" % fixed)
gsettings("picture-options", "'zoom'")
start = time.time()
first = None
while time.time() - start < 20:
    if look() == 0:
        first = int((time.time() - start) * 1000)
        break
log("screenshot name owned %s, the picture set once showed after %s ms, sample look %s" % (bool(owned), first, look()))

for way in ("rename", "inplace", "setting"):
    shown, times, between = 0, [], 0
    for index in range(1, count + 1):
        target = index % 2
        began = time.time()
        if way == "rename":
            fresh = os.path.join(folder, ".next.jpg")
            with open(fresh, "wb") as file:
                file.write(data[target])
                file.flush()
                os.fsync(file.fileno())
            os.rename(fresh, fixed)
        elif way == "inplace":
            with open(fixed, "r+b") as file:
                file.truncate(0)
                file.write(data[target])
                file.flush()
                os.fsync(file.fileno())
        else:
            path = os.path.join(folder, "set-%d.jpg" % index)
            with open(path, "wb") as file:
                file.write(data[target])
            gsettings("picture-uri", "'file://%s'" % path)
            gsettings("picture-uri-dark", "'file://%s'" % path)
        seen = None
        while time.time() - began < 6:
            found = look()
            if found == target:
                seen = int((time.time() - began) * 1000)
                break
            if found == 2:
                between += 1
        if seen is not None:
            shown += 1
            times.append(seen)
        time.sleep(0.3)
    if way == "setting":
        with open(fixed, "wb") as file:
            file.write(data[0])
        gsettings("picture-uri", "'file://%s'" % fixed)
        gsettings("picture-uri-dark", "'file://%s'" % fixed)
    median = statistics.median(times) if times else None
    log("way %s: %d of %d showed the new picture, median %s ms, most %s ms, captures between the two pictures %d" % (way, shown, count, median, max(times) if times else None, between))
