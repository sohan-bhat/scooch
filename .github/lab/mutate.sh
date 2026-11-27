#!/usr/bin/env bash
set -eu

name="$1"
root="$(cd "$(dirname "$0")/../.." && pwd)"
python3 - "$name" "$root/crates/scooch-app/src" <<'PY'
import sys

name, source = sys.argv[1:3]
arm = "        Step::Stay | Step::AtClose if fresh && change == Change::Same => Close::Fresh,\n"
swaps = {
    "never": ("life.rs", arm, arm.replace("if fresh &&", "if false && fresh &&")),
    "every": ("life.rs", arm, arm.replace("if fresh && change", "if change")),
    "update": ("life.rs", "    if !(on && icon) || update {\n", "    if !(on && icon) || (update && !fresh) {\n"),
    "failed": ("life.rs", "    door.stays();\n    Leave::Failed\n", "    Leave::Gone\n"),
    "unwired": ("main.rs", "fn fresh_at_close() -> bool {\n    session::watcher_now()\n}\n", "fn fresh_at_close() -> bool {\n    false\n}\n"),
}
file, old, new = swaps[name]
path = f"{source}/{file}"
text = open(path).read()
if text.count(old) != 1:
    sys.exit(f"The mutation {name} found its line {text.count(old)} times in {file}.")
open(path, "w").write(text.replace(old, new))
print(f"The mutation {name} changed {file}.")
PY
git -C "$root" diff --stat
