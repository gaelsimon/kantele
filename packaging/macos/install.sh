#!/bin/sh
# Installs the binary, the configuration and the launch agent for whoever runs this.
set -eu
here="$(cd "$(dirname "$0")" && pwd)"
bin="$HOME/.local/bin"
support="$HOME/Library/Application Support/Kantele"
agents="$HOME/Library/LaunchAgents"
logs="$HOME/Library/Logs"
label=com.kantele.server

mkdir -p "$bin" "$support" "$agents" "$logs"
install -m 755 "$here/kantele" "$bin/kantele"
# Downloaded binaries carry a quarantine flag, and launchd refuses to start one.
xattr -d com.apple.quarantine "$bin/kantele" 2>/dev/null || true
[ -f "$support/kantele.toml" ] || cp "$here/kantele.example.toml" "$support/kantele.toml"

sed -e "s|__BIN__|$bin/kantele|g" \
    -e "s|__SUPPORT__|$support|g" \
    -e "s|__LOGS__|$logs|g" \
    "$here/com.kantele.server.plist" > "$agents/$label.plist"

launchctl bootout "gui/$(id -u)/$label" 2>/dev/null || true
launchctl bootstrap "gui/$(id -u)" "$agents/$label.plist"

echo "Kantele is running. Open http://localhost:8200/config and choose a music folder."
echo "The log is $support/kantele.log, and the page shows the end of it."
