# Kantele on macOS

    ./install.sh

There is nothing else to run. The installer puts:

| Where | What |
|---|---|
| `~/.local/bin/kantele` | the binary, for both Apple silicon and Intel |
| `~/Library/Application Support/Kantele/kantele.toml` | the configuration, left alone if it is already there |
| `~/Library/Application Support/Kantele/index.sqlite` | the saved index |
| `~/Library/LaunchAgents/com.kantele.server.plist` | the launch agent, which starts it at login |
| `~/Library/Application Support/Kantele/kantele.log` | the log |

Then open <http://localhost:8200/config> and choose a music folder.

The binary is not signed or notarised. macOS refuses a downloaded binary that is neither, so the
installer clears the quarantine flag. If you would rather not take that on faith, build it yourself:
`cargo build --release`.

macOS asks once whether to accept incoming connections. Refuse it and no device finds the
server, because discovery is a multicast exchange.

## Stopping, starting, removing

    launchctl bootout gui/$(id -u)/com.kantele.server
    launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/com.kantele.server.plist
    ./uninstall.sh

`uninstall.sh` leaves the configuration and the index where they are. Remove
`~/Library/Application Support/Kantele` by hand to be rid of those too.
