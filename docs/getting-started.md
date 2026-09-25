# Getting started

Three steps: install the server, choose the folder your music is in, and open it on your player.

## 1. Install

### Synology

1. In Package Center, open **Settings**, then **Package Sources**, then **Add**, and give it this
   address:

       https://gaelsimon.github.io/kantele/index.json

   Kantele then appears in Package Center like any other package, and each new version arrives
   there as an update. One package runs on every model.
2. The package is not signed, so Package Center installs it only when **Settings**, **General**,
   **Trust Level** is set to **Any publisher**.
3. Install Kantele.
4. Give it read access to your music. In Control Panel, **Shared Folder**, select your music share,
   **Edit**, **Permissions**. Change the list at the top from **Local users** to **System internal
   user**, find `kantele`, and tick **Read only**. Without this, every folder looks empty to the
   server.

Prefer to install by hand? The `.spk` file on the
[release page](https://github.com/gaelsimon/kantele/releases/latest) installs under
**Manual Install**. You are then not told when a new version comes out.

### macOS

Download the macOS archive from the
[latest release](https://github.com/gaelsimon/kantele/releases/latest), open it, and run:

    ./install.sh

The server then starts at every login. The binary is not signed by Apple, so the installer clears
the flag macOS puts on downloaded files. The first time the server runs, macOS asks whether to
accept incoming connections: say yes, or no player can find it.

### Linux

Download the archive for your machine from the
[latest release](https://github.com/gaelsimon/kantele/releases/latest), and run inside it:

    sudo ./install.sh

The server then runs as a systemd service, as the `kantele` user. Make sure that user can read your
music folder.

## 2. Choose your music folder

Open the page:

- **Synology**: the Kantele icon in the DSM main menu.
- **macOS and Linux**: <http://localhost:8200/config>.
- **From another computer**: `http://<name of the machine>.local:8200/config`.

The page opens on **Settings**. Under **Your music**, choose the folder your music is in and press
**Save**. You can add several folders; they become one library.

The server now reads every file once. On a NAS with tens of thousands of tracks this takes a
quarter of an hour or more. The strip at the top of the page shows how far it has got.
You do not have to wait: your player can already browse what has been read, and the menus fill as
the reading goes on.

## 3. Open it on your player

Kantele appears in your player's list of media servers, under the name set in **Server name**. On
most players that list is called *Music server*, *Media server*, *UPnP* or *DLNA*. If it is not
there after a minute, see [troubleshooting](troubleshooting.md).

[Your library on your player](on-your-player.md) says what each menu holds.

## New music

Copy new albums into the folder and they appear on your player on their own. On a Mac the server
notices within seconds. On a Synology and on Linux, the system limits how many folders a program can
watch, so the server looks for changes on a timer instead; **Scan interval** under
**Advanced options** sets how often. **Rescan all** on the page looks at once.

To have a Synology notice new music within seconds as well, raise the watch limit. When it is too
low, the log says so and gives the line to run, with a number sized for your library; the end of the
log is on the Settings page. In DSM, **Control Panel**,
**Task Scheduler**, **Create**, **Triggered Task**, **User-defined script**, user `root`, event
**Boot-up**, and as the script:

    sysctl -w fs.inotify.max_user_watches=<the number the log gives>

Run it once by hand, then restart Kantele in Package Center.

## Where things are kept

The server never writes inside your music folder. Its settings, its index of your library and its
log are kept here:

| | Synology | macOS | Linux |
|---|---|---|---|
| Settings | `/var/packages/kantele/var/kantele.toml` | `~/Library/Application Support/Kantele/kantele.toml` | `/etc/kantele/kantele.toml` |
| Index | `/var/packages/kantele/var` | `~/Library/Application Support/Kantele` | `/var/lib/kantele` |
| Log | `/var/packages/kantele/var/kantele.log` | `~/Library/Application Support/Kantele/kantele.log` | `/var/lib/kantele/kantele.log` |

The end of the log is also on the Settings page. After a restart the server serves your library
from its index straight away, and only reads again the files that changed.
