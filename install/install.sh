#!/bin/sh
set -eu

repo="sohan-bhat/scooch"
base="${SCOOCH_RELEASE_URL:-https://github.com/$repo/releases/latest/download}"

say() {
  printf '%s\n' "$*"
}

fail() {
  say "$*" >&2
  exit 1
}

fetch() {
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$1" -o "$2"
  elif command -v wget >/dev/null 2>&1; then
    wget -q "$1" -O "$2"
  else
    fail "Install curl or wget, then run this again."
  fi
}

digest() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

on_path() {
  case ":$PATH:" in
    *":$1:"*) return 0 ;;
    *) return 1 ;;
  esac
}

choose() {
  if [ -n "${SCOOCH_INSTALL:-}" ]; then
    say "$SCOOCH_INSTALL"
    return
  fi
  for dir in "$HOME/.local/bin" "$HOME/bin" /usr/local/bin /opt/homebrew/bin; do
    if on_path "$dir" && [ -d "$dir" ] && [ -w "$dir" ]; then
      say "$dir"
      return
    fi
  done
  say "$HOME/.local/bin"
}

remember() {
  dir="$1"
  shell_name=$(basename "${SHELL:-sh}")
  case "$shell_name" in
    fish)
      file="$HOME/.config/fish/conf.d/scooch.fish"
      line="fish_add_path \"$dir\""
      ;;
    zsh)
      file="${ZDOTDIR:-$HOME}/.zshrc"
      line="export PATH=\"$dir:\$PATH\""
      ;;
    bash)
      if [ "$(uname -s)" = "Darwin" ]; then
        file="$HOME/.bash_profile"
      else
        file="$HOME/.bashrc"
      fi
      line="export PATH=\"$dir:\$PATH\""
      ;;
    *)
      file="$HOME/.profile"
      line="export PATH=\"$dir:\$PATH\""
      ;;
  esac
  mkdir -p "$(dirname "$file")"
  if [ ! -f "$file" ] || ! grep -qF "$dir" "$file"; then
    printf '\n%s\n' "$line" >> "$file"
  fi
}

os=$(uname -s)
arch=$(uname -m)

case "$os" in
  Darwin)
    asset="scooch-macos.tar.gz"
    ;;
  Linux)
    case "$arch" in
      x86_64 | amd64) asset="scooch-linux-x86_64.tar.gz" ;;
      aarch64 | arm64) asset="scooch-linux-aarch64.tar.gz" ;;
      *) fail "Scooch has no build for $arch yet." ;;
    esac
    ;;
  *)
    fail "Scooch has no build for $os here. On Windows, use the PowerShell command from the website."
    ;;
esac

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

fetch "$base/$asset" "$tmp/$asset" || fail "The download failed, so check your connection and run this again."

if fetch "$base/SHA256SUMS" "$tmp/SHA256SUMS" 2>/dev/null; then
  want=$(grep " $asset\$" "$tmp/SHA256SUMS" | cut -d' ' -f1 || true)
  got=$(digest "$tmp/$asset" || true)
  if [ -n "$want" ] && [ -n "$got" ] && [ "$want" != "$got" ]; then
    fail "The download did not match its checksum, so nothing was installed. Run this again."
  fi
fi

tar -xzf "$tmp/$asset" -C "$tmp"
[ -f "$tmp/scooch" ] || fail "The download was not a Scooch build, so nothing was installed."

dir=$(choose)
mkdir -p "$dir" || fail "Scooch could not make $dir, so set SCOOCH_INSTALL to a folder you own and run this again."
cp "$tmp/scooch" "$dir/.scooch.new"
chmod 755 "$dir/.scooch.new"
mv -f "$dir/.scooch.new" "$dir/scooch"

if on_path "$dir"; then
  say "Scooch is installed. Run scooch to turn it on."
elif [ -n "${SCOOCH_INSTALL:-}" ]; then
  say "Scooch is installed in $dir. Add that folder to your PATH, then run scooch to turn it on."
else
  remember "$dir"
  say "Scooch is installed in $dir. Open a new terminal, then run scooch to turn it on."
fi
