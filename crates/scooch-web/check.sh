#!/bin/sh
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
target=${CARGO_TARGET_DIR:-$root/target}
"$here/build.sh" --node
walk="$target/scooch-web-walk"
rm -rf "$walk"
cargo run --manifest-path "$root/Cargo.toml" -q -p scooch-web --features portable --release --example walk -- "$walk"
node "$here/tests/node.mjs" "$target/scooch-web-node" "$walk"
rm -rf "$walk"
