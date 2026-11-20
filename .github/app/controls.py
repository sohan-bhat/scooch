import sys

import pyatspi

wanted = [name for name in sys.argv[1].split(",") if name]
roles = {"push button", "toggle button", "check box", "switch", "button", "radio button"}


def walk(node, found, depth=0):
    if node is None or depth > 80:
        return
    try:
        role = node.getRoleName()
        name = node.name
    except Exception:
        return
    if name in wanted and role in roles and name not in found:
        found[name] = node
    for index in range(node.childCount):
        try:
            walk(node.getChildAtIndex(index), found, depth + 1)
        except Exception:
            continue


desktop = pyatspi.Registry.getDesktop(0)
app = None
for candidate in desktop:
    if candidate is not None and candidate.name in ("scooch-app", "Scooch"):
        app = candidate
        break
if app is None:
    print("- no accessible Scooch window, apps " + ", ".join(str(item.name) for item in desktop if item is not None))
    sys.exit(0)
frame = app.getChildAtIndex(0)
box = frame.queryComponent().getExtents(pyatspi.DESKTOP_COORDS)
found = {}
walk(frame, found)
whole = True
parts = []
for name in wanted:
    node = found.get(name)
    if node is None:
        parts.append(name + " not shown")
        continue
    rect = node.queryComponent().getExtents(pyatspi.DESKTOP_COORDS)
    inside = rect.x >= box.x and rect.y >= box.y and rect.x + rect.width <= box.x + box.width and rect.y + rect.height <= box.y + box.height
    whole = whole and inside
    parts.append("%s %s" % (name, "inside" if inside else "cut"))
print("- every control whole inside the window %s, %s" % (whole, ", ".join(parts)))
