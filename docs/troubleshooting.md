# Troubleshooting

Start with the **Library** tab of the page. It shows every folder with its albums and tracks, and
says what is wrong with the ones that have a problem. The log is for when the page says nothing is
wrong and something still is.

## Your player does not list the server

Players find servers by a broadcast on the local network, and home networks sometimes block it.

- **Is it running?** Open the page. If it does not load, the server is not running: on a Synology,
  look in Package Center; elsewhere, read [the log](#the-log).
- **Are both on the same network?** A guest network, or a mesh or second access point in the wrong
  mode, keeps them apart. Wired and wireless on one router is fine.
- **Does the router let the broadcast through?** A setting called *IGMP snooping* or *multicast
  filtering* on a router or switch can drop it. This is the most common cause.
- **On a Mac**, macOS asks once whether Kantele may accept incoming connections. If you said no,
  allow it in **System Settings**, **Network**, **Firewall**, **Options**.
- **On a Mac, the other way round**: an app on the Mac that plays from media servers needs
  permission to reach the local network, in **System Settings**, **Privacy & Security**,
  **Local Network**.

To see which devices have talked to the server and how far each got, run this on the machine
Kantele runs on (on a Synology, over SSH):

    curl -s localhost:8200/api/status

Each device is listed with a sentence such as *read the description and never came back* or
*listening for changes, and not browsing yet*. A player listed there has found the server, and the
problem is elsewhere.

**A player that finds the server one minute and not the next** is usually losing the broadcast
over Wi-Fi. Players that look again regularly get there; some look only when they start.

**Two copies of Kantele that show as one.** If you copied Kantele's index folder to start a second
copy, both copies carry the same identity and your player sees only one of them. Start the second
copy with an empty index folder.

## The menus are empty

- **On a Synology, it is almost always the permissions.** Kantele runs as its own user, `kantele`,
  which must be allowed to read your music share. The Library tab then shows the folder as
  **Unreadable folder**. [Getting started](getting-started.md#synology) says how to allow it.
- **The first reading is still going on.** The strip at the top of the page shows how far it has
  got, and the menus fill as it goes.
- **The folder holds nothing Kantele plays.** The log says `nothing playable found`.
- **The music is behind a link that leaves the music folder.** The Library tab says
  *Link out of the music folder*. Kantele serves only the folders you chose; add the other place as
  a second music folder.

## An album is split in two, or listed under nobody

This almost always comes from the tags, and the Library tab shows which. It separates **problems**,
which are failures, from **notes**, which are what your tags decided.

Two are notes:

| The page says | What it means |
|---|---|
| Identical album tags | Two folders carry the same album tags, so they are kept as two albums |
| Identical track tags | The same, for two tracks |

The rest are problems:

| The page says | What it means |
|---|---|
| Unreadable file | The file's tags would not read: the file is damaged, or in a format Kantele does not read |
| Unreadable folder | Kantele is not allowed to open the folder, usually |
| Not in the saved index | Kantele will read this file again at the next check |
| Unreadable playlist | The playlist file itself would not read |
| Broken playlist link | A playlist names a file that is not there, usually one moved or renamed since |
| Repeated playlist link | A playlist names the same track twice |
| Empty playlist | None of the playlist's files are there, so the playlist is not shown |

Under the folders, a line counts the tracks with no artist, no date, no genre or no cover. Each
count opens onto the folders that hold those tracks. [Tagging your music](tagging.md) says how
albums, discs and artists are formed.

## Finding what to fix in your tags

Tick **Something to fix** above the folders on the Library tab. Every column then keeps only the
folders that hold a problem, or an album one of these checks found wanting, so you can follow them
down to the album. Under the album, the page says what it found:

| The page says | What to do in your tagger |
|---|---|
| Tracks 7 and 9 of 12 are missing | Find the missing files, or correct the track total |
| Same title and artist as the album in another folder | Keep one copy, or tell the two apart in the album title. **Show both** lists the two folders |
| No album artist and not marked as a compilation | Set `ALBUMARTIST`, or `COMPILATION` to 1 ([compilations](tagging.md#artists)) |
| The cover is 300 × 300 | Replace it with a larger one ([covers](tagging.md#covers)) |

A folder holding one or two tracks of a longer album is not reported as incomplete: that is taken
for a choice. Neither is an album whose files give different track totals, or no total at all.

## A track will not play, or will not seek

- **It will not play at all.** Check that it is not listed as **Unreadable file**.
- **It plays but will not seek.** Seeking is up to the player as much as the server, and some
  players cannot seek on any media server. If yours seeks on another server and not on Kantele,
  [report it](players.md#report-your-player).
- **A whole CD in one file** shows as one long track. Kantele does not read cue sheets.

## Searching from the player finds nothing

A search your player sends and Kantele cannot answer is written to the log, with what the player
asked for and why it was refused. Look there first.

Kantele refuses one kind of search on purpose: one mixing `and` and `or` without brackets, such as
`a and b or c`, which can be read two ways. The searches players usually send are bracketed and are
answered.

## The log

| | Where |
|---|---|
| Synology | `/var/packages/kantele/var/kantele.log` |
| macOS | `~/Library/Application Support/Kantele/kantele.log` |
| Linux | `/var/lib/kantele/kantele.log`, and `journalctl -u kantele` |

The end of the log is also on the Settings page. Every request from a player is in it, with the name
the player gave, so it shows what the player asked for and what it got. Anything refused says why.

## Still stuck

[Open an issue](https://github.com/gaelsimon/kantele/issues) with your player's make and model, what
it did and did not do, and the lines of the log from that moment.
