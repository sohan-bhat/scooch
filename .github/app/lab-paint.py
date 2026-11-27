import ctypes
import ctypes.util
import sys
import time

STEP = 0.02


class Image(ctypes.Structure):
    _fields_ = [
        ("width", ctypes.c_int),
        ("height", ctypes.c_int),
        ("xoffset", ctypes.c_int),
        ("format", ctypes.c_int),
        ("data", ctypes.POINTER(ctypes.c_ubyte)),
        ("byte_order", ctypes.c_int),
        ("bitmap_unit", ctypes.c_int),
        ("bitmap_bit_order", ctypes.c_int),
        ("bitmap_pad", ctypes.c_int),
        ("depth", ctypes.c_int),
        ("bytes_per_line", ctypes.c_int),
        ("bits_per_pixel", ctypes.c_int),
    ]


HANDLER = ctypes.CFUNCTYPE(ctypes.c_int, ctypes.c_void_p, ctypes.c_void_p)
quiet = HANDLER(lambda display, event: 0)


def colours(x11, display, window, width, height):
    seen = set()
    for row in (height // 5, height // 2, height * 4 // 5):
        image = x11.XGetImage(display, window, 0, row, width, 1, 0xFFFFFFFFFFFFFFFF, 2)
        if not image:
            return None
        held = ctypes.cast(image, ctypes.POINTER(Image)).contents
        size = held.bits_per_pixel // 8
        if size < 3:
            return None
        raw = ctypes.string_at(held.data, held.bytes_per_line)
        for at in range(0, width * size, size * 4):
            seen.add(raw[at:at + 3])
    return len(seen)


def main():
    named, started, limit = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
    x11 = ctypes.CDLL(ctypes.util.find_library("X11") or "libX11.so.6")
    x11.XOpenDisplay.restype = ctypes.c_void_p
    x11.XOpenDisplay.argtypes = [ctypes.c_char_p]
    x11.XGetImage.restype = ctypes.c_void_p
    x11.XGetImage.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int, ctypes.c_int, ctypes.c_uint, ctypes.c_uint, ctypes.c_ulong, ctypes.c_int]
    x11.XGetGeometry.argtypes = [ctypes.c_void_p, ctypes.c_ulong] + [ctypes.c_void_p] * 7
    x11.XSetErrorHandler.argtypes = [HANDLER]
    x11.XSetErrorHandler.restype = ctypes.c_void_p
    display = x11.XOpenDisplay(None)
    if not display:
        print("-1 -1 0")
        return
    x11.XSetErrorHandler(quiet)
    window = 0
    while not window and int(time.time() * 1000) - started <= limit:
        try:
            with open(named) as handle:
                window = int(handle.read().strip() or 0)
        except (OSError, ValueError):
            window = 0
        if not window:
            time.sleep(0.005)
    if not window:
        print("-1 -1 0")
        return
    root, x, y, border, depth = ctypes.c_ulong(), ctypes.c_int(), ctypes.c_int(), ctypes.c_uint(), ctypes.c_uint()
    width, height = ctypes.c_uint(), ctypes.c_uint()
    x11.XGetGeometry(display, window, ctypes.byref(root), ctypes.byref(x), ctypes.byref(y), ctypes.byref(width), ctypes.byref(height), ctypes.byref(border), ctypes.byref(depth))
    page, picture, most = -1, -1, 0
    while True:
        now = int(time.time() * 1000)
        if now - started > limit:
            break
        if width.value > 8 and height.value > 8:
            count = colours(x11, display, window, width.value, height.value) or 0
            most = max(most, count)
            if page < 0 and count >= 4:
                page = now - started
            if count >= 48:
                picture = now - started
                break
        else:
            x11.XGetGeometry(display, window, ctypes.byref(root), ctypes.byref(x), ctypes.byref(y), ctypes.byref(width), ctypes.byref(height), ctypes.byref(border), ctypes.byref(depth))
        time.sleep(STEP)
    print(f"{page} {picture} {most}")


main()
