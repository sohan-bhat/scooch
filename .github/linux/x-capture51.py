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


def log(text):
    with open(os.path.join(out, "replace.txt"), "a") as file:
        file.write(text + "\n")
    print(text, flush=True)


def picture(color, path):
    pixbuf = GdkPixbuf.Pixbuf.new(GdkPixbuf.Colorspace.RGB, False, 8, 1920, 1080)
    pixbuf.fill((color[0] << 24) | (color[1] << 16) | (color[2] << 8) | 0xFF)
    if path.endswith(".png"):
        pixbuf.savev(path, "png", [], [])
    else:
        pixbuf.savev(path, "jpeg", ["quality"], ["95"])
    with open(path, "rb") as file:
        return file.read()


def look():
    shot = os.path.join(folder, "shot.png")
    try:
        reply = bus.call_sync("org.scooch.Capture", "/org/scooch/Capture", "org.scooch.Capture", "Capture", GLib.Variant("(s)", (shot,)), GLib.VariantType("(b)"), Gio.DBusCallFlags.NONE, 10000, None)
    except GLib.Error as error:
        return "error " + error.message
    (done,) = reply.unpack()
    if not done:
        return "refused"
    pixbuf = GdkPixbuf.Pixbuf.new_from_file(shot)
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


data = [picture(color, os.path.join(folder, "swatch-%d.%s" % (index, "png" if index else "jpg"))) for index, color in enumerate(swatches)]
log("the shown file is picture.jpg, swatch 0 is a JPEG starting %s and swatch 1 a PNG starting %s" % (data[0][:4].hex(), data[1][:4].hex()))
fixed = os.path.join(folder, "picture.jpg")
with open(fixed, "wb") as file:
    file.write(data[0])
for key in ("picture-uri", "picture-uri-dark"):
    subprocess.run(["gsettings", "set", "org.gnome.desktop.background", key, "'file://%s'" % fixed])
subprocess.run(["gsettings", "set", "org.gnome.desktop.background", "picture-options", "'zoom'"])
started = time.time()
first = None
while time.time() - started < 30:
    seen = look()
    if seen == 0:
        first = int((time.time() - started) * 1000)
        break
    time.sleep(0.25)
log("version %s, first capture %s, the picture set once showed after %s ms" % (subprocess.run(["gnome-shell", "--version"], capture_output=True, text=True).stdout.strip(), look(), first))
shown, times, faded = 0, [], 0
for index in range(1, count + 1):
    target = index % 2
    began = time.time()
    staged = os.path.join(folder, ".next.jpg")
    with open(staged, "wb") as file:
        file.write(data[target])
        file.flush()
        os.fsync(file.fileno())
    os.rename(staged, fixed)
    seen, mid, captures = None, 0, 0
    while time.time() - began < 6:
        found = look()
        captures += 1
        if found == target:
            seen = int((time.time() - began) * 1000)
            break
        if found == 2:
            mid += 1
    if mid:
        faded += 1
    if seen is None:
        log("change %d showed no new picture within 6 s after %d captures, last %s" % (index, captures, found))
    else:
        shown += 1
        times.append(seen)
    time.sleep(0.3)
times.sort()
log("rename: %d of %d changes showed the new picture, median %s ms, most %s ms, %d changes had captures between the pictures" % (shown, count, statistics.median(times) if times else None, times[-1] if times else None, faded))
