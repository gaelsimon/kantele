# First run

The server starts with no music folder chosen and serves its page anyway, so there is nothing to
edit before it will run. Open the page and choose the folder:

- **Synology**: the Kantele icon in the DSM menu.
- **macOS**: <http://localhost:8200/config>.

It opens on **Settings**, because that is where the folder picker is. Pick the folder your music is
in and press Save.

## The first check reads every file

Nothing is remembered yet, so every file is opened. On a NAS with spinning disks that is minutes
rather than seconds for a large library, and the progress strip at the top of the page counts it.
You can watch it, close the page, come back.

Every check after that is short. What was read is kept in a saved index beside the server, so a
restart serves the whole library before it has looked at the disk at all, and a check that finds
nothing changed opens no file and writes nothing.

## The amplifier finds it straight away

The server announces itself on the network as soon as it is running, and a control point that is
already looking finds it within a second or two. If it does not, see
[troubleshooting](troubleshooting.md).

A library that is still being read is already browsable. The menus fill as the check goes.

## Music you add later

Dropped-in music appears on its own. The server looks for changes every fifteen minutes by default,
and reads only the folders that differ. You can change that under **Checks for changes**, or ask for
one now with **Rescan all** on the page.

On macOS it also watches the folder and notices at once. On a Synology it cannot: DSM allows fewer
folder watches than a large library has folders, so the timed check is what finds new music there.
Linux has the same limit, `fs.inotify.max_user_watches`, and the same fallback.

## Where the settings, index and log live

| | On a Synology | On a Mac | On Linux |
|---|---|---|---|
| Configuration | `/var/packages/kantele/var/kantele.toml` | `~/Library/Application Support/Kantele/kantele.toml` | `/etc/kantele/kantele.toml` |
| Saved index | the same folder | the same folder | `/var/lib/kantele` |
| Log | the same folder, `kantele.log` | `~/Library/Logs/kantele.log` | `journalctl -u kantele` |

Nothing is ever written inside your music folder.
