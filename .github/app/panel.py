import sys

import pyatspi

BUTTONS = {"push button", "toggle button", "button"}
ITEMS = {"menu item", "check menu item", "radio menu item"}


def walk(node, depth=0):
    if node is None or depth > 60:
        return
    yield node
    try:
        count = node.childCount
    except Exception:
        return
    for index in range(count):
        try:
            yield from walk(node.getChildAtIndex(index), depth + 1)
        except Exception:
            continue


def panel():
    for app in pyatspi.Registry.getDesktop(0):
        if app is not None and app.name == "xfce4-panel":
            return app
    return None


def box(node):
    try:
        extents = node.queryComponent().getExtents(pyatspi.DESKTOP_COORDS)
        return extents.x, extents.y, extents.width, extents.height
    except Exception:
        return None


def describe(node):
    try:
        return "%s|%s|%s" % (node.getRoleName(), node.name or "", node.description or "")
    except Exception:
        return "unknown"


def buttons():
    found = []
    root = panel()
    if root is None:
        return found
    for node in walk(root):
        try:
            if node.getRoleName() in BUTTONS:
                place = box(node)
                if place and place[2] > 0:
                    found.append((place, describe(node)))
        except Exception:
            continue
    return found


def menu_items():
    told = []
    for app in pyatspi.Registry.getDesktop(0):
        if app is None:
            continue
        for node in walk(app):
            try:
                role = node.getRoleName()
                if role not in ITEMS or not node.getState().contains(pyatspi.STATE_SHOWING):
                    continue
                checked = node.getState().contains(pyatspi.STATE_CHECKED)
                told.append((node, node.name, role == "check menu item", checked))
            except Exception:
                continue
    return told


def main():
    mode = sys.argv[1]
    if mode == "buttons":
        print(";".join("%d,%d,%d,%d %s" % (place + (said,)) for place, said in buttons()) or "none")
    elif mode == "menu":
        print("|".join("%s%s" % (name, (" checked" if checked else " unchecked") if checkable else "") for _, name, checkable, checked in menu_items()) or "none")
    elif mode == "press":
        for node, name, _, _ in menu_items():
            if name == sys.argv[2]:
                action = node.queryAction()
                names = [action.getName(index) for index in range(action.nActions)]
                pick = next((index for index, each in enumerate(names) if each in ("click", "activate", "press")), 0)
                print("pressed %s in the panel's menu through AT-SPI" % name if action.doAction(pick) else "could not press %s" % name)
                return
        print("no %s showing in a menu" % sys.argv[2])


main()
