#!/bin/sh
# Stops the server and removes the binary and the launch agent. The configuration and the index
# stay, so a reinstall picks up where this left off.
set -eu
label=com.kantele.server
agent="$HOME/Library/LaunchAgents/$label.plist"

launchctl bootout "gui/$(id -u)/$label" 2>/dev/null || true
rm -f "$agent" "$HOME/.local/bin/kantele"

echo "Removed. The configuration and the index are still in"
echo "  $HOME/Library/Application Support/Kantele"
