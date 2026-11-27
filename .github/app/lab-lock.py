import fcntl
import os
import statistics
import subprocess
import sys
import tempfile
import time

CHILD = r'''
import fcntl, os, sys, threading, time
path, megabytes, threads = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
held = open(path, "w")
fcntl.flock(held, fcntl.LOCK_EX)
blob = bytearray(megabytes << 20)
for at in range(0, len(blob), 4096):
    blob[at] = 1
for _ in range(threads):
    threading.Thread(target=time.sleep, args=(3600,), daemon=True).start()
sys.stdout.write("ready\n")
sys.stdout.flush()
sys.stdin.readline()
os._exit(0)
'''


def state(pid):
    try:
        with open(f"/proc/{pid}/stat") as stat:
            return stat.read().rsplit(") ", 1)[1][0]
    except (OSError, IndexError):
        return "-"


def tasks(pid):
    try:
        return len(os.listdir(f"/proc/{pid}/task"))
    except OSError:
        return 0


def taken(lock):
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except OSError:
        return False
    fcntl.flock(lock, fcntl.LOCK_UN)
    return True


def round_of(path, megabytes, threads):
    child = subprocess.Popen([sys.executable, "-c", CHILD, path, str(megabytes), str(threads)], stdin=subprocess.PIPE, stdout=subprocess.PIPE)
    child.stdout.readline()
    lock = open(path, "w")
    child.stdin.write(b"\n")
    child.stdin.flush()
    while state(child.pid) not in "ZX-":
        pass
    seen = time.perf_counter()
    free = taken(lock)
    left = tasks(child.pid)
    lag = 0.0
    if not free:
        while not taken(lock):
            pass
        lag = (time.perf_counter() - seen) * 1000
    child.wait()
    lock.close()
    return free, left, lag


def main():
    folder = tempfile.mkdtemp()
    path = os.path.join(folder, "icon.lock")
    for megabytes, threads in [(0, 0), (0, 20), (150, 20), (300, 20), (300, 60)]:
        held, lags, lefts = 0, [], []
        rounds = 80
        for _ in range(rounds):
            free, left, lag = round_of(path, megabytes, threads)
            if not free:
                held += 1
                lags.append(lag)
                lefts.append(left)
        line = f"{megabytes} MB, {threads} threads, {rounds} rounds, the lock was still held at the first read of the ended state in {held}"
        if lags:
            line += f", freed after a median of {statistics.median(lags):.2f} ms and at most {max(lags):.2f} ms, tasks still listed then {min(lefts)} to {max(lefts)}"
        print(line, flush=True)


main()
