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
    if role in ("status bar", "statusbar") and "status" not in found:
        found["status"] = node
    for index in range(node.childCount):
        try:
            walk(node.getChildAtIndex(index), found, depth + 1)
        except Exception:
            continue


desktop = pyatspi.Registry.getDesktop(0)
mode = sys.argv[2] if len(sys.argv) > 2 else "whole"
pressing = mode == "press"
app = None
for candidate in desktop:
    if candidate is not None and candidate.name in ("scooch-app", "Scooch"):
        app = candidate
        break
if app is None:
    print("- no accessible Scooch window, apps " + ", ".join(str(item.name) for item in desktop if item is not None))
    sys.exit(0)
frame = app.getChildAtIndex(0)
if pressing:
    found = {}
    walk(frame, found)
    node = found.get(wanted[0]) if wanted else None
    if node is None:
        print("- no control named " + ",".join(wanted))
        sys.exit(0)
    action = node.queryAction()
    names = [action.getName(index) for index in range(action.nActions)]
    pick = next((index for index, name in enumerate(names) if name in ("click", "press", "activate")), 0)
    print("- %s %s" % ("pressed" if action.nActions and action.doAction(pick) else "could not press", wanted[0]))
    sys.exit(0)
if mode == "states":
    found = {}
    walk(frame, found)
    told = []
    for name in wanted:
        node = found.get(name)
        if node is None:
            told.append(name + " missing")
            continue
        states = node.getState()
        on = states.contains(pyatspi.STATE_CHECKED) or states.contains(pyatspi.STATE_PRESSED)
        told.append("%s %s" % (name, "on" if on else "off"))
    print("- states " + "; ".join(told))
    sys.exit(0)


def text_of(node, depth=0):
    if node is None or depth > 20:
        return ""
    try:
        text = node.queryText().getText(0, -1)
    except Exception:
        text = ""
    if text.strip():
        return text.strip()
    parts = []
    for index in range(node.childCount):
        try:
            part = text_of(node.getChildAtIndex(index), depth + 1)
        except Exception:
            part = ""
        if part:
            parts.append(part)
    return " ".join(parts) or (node.name or "").strip()


if mode == "status":
    found = {}
    walk(frame, found)
    print("- status " + text_of(found.get("status")))
    sys.exit(0)
box = frame.queryComponent().getExtents(pyatspi.DESKTOP_COORDS)
found = {}
walk(frame, found)
needed = 5
inside_all = True
parts = []
for name in wanted:
    node = found.get(name)
    if node is None:
        parts.append(name + " not shown")
        continue
    rect = node.queryComponent().getExtents(pyatspi.DESKTOP_COORDS)
    inside = rect.width > 0 and rect.x >= box.x and rect.y >= box.y and rect.x + rect.width <= box.x + box.width and rect.y + rect.height <= box.y + box.height
    inside_all = inside_all and inside
    parts.append("%s %s" % (name, "inside" if inside else "cut"))
status = found.pop("status", None)
line = status.queryComponent().getExtents(pyatspi.DESKTOP_COORDS).height if status is not None else 0
shown = len(found)
whole = inside_all and shown >= needed
print("- every control whole inside the window %s, found %d of at least %d, status line %d high, %s" % (whole, shown, needed, line, ", ".join(parts)))
