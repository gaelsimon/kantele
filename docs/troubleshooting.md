# Troubleshooting

The page's **Library** tab is the folder tree, with how many tracks and albums each folder holds and
what is wrong with it. Every count opens onto the files behind it.

Start there, before the log. Read the log when the page says nothing is wrong and something still
is.

## No device finds the server

Discovery is a multicast exchange, and home networks break it.

- **Is it running?** Run `curl -s localhost:8200/api/status` on the machine it runs on. If nothing
  answers, it is not running, and the log says why.
- **Is the device on the same network?** A guest network, a second access point, or a mesh node in
  the wrong bridge mode all separate the two. Wired and wireless on one router is fine.
- **Does multicast get through?** Many routers have an IGMP snooping or "multicast filtering"
  setting that silently drops it. It is the most common cause.
- **macOS:** the firewall asks once whether to accept incoming connections. If you refuse, you get
  exactly this.
- **macOS, on the other side:** the control point application needs its own permission to reach the
  local network. Until you grant it, the application acts as if the server is not there. It starts
  to work as soon as you grant it.

The page's **Devices on the network** section lists every address the server has heard from and how
far each got: searching, reading its description, browsing, playing. A device that appears there and
still shows nothing is a different problem from one that never appears.

## One device lists some servers and not this one

The ordinary cause is the network. A device that misses a server one minute and finds it the next
is losing the multicast, which a wireless one does often. A device that searches again and again
gets there in the end; one that searches once may not.

Past that, some amplifiers keep room for a fixed number of servers, and a house with a NAS, a
streamer and a phone can already be over it. One amplifier here once stopped at four: with four
other servers on the network, this server never reached its list, however many times it searched
and got an answer, and it arrived within seconds after one of the four went off. That happened
once, on one device. The same house has run more servers since and no list has overflowed, so read
the number as that device's rather than as a rule.

The devices list tells you which case you are in. A device may search, get an answer every time, and
never read the description. That device has heard this server and had no room for it. Switch off a
server you do not use, or stop a second copy of this one.

Two copies of this server that share an identity are one device to every control point. The
identity lives in the saved index. If you copy a state folder to start a second instance, you copy
the identity with it. Start the second one on an empty state folder, and it makes one of its own.

## The menus are empty on your player

- **On a Synology, the usual cause is permissions.** The server runs as the `kantele` package user.
  If that user cannot read your music share, every folder is empty, and the amplifier does not say
  why. The page does say why: the folder shows as **Unreadable folder**. Grant that user read access
  in Control Panel, Shared Folder, Edit, Permissions. Set the list to **System internal user**,
  which holds the package accounts and not the people.
- A check that still runs has not filled every menu yet. The progress strip says so.
- `nothing playable found` appears in the log when the folder holds no audio this server reads.
- **The server leaves out music reached through a symbolic link that leaves the music folder.** The
  page says so as *Link out of the music folder*. Every device on the network can play anything this
  server serves, so the server keeps to the folders you named. To serve a second disk, add it as
  another music folder in Settings.

## An album is split in two, or filed under nobody

Almost always, this is what the tags say and not what the server did. The page tells you which. It
separates **problems**, which are failures, from **notes**, which are what your tags decided.

Two are notes:

| On the page | What it means |
|---|---|
| Identical album tags | Two folders carry the same album tags, so the server tells one of them apart by its path instead |
| Identical track tags | The same, for two tracks |

The rest are failures to look into:

| On the page | What it means |
|---|---|
| Unreadable file | The file's tags would not read. The file is corrupt, or its format is one this server does not read |
| Unreadable folder | Permissions, usually |
| Not in the saved index | A row this build cannot use. The server reads it again on the next check |
| Unreadable playlist | The playlist file itself would not read |
| Broken playlist link | An entry that names a file that is not there. The file moved or changed its name after somebody wrote the playlist |
| Repeated playlist link | An entry that names a track the same playlist already names |
| Empty playlist | Every entry pointed at nothing, so the server does not offer the playlist. A menu entry that opens onto nothing is worse than no entry at all |

Under the tree, a line counts the tracks that carry each tag: how many have no artist, no date, no
genre, no cover art. Each count is a button. It narrows the tree to the folders that hold those
tracks, so a hole in a menu on a player leads back to the files behind it.

## A track will not play, or will not seek

- Will not play at all: check it is not in the **Unreadable file** list. A file whose tags will not
  read has no entry to play.
- Plays but will not seek: the renderer decides this as much as the server does. The server accepts
  byte ranges and says so in the headers it sends. If your device seeks on another server and not on
  this one, open an issue and name the device.

## Searching from the amplifier finds nothing

Not every device sends a search this server can answer. The server writes a refused search to the
log with the query the device sent and the reason. Start there.

The server refuses one query on purpose: a query that mixes `and` and `or` at the same level without
brackets. The UPnP grammar gives the two no order, so `a and b or c` has two possible meanings. An
answer to the wrong one returns the wrong tracks without a word. The server answers a query with
brackets, including the usual one that asks for a word in the title or the artist.

## Reading the log

| | Where |
|---|---|
| Synology | `/var/packages/kantele/var/kantele.log` |
| macOS | `~/Library/Logs/kantele.log` |

The log traces every request with the user agent that sent it, so it answers "what did that device
actually ask for, and what did it get". Anything refused says why. Strip the colour codes before you
read it, or a search for `status=` comes back empty.

For more than the log says:

    curl -s localhost:8200/api/status

which answers in plain `key = value` lines for a shell with no `jq`, and in JSON if you ask with
`Accept: application/json`.

## Reporting a device

Open an issue with the device, what it did and did not do, and what the log said at the time. Only
somebody who has a device can test it, and nobody here owns them all.
