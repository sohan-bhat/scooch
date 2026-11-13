#!/usr/bin/env bash
set -u

script="$1"
mkdir -p linuxprobe
log=linuxprobe/packages.log

install_arch() {
    pacman-key --init
    pacman-key --populate archlinux
    pacman -Sy --noconfirm archlinux-keyring
    pacman -Su --noconfirm
    pacman -S --noconfirm --needed xorg-server-xvfb xorg-xrandr xorg-xprop xorg-xwininfo xorg-xdpyinfo dbus xdg-user-dirs glib2 gvfs procps-ng util-linux noto-fonts ttf-dejavu ${PACKAGES}
    for extra in ${EXTRAS}; do
        pacman -S --noconfirm --needed "$extra" || echo "skipped $extra"
    done
    systemd-sysusers || true
}

install_debian() {
    export DEBIAN_FRONTEND=noninteractive
    apt-get update
    apt-get install -y --no-install-recommends xvfb x11-utils x11-xserver-utils dbus dbus-x11 xdg-user-dirs libglib2.0-bin gvfs gvfs-daemons procps util-linux passwd fonts-noto-core fonts-dejavu-core ${PACKAGES}
    for extra in ${EXTRAS}; do
        apt-get install -y --no-install-recommends "$extra" || echo "skipped $extra"
    done
}

versions() {
    if command -v pacman >/dev/null 2>&1; then
        pacman -Q ${PACKAGES} ${EXTRAS} 2>/dev/null
    else
        dpkg-query -W -f '${Package} ${Version}\n' ${PACKAGES} ${EXTRAS} 2>/dev/null
    fi
}

if command -v pacman >/dev/null 2>&1; then
    installer=install_arch
else
    installer=install_debian
fi
if ! "$installer" >"$log" 2>&1; then
    echo "package install failed"
    tail -40 "$log"
    exit 1
fi
versions >linuxprobe/versions.txt || true

dbus-uuidgen --ensure
mkdir -p /run/dbus /tmp/.X11-unix
chmod 1777 /tmp/.X11-unix
dbus-daemon --system --fork >>"$log" 2>&1 || echo "no system bus" >>"$log"
id -u tester >/dev/null 2>&1 || useradd -m -u "${HOST_UID:-1001}" tester
chown -R tester linuxprobe

exec runuser -u tester -- env -i HOME=/home/tester USER=tester LOGNAME=tester PATH=/usr/local/sbin:/usr/local/bin:/usr/bin:/bin LANG=C.UTF-8 \
    SCOOCH_SCALE="${SCOOCH_SCALE:-1}" SCOOCH_MONITORS="${SCOOCH_MONITORS:-1}" \
    bash -c 'XDG_RUNTIME_DIR="$(mktemp -d)"; chmod 700 "$XDG_RUNTIME_DIR"; export XDG_RUNTIME_DIR; exec dbus-run-session -- bash ".github/linux/$0.sh" ./target/release/examples/linuxprobe linuxprobe' "$script"
