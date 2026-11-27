import json
import os
import re
import sys
import time

PROC = os.environ.get("LAB_PROC", "/proc")
HEAD = re.compile(r"^([0-9a-f]+)-([0-9a-f]+) (\S{4}) ([0-9a-f]+) (\S+) (\d+)\s*(.*)$")
FIELD = re.compile(r"^(\w+):\s+(\d+) kB")
KEYS = ("Rss", "Pss", "Private_Clean", "Private_Dirty", "Shared_Clean", "Shared_Dirty", "Anonymous", "Swap")
KINDS = ("heap", "anonymous", "file", "shared", "stack", "other")


def read(path):
    try:
        with open(path, "r", errors="replace") as handle:
            return handle.read()
    except OSError:
        return ""


def mb(kilobytes):
    return "%.1f" % (kilobytes / 1024)


def stat(pid):
    text = read(f"{PROC}/{pid}/stat")
    close = text.rfind(")")
    if close < 0:
        return None
    rest = text[close + 2:].split()
    if len(rest) < 20:
        return None
    return {"pid": int(pid), "comm": text[text.find("(") + 1:close], "state": rest[0], "ppid": int(rest[1]), "ticks": int(rest[11]) + int(rest[12]), "threads": int(rest[17]), "start": int(rest[19])}


def processes():
    found = {}
    for name in os.listdir(PROC):
        if name.isdigit():
            entry = stat(name)
            if entry:
                found[entry["pid"]] = entry
    return found


def below(root, table):
    children = {}
    for entry in table.values():
        children.setdefault(entry["ppid"], []).append(entry["pid"])
    found, queue = [], sorted(children.get(root, []))
    while queue:
        pid = queue.pop(0)
        found.append(pid)
        queue.extend(sorted(children.get(pid, [])))
    return found


def command(pid):
    return " ".join(part for part in read(f"{PROC}/{pid}/cmdline").split("\0") if part)[:200]


def rollup(pid):
    values = {}
    for line in read(f"{PROC}/{pid}/smaps_rollup").splitlines():
        match = FIELD.match(line)
        if match:
            values[match.group(1)] = int(match.group(2))
    if "Rss" not in values:
        return None
    return {key: values.get(key, 0) for key in KEYS + ("Pss_Anon", "Pss_File", "Pss_Shmem")}


def kind(name):
    if name == "[heap]":
        return "heap"
    if name == "" or name.startswith("[anon"):
        return "anonymous"
    if name.startswith("[stack"):
        return "stack"
    if name.startswith("["):
        return "other"
    if name.startswith(("/memfd:", "/dev/", "/SYSV")) or name.endswith("(deleted)"):
        return "shared"
    return "file"


def split(pid):
    kinds = {name: dict({key: 0 for key in KEYS}, size=0, count=0) for name in KINDS}
    files, pieces, current = {}, [], None

    def add(mapping):
        sort = kind(mapping["name"])
        into = kinds[sort]
        into["size"] += mapping["size"]
        into["count"] += 1
        for key in KEYS:
            into[key] += mapping.get(key, 0)
        if sort in ("file", "shared"):
            entry = files.setdefault(mapping["name"], dict({key: 0 for key in KEYS}, size=0, kind=sort))
            entry["size"] += mapping["size"]
            for key in KEYS:
                entry[key] += mapping.get(key, 0)
        if sort == "anonymous":
            pieces.append({"at": mapping["at"], "perms": mapping["perms"], "size": mapping["size"], "rss": mapping.get("Rss", 0), "private": mapping.get("Private_Clean", 0) + mapping.get("Private_Dirty", 0)})

    for line in read(f"{PROC}/{pid}/smaps").splitlines():
        head = HEAD.match(line)
        if head:
            if current is not None:
                add(current)
            current = {"at": head.group(1), "perms": head.group(3), "name": head.group(7).strip(), "size": (int(head.group(2), 16) - int(head.group(1), 16)) // 1024}
            continue
        field = FIELD.match(line)
        if field and current is not None:
            current[field.group(1)] = int(field.group(2))
    if current is not None:
        add(current)
    return kinds, files, pieces


def private(entry):
    return entry.get("Private_Clean", 0) + entry.get("Private_Dirty", 0)


def threads(pid):
    found = []
    try:
        tasks = os.listdir(f"{PROC}/{pid}/task")
    except OSError:
        return found
    for task in tasks:
        name = read(f"{PROC}/{pid}/task/{task}/comm").strip().replace(" ", "_")
        switches = 0
        for line in read(f"{PROC}/{pid}/task/{task}/status").splitlines():
            if "ctxt_switches" in line:
                switches += int(line.split()[1])
        found.append((int(task), name, switches))
    return found


def grouped(names):
    counts = {}
    for name in names:
        counts[name] = counts.get(name, 0) + 1
    return counts


def told(counts):
    return ", ".join(f"{name} x{count}" if count > 1 else name for name, count in sorted(counts.items(), key=lambda item: (-item[1], item[0])))


def numbers(values):
    return f"{mb(values['Rss'])} MB resident, {mb(values['Pss'])} MB proportional, {mb(private(values))} MB private"


def meminfo():
    values = {}
    for line in read(f"{PROC}/meminfo").splitlines():
        match = FIELD.match(line)
        if match and match.group(1) in ("MemTotal", "MemFree", "MemAvailable", "Cached", "SwapTotal", "SwapFree"):
            values[match.group(1)] = int(match.group(2))
    return values


def save(folder, record):
    with open(os.path.join(folder, "data.jsonl"), "a") as handle:
        handle.write(json.dumps(record) + "\n")


def described(pid, table):
    entry = table.get(pid) or stat(pid)
    if entry is None:
        return None
    return {"pid": pid, "ppid": entry["ppid"], "comm": entry["comm"], "state": entry["state"], "threads": entry["threads"], "command": command(pid), "rollup": rollup(pid)}


def snap(pid, label, folder):
    table = processes()
    own = table.get(pid)
    if own is None:
        print(f"- no process {pid}")
        return
    values = rollup(pid)
    if values is None:
        print(f"- the memory of process {pid} could not be read")
        return
    kinds, files, pieces = split(pid)
    lanes = threads(pid)
    names = grouped(name for _, name, _ in lanes)
    print(f"- the icon process {own['comm']} {pid} holds {numbers(values)}, of the private part {mb(values['Private_Clean'])} MB clean and {mb(values['Private_Dirty'])} MB dirty, {mb(values['Anonymous'])} MB anonymous in all, {mb(values['Swap'])} MB swapped")
    print(f"- its private memory by kind: heap {mb(private(kinds['heap']))} MB, anonymous mappings {mb(private(kinds['anonymous']))} MB in {kinds['anonymous']['count']} mappings, pages of files {mb(kinds['file']['Private_Clean'])} MB clean and {mb(kinds['file']['Private_Dirty'])} MB dirty in {len([1 for entry in files.values() if entry['kind'] == 'file'])} files, shared memory and devices {mb(private(kinds['shared']))} MB, stack {mb(private(kinds['stack']))} MB, other {mb(private(kinds['other']))} MB")
    print(f"- its resident memory by kind: heap {mb(kinds['heap']['Rss'])} MB, anonymous mappings {mb(kinds['anonymous']['Rss'])} MB, files {mb(kinds['file']['Rss'])} MB, shared memory and devices {mb(kinds['shared']['Rss'])} MB, stack {mb(kinds['stack']['Rss'])} MB, other {mb(kinds['other']['Rss'])} MB")
    ranked = sorted(files.items(), key=lambda item: -private(item[1]))
    top = [f"{os.path.basename(name) or name} {mb(private(entry))} MB ({mb(entry['Private_Clean'])} clean, {mb(entry['Private_Dirty'])} dirty, {mb(entry['Rss'])} resident)" for name, entry in ranked[:10] if private(entry) > 0]
    print(f"- the ten files with the most private pages: {'; '.join(top) or 'none'}")
    largest = sorted(pieces, key=lambda piece: -piece["private"])[:8]
    print(f"- the largest anonymous mappings, as private MB of mapped MB: {', '.join(mb(piece['private']) + ' of ' + mb(piece['size']) + ' ' + piece['perms'] for piece in largest if piece['private'] > 0) or 'none'}")
    print(f"- its {len(lanes)} threads: {told(names)}")
    under = [described(child, table) for child in below(pid, table)]
    under = [entry for entry in under if entry is not None]
    if under:
        for entry in under:
            held = numbers(entry["rollup"]) if entry["rollup"] else ("defunct" if entry["state"] == "Z" else "memory not readable")
            print(f"- below the icon process: {entry['comm']} {entry['pid']} with parent {entry['ppid']}, {held}, {entry['threads']} threads, started as {entry['command'][:90]}")
        total = {key: sum(entry["rollup"][key] for entry in under if entry["rollup"]) for key in KEYS}
        print(f"- the {len(under)} processes below the icon process hold {numbers(total)} together, and with the icon process {mb(values['Pss'] + total['Pss'])} MB proportional and {mb(private(values) + private(total))} MB private")
    else:
        print("- below the icon process: no process")
    known = {pid} | {entry["pid"] for entry in under}
    apart = [described(other, table) for other, entry in sorted(table.items()) if other not in known and (entry["comm"].startswith("WebKit") or entry["comm"] in ("scooch-app", "scooch", "bwrap", "xdg-dbus-proxy"))]
    apart = [entry for entry in apart if entry is not None]
    for entry in apart:
        held = numbers(entry["rollup"]) if entry["rollup"] else ("defunct" if entry["state"] == "Z" else "memory not readable")
        print(f"- outside the icon process's tree: {entry['comm']} {entry['pid']} with parent {entry['ppid']}, {held}, started as {entry['command'][:90]}")
    free = meminfo()
    print(f"- the machine has {mb(free.get('MemAvailable', 0))} MB available of {mb(free.get('MemTotal', 0))} MB")
    save(folder, {"what": "snap", "label": label, "at": time.time(), "pid": pid, "comm": own["comm"], "rollup": values, "kinds": kinds, "files": dict(ranked[:30]), "pieces": sorted(pieces, key=lambda piece: -piece["private"])[:20], "threads": names, "thread_count": len(lanes), "below": under, "apart": apart, "meminfo": free})
    with open(os.path.join(folder, f"smaps-{re.sub(r'[^a-z0-9]+', '-', label.lower()).strip('-')}.txt"), "w") as handle:
        handle.write(read(f"{PROC}/{pid}/smaps"))


def wakes(pid, seconds, label, folder):
    before = {task: (name, switches) for task, name, switches in threads(pid)}
    own = stat(pid)
    if own is None or not before:
        print(f"- no process {pid} to count wakes of")
        return
    time.sleep(seconds)
    after = threads(pid)
    later = stat(pid)
    if later is None or not after:
        print(f"- the process {pid} ended while its wakes were counted")
        return
    woke = {}
    for task, name, switches in after:
        delta = switches - before.get(task, (name, 0))[1]
        if delta > 0:
            woke[name] = woke.get(name, 0) + delta
    total = sum(woke.values())
    used = (later["ticks"] - own["ticks"]) * 1000 // os.sysconf("SC_CLK_TCK")
    names = " ".join(f"{name}:{count}" for name, count in sorted(woke.items(), key=lambda item: -item[1]))
    print(f"- over {seconds} s, {label}, the icon process {pid} woke its {len(after)} threads {total} times, {names or 'none of them'}, and used {used} ms of CPU")
    save(folder, {"what": "wakes", "label": label, "at": time.time(), "pid": pid, "seconds": seconds, "threads": len(after), "total": total, "woke": woke, "cpu_ms": used})


def watch(pid, seconds, path):
    started = time.monotonic()
    table = processes()
    followed = {pid: table.get(pid)}
    for child in below(pid, table):
        followed[child] = table[child]
    with open(path, "w") as handle:
        def say(text):
            handle.write(f"{time.monotonic() - started:.1f} s {text}\n")
            handle.flush()
        say("watching " + ", ".join(f"{entry['comm']} {key}" for key, entry in followed.items() if entry))
        alive = {key for key, entry in followed.items() if entry}
        while time.monotonic() - started < seconds:
            time.sleep(0.2)
            table = processes()
            for key in sorted(alive):
                now = table.get(key)
                if now is None or now["start"] != followed[key]["start"] or now["state"] == "Z":
                    say(f"{followed[key]['comm']} {key} ended")
                    alive.discard(key)
            for key in list(alive):
                for child in below(key, table):
                    if child not in followed:
                        followed[child] = table[child]
                        alive.add(child)
                        say(f"{table[child]['comm']} {child} started below {table[child]['ppid']} as {command(child)[:80]}")
        say("still running " + (", ".join(f"{followed[key]['comm']} {key}" for key in sorted(alive)) or "none of them"))


def main():
    what = sys.argv[1]
    if what == "snap":
        snap(int(sys.argv[2]), sys.argv[3], sys.argv[4])
    elif what == "wakes":
        wakes(int(sys.argv[2]), int(sys.argv[3]), sys.argv[4], sys.argv[5])
    elif what == "watch":
        watch(int(sys.argv[2]), float(sys.argv[3]), sys.argv[4])


main()
