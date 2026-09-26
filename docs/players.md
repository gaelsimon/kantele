# Players

Kantele works with any UPnP/DLNA player or control app. Some players need a quirk handled, and
Kantele holds those as device profiles; see [settings](settings.md#advanced-options).

A player is often two things: the app you browse with (the *control point*) and the device that
plays (the *renderer*). These are the pairs in daily use by the author. Each of them finds the
server, browses it, searches it and plays from it.

| App you browse with | Plays on |
|---|---|
| Yamaha MusicCast | Yamaha TSX-N237D |
| Denon HEOS (Android) | Marantz Model 40n |
| [Neos](https://github.com/gaelsimon/neos-audio) | Marantz Model 40n |
| BubbleUPnP (Android) | the Android device itself |

**Denon and Marantz with HEOS.** A profile is built in. HEOS opens on its own any first-screen
entry with "folder" in its name, so for HEOS the folder view is called **📁 Directories** and sits
at the end of the list.

## Report your player

If you use Kantele with a player not listed here, whether it works or not,
[open an issue](https://github.com/gaelsimon/kantele/issues) saying:

- the app and the device, with their model,
- which of these worked: finding the server, browsing, searching, playing, seeking,
- for what did not, the lines of [the log](troubleshooting.md#the-log) from that moment.

To help with a player that misbehaves, set **Capture folder** under **Advanced options** on the
Settings page and restart Kantele. It then writes every exchange with each player to that folder,
which shows exactly what the player asked for. Attach the player's folder to the issue, and turn the
capture off again afterwards.
