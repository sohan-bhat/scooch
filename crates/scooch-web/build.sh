#!/bin/sh
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
target=${CARGO_TARGET_DIR:-$root/target}
version=0.2.130
if ! command -v wasm-bindgen >/dev/null 2>&1 || [ "$(wasm-bindgen --version | cut -d' ' -f2)" != "$version" ]; then
  echo "installing wasm-bindgen-cli $version from crates.io"
  cargo install wasm-bindgen-cli --version "$version" --locked
fi
rustup target add wasm32-unknown-unknown >/dev/null
CARGO_PROFILE_RELEASE_OPT_LEVEL=s \
CARGO_PROFILE_RELEASE_LTO=fat \
CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 \
CARGO_PROFILE_RELEASE_PANIC=abort \
CARGO_PROFILE_RELEASE_STRIP=false \
  cargo build --manifest-path "$root/Cargo.toml" -p scooch-web --release --target wasm32-unknown-unknown
wasm="$target/wasm32-unknown-unknown/release/scooch_web.wasm"
rm -rf "$here/pkg"
wasm-bindgen --target web --remove-name-section --remove-producers-section --out-dir "$here/pkg" "$wasm"
cp "$here/js/engine.js" "$here/js/worker.js" "$here/pkg/"
if [ "${1:-}" = "--node" ]; then
  rm -rf "$target/scooch-web-node"
  wasm-bindgen --target nodejs --no-typescript --out-dir "$target/scooch-web-node" "$wasm"
fi
total=0
gzipped=0
for file in "$here/pkg"/*; do
  raw=$(wc -c < "$file" | tr -d ' ')
  packed=$(gzip -9 -c "$file" | wc -c | tr -d ' ')
  total=$((total + raw))
  gzipped=$((gzipped + packed))
  printf '%-24s %9s bytes %9s gzipped\n' "$(basename "$file")" "$raw" "$packed"
done
printf '%-24s %9s bytes %9s gzipped\n' "all" "$total" "$gzipped"
