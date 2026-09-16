#!/bin/sh
# Installs the binary, the configuration and a systemd service, run as root.
set -eu
here="$(cd "$(dirname "$0")" && pwd)"
bin=/usr/local/bin
config=/etc/kantele
state=/var/lib/kantele
unit=/etc/systemd/system/kantele.service

[ "$(id -u)" = 0 ] || { echo "run this as root: sudo ./install.sh" >&2; exit 1; }
command -v systemctl >/dev/null || { echo "this installer wants systemd; run the binary yourself otherwise" >&2; exit 1; }

# A system user with no login and no home: it reads music and writes its index, nothing else.
id kantele >/dev/null 2>&1 || useradd --system --no-create-home --shell /usr/sbin/nologin kantele

install -m 755 "$here/kantele" "$bin/kantele"
mkdir -p "$config" "$state"
[ -f "$config/kantele.toml" ] || install -m 644 "$here/kantele.example.toml" "$config/kantele.toml"
chown -R kantele:kantele "$state"

install -m 644 "$here/kantele.service" "$unit"
systemctl daemon-reload
systemctl enable --now kantele

echo "Kantele is running. Open http://localhost:8200/config and choose a music folder."
echo "The log is journalctl -u kantele."
echo
echo "The server runs as the kantele user, so that user needs read access to your music."
