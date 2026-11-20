take() {
  local from="$compiled/$1" to="$2"
  if [ ! -f "$from" ] || [ -L "$from" ]; then
    echo "The compiled programs hold no plain file named $1."
    exit 1
  fi
  mkdir -p "$(dirname "$to")"
  cp "$from" "$to"
  chmod 755 "$to"
}

bundler() {
  (cd "$app/ui" && npm ci --ignore-scripts --no-audit --no-fund >/dev/null)
}

bundle() {
  local attempt
  for attempt in 1 2 3; do
    if (cd "$app" && node ui/node_modules/@tauri-apps/cli/tauri.js bundle --ci "$@"); then
      return 0
    fi
    echo "The bundle failed, trying again in 15 s."
    sleep 15
  done
  return 1
}
