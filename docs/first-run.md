# First run

The server starts with no music folder chosen, and serves its page anyway. You do not have to edit
anything before it runs. Open the page and choose the folder:

- **Synology**: the Kantele icon in the DSM menu.
- **macOS**: <http://localhost:8200/config>.

The server announces the page on the local network under the name devices show. A Bonjour browser
lists it, and `http://<the machine name>.local:8200/config` opens it from another computer.

It opens on **Settings**, because that is where you choose the folder. Choose the folder your music
is in, and press Save.

## The first check reads every file

The server remembers nothing yet, so it opens every file. On a NAS with spinning disks, a large
library takes minutes. The progress strip at the top of the page shows how far it has got. You can
watch it, or close the page and come back later.

Every check after that is short. The server keeps what it read in a saved index beside itself. After
a restart it serves the whole library before it looks at the disk. A check that finds no change
opens no file and writes nothing.

## Your player finds it straight away

The server announces itself on the network as soon as it runs. A control point that already searches
finds it in a second or two. If it does not, see [troubleshooting](troubleshooting.md).

You can browse a library that the server still reads. The menus fill as the check goes on.

## Music you add later

Music you drop in appears on its own. The server looks for changes every fifteen minutes by default,
and reads only the folders that differ. You can change that under **Scan interval**. To ask for a
check now, use **Rescan all** on the page.

On macOS the server also watches the folder and notices at once. On a Synology it cannot. DSM allows
fewer folder watches than a large library has folders, so the timed check finds new music there.
Linux has the same limit, `fs.inotify.max_user_watches`, and uses the timed check too.

## Where the settings, index and log live

| | On a Synology | On a Mac | On Linux |
|---|---|---|---|
| Configuration | `/var/packages/kantele/var/kantele.toml` | `~/Library/Application Support/Kantele/kantele.toml` | `/etc/kantele/kantele.toml` |
| Saved index | the same folder | the same folder | `/var/lib/kantele` |
| Log | the same folder, `kantele.log` | `~/Library/Logs/kantele.log` | `journalctl -u kantele` |

The server never writes inside your music folder.
