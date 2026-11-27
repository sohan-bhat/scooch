#!/usr/bin/env bash
set -eu

name="$1"
root="$(cd "$(dirname "$0")/../.." && pwd)"
python3 - "$name" "$root/crates/scooch-app/src" <<'PY'
import sys

name, source = sys.argv[1:3]
swaps = {
    "once": ("life.rs", "            deadline = deadline.min(std::time::Instant::now() + lag);\n", "            deadline = std::time::Instant::now();\n"),
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
