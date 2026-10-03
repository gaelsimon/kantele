# Coming from MinimServer

Kantele browses much like MinimServer, and reads a library tagged for MinimServer without
retagging. This page says what carries over, what differs, and what Kantele does not do yet.

## Trying it beside MinimServer

Both can run on the same NAS, on the same music folder. Kantele never writes inside the music
folder. Give it a different **Server name** so your player lists both, and compare menu by menu.

## Where things are on the first screen

| In MinimServer | In Kantele |
|---|---|
| `N albums` | **Albums** |
| `N items` | **Tracks** |
| `N playlists` | **Playlists** |
| `Artist` | **Artists** |
| `All Artists` | **Track artists** |
| `Genre` | **Genres** |
| `Composer` | **Composers** |
| `Date` | **Years** |
| `Artist` → `Various Artists` | **Compilations** |
| `[untagged]` | **Untagged tracks** |
| `[folder view]` | **Folders**, or **📁 Directories** on HEOS |

Kantele also lists **Audio quality**, and **Recently added** unless you turn it off. The first
screen shows names without counts, most used first.

## What is the same

- **Menus that narrow.** Choosing a genre offers the other menus that split it, until few enough
  albums are left to list them. Those menus start with the genre's albums and items, as
  MinimServer's do.
- **Multi-disc albums.** A disc number alone plays the discs as one list; a disc subtitle, or a
  disc marker in the album title, opens the album disc by disc. An album that showed one way in
  MinimServer shows the same way here.
- **`GROUPING`** joins consecutive tracks into one entry inside the album.
- **Composers written with a slash**, `Lennon/McCartney`, are listed one by one under
  **Composers**. An artist written that way stays one name, so `AC/DC` is not split.
- **Playlists**: `.m3u`, `.m3u8` and `.pls`, one entry per file, in the file's order.
- **Covers**: the picture in the file wins, and the folder's image fills in where a file has none.
- **Audio quality**: the same words, from Lossy through CD and HD to DSD.
- **Recently added**, built from the newest few hundred files.

## Settings you may be looking for

| In MinimServer | In Kantele |
|---|---|
| `contentDir` | **Music folders** |
| `indexTags` | **Menu sequence**, among the menus Kantele offers |
| `listViewAlbums` | **Albums shown in a list** |
| `alphaGroup` | **A to Z index** |
| `excludePattern` | **Exclusions** |
| the `ignore.sort` option | **Words to ignore in the sort order** |

All of them are on the Settings page, and none of them needs a restart.

## What is different

- **Albums follow folders.** MinimServer joins tracks from different folders when their album and
  artist tags match. Kantele keeps one album per folder and joins folders only when they are discs
  of one album (see [tagging](tagging.md#albums)). Two recordings of the same work, tagged alike in
  two folders, stay two albums.
- **A long list keeps all its entries.** MinimServer's `alphaGroup` replaces a long list with
  letters, and **Show All** takes two steps to get back. Kantele keeps the whole list and adds an
  **A-Z** entry at its top.
- **Accented names sort with their letter.** `Šerkšnytė` is under S, where MinimServer puts names
  with letters beyond Latin-1 after Z.
- **Years, not dates.** MinimServer's `Date` lists each date as written, so `2018` and
  `2018-06-29` are two entries. **Years** lists the year.
- **Spellings that differ only in capitals are one entry.** MinimServer lists `ABBA` and `Abba`
  apart.
- **No unknown entry in each menu.** A track with no genre is not under an unknown genre. It is
  reached through its other tags, and under **Untagged tracks** when no menu reaches it.
- **Recently added** uses the date Kantele first saw a file, so retagging an album does not bring
  it back to the top.
- **`Various Artists`** and its variants are not listed as an artist. Their albums are under
  **Compilations** on the first screen.
- **Setup** is a page with a folder picker. There is no Java to install, and nothing to buy.

## What Kantele does not do yet

- **Custom menus.** The menus are the ones listed in [settings](settings.md#menu-on-your-players).
  There is no **Conductor** or **Orchestra** menu yet, and no way to index a tag of your own.
- **Rewriting tags for display**: nothing like `tagValue`, `tagFormat`, `displayFormat` or
  `aliasTags`.
- **Name reversal**, such as showing `Bach, Johann Sebastian` as `Johann Sebastian Bach`. A sort tag
  sorts a name, but it is shown as written.
- **Converting formats.** There is no MinimStreamer; files are sent as they are.

If one of these keeps you on MinimServer, [open an
issue](https://github.com/gaelsimon/kantele/issues) and say which.
