# Troubleshooting

The page's **Library** tab is the folder tree, with how many tracks and albums each folder holds and
what is wrong with it. Every count opens onto the files behind it.

Start there, not in the log. The log is for when the page says nothing is wrong and something still
is.

## No device finds the server

Discovery is a multicast exchange, and it is what home networks break.

- **Is it running?** `curl -s localhost:8200/api/status` from the machine it runs on. Nothing
  answering means it is not running, and the log says why.
- **Is the device on the same network?** A guest network, a second access point, or a mesh node
  bridging in the wrong mode all separate the two. Wired and wireless on one router is fine.
- **Is multicast getting through?** Many routers have an IGMP snooping or "multicast filtering"
  setting that silently drops it. It is the most common cause.
- **macOS:** the firewall asks once whether to accept incoming connections, and refusing it produces
  exactly this.

The page's **Devices on the network** section lists every address the server has heard from and how
far each got: searching, reading its description, browsing, playing. A device that appears there and
still shows nothing is a different problem from one that never appears.

## One device lists some servers and not this one

Some amplifiers keep room for a fixed number of servers, and a house with a NAS, a streamer and a
phone can already be over it. One amplifier here stops at four. With four others announcing, this
server never reached its list, however many times it searched and got an answer, and it arrived
within seconds of one of the four going off.

The devices list tells you which case you are in. A device that searches, gets an answer every time
and never goes on to read the description has heard this server and had no room for it. Switch off a
server you do not use, or stop a second copy of this one.

Two copies of this server that share an identity are one device to every control point. The
identity lives in the saved index, so copying a state folder to start a second instance copies the
identity with it. Start the second one on an empty state folder and it mints one of its own.

## The menus are empty on the amplifier

- **On a Synology, the usual cause is permissions.** The server runs as the `kantele` package user,
  and if that user cannot read your music share, every folder is empty and nothing says so on the
  amplifier. The page does say so: the folder shows as unreadable. Grant that user read access in
  Control Panel.
- A check that is still running has not filled every menu yet. The progress strip says so.
- `nothing playable found` appears in the log when the folder holds no audio this server reads.

## An album is split in two, or filed under nobody

This is almost always what the tags say rather than what the server did, and the page tells you
which. It separates **problems**, which are failures, from **notes**, which are what your tags
decided.

Three are notes:

| On the page | What it means |
|---|---|
| Album artist not set | Every file credits a different artist and none is set for the album, so it files under no artist. Usual on a compilation tagged `Various Artists` |
| Album tags identical | Two folders carry the same album tags, so one of them is told apart by its path instead |
| Track tags identical | The same, for two tracks |

The rest are failures worth chasing:

| On the page | What it means |
|---|---|
| Unreadable | The file's tags would not read. Corrupt, or a format this server does not read |
| Folder unreadable | Permissions, usually |
| Not in the saved index | A row this build cannot use. It is read again on the next check |
| Playlist unreadable | The playlist file itself would not read |
| Playlist link broken | An entry naming a file that is not there. Moved or renamed since the playlist was written |
| Playlist link repeated | An entry naming a track the same playlist already named |
| Playlist empty | Every entry pointed at nothing, so the playlist is not offered. A menu entry opening onto nothing is worse than an absent one |

Under the tree, a line counts the tracks carrying each tag: how many have no artist, no date, no
genre, no cover art. Each is a button that narrows the tree to the folders holding those tracks, so
a hole in a menu on the amplifier leads back to the files that caused it.

## A track will not play, or will not seek

- Will not play at all: check it is not in the **Unreadable** list. A file whose tags will not read
  has no entry to play.
- Plays but will not seek: seeking is the renderer's business as much as the server's. The server
  accepts byte ranges and says so in the headers it sends. If your device seeks on another server
  and not on this one, that is worth an issue, with the device named.

## Searching from the amplifier finds nothing

Not every device sends a search this server can answer. A refused one is written to the log with
the query the device sent and the reason, so start there.

The one thing refused on purpose is a query that mixes `and` and `or` at the same level without
brackets. The UPnP grammar gives the two no order, so `a and b or c` could mean either grouping,
and answering the wrong one would quietly return the wrong tracks. Bracketed queries are answered,
including the usual one that asks for a word in the title or the artist.

## Reading the log

| | Where |
|---|---|
| Synology | `/var/packages/kantele/var/kantele.log` |
| macOS | `~/Library/Logs/kantele.log` |

Every request is traced with the user agent that sent it, so the log answers "what did that device
actually ask for, and what did it get". Anything refused says why. Reading it needs the colour codes
stripped, or a search for `status=` comes back empty.

For more than the log says:

    curl -s localhost:8200/api/status

which answers in plain `key = value` lines for a shell with no `jq`, and in JSON if you ask with
`Accept: application/json`.

## Reporting a device

Open an issue with the device, what it did and did not do, and what the log said at the time. A
device nobody here owns can only be tested by whoever has one.
