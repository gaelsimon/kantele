# Kantele on Linux

    sudo ./install.sh

The binary is static, so it wants nothing installed alongside it. The installer puts:

| Where | What |
|---|---|
| `/usr/local/bin/kantele` | the binary |
| `/etc/kantele/kantele.toml` | the configuration, left alone if it is already there |
| `/var/lib/kantele/index.sqlite` | the saved index |
| `/etc/systemd/system/kantele.service` | the service, which starts it at boot |

Then open <http://localhost:8200/config> and choose a music folder.

The server runs as the `kantele` system user, which has to be able to read your music. A library
under `/home` usually needs its folder opened up, and one on its own mount usually already works.
Without read access every folder comes back empty.

## Stopping, starting, removing

    systemctl stop kantele
    systemctl start kantele
    journalctl -u kantele -f
    sudo ./uninstall.sh

The server also keeps its own log, `/var/lib/kantele/kantele.log`, rolled by itself, and the
Settings tab of the page shows the end of it.

`uninstall.sh` leaves the configuration and the index where they are.

## A large library and folder watches

The server watches the music folder and notices a new album at once. The kernel allows a limited
number of watches, `fs.inotify.max_user_watches`, and a library with more folders than that is
watched not at all: the server says so in the log and falls back to the timed check, fifteen
minutes by default. Raising the limit is a line in `/etc/sysctl.d/`.
