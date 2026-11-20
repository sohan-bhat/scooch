#!/usr/bin/env bash
set -eu

archives="$1"
export DEBIAN_FRONTEND=noninteractive
mkdir -p "$archives/partial"
started=$(date +%s)
before=$(du -sk "$archives" | cut -f1)
mirror=$(grep -m1 -o '^[a-z]*://[^[:space:]]*' /etc/apt/apt-mirrors.txt 2>/dev/null || echo "the mirror in the apt sources")

report() {
    took=$(( $(date +%s) - started ))
    got=$(( $(du -sk "$archives" | cut -f1) - before ))
    echo "packages $1 after $took s, $got kB fetched at $(( got / (took > 0 ? took : 1) )) kB/s from $mirror"
}

trap 'report stopped; exit 124' TERM

get() {
    apt-get -o Dir::Cache::Archives="$archives/" "$@"
}

get update
get install -y --no-install-recommends dbus dbus-x11 dbus-user-session xdg-user-dirs procps
get install -y ${PACKAGES}
for extra in ${EXTRAS}; do
    get install -y --no-install-recommends "$extra" || echo "skipped $extra"
done
report done
