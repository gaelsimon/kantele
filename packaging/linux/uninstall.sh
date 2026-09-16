#!/bin/sh
# Stops the server and removes the binary and the service. The configuration and the index stay,
# so a reinstall picks up where this left off.
set -eu
[ "$(id -u)" = 0 ] || { echo "run this as root: sudo ./uninstall.sh" >&2; exit 1; }

systemctl disable --now kantele 2>/dev/null || true
rm -f /etc/systemd/system/kantele.service /usr/local/bin/kantele
systemctl daemon-reload 2>/dev/null || true

echo "Removed. The configuration is still in /etc/kantele and the index in /var/lib/kantele."
echo "The kantele user is still there too; userdel kantele removes it."
