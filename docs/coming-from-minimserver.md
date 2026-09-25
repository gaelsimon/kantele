# Coming from MinimServer

Kantele browses much like MinimServer, and reads a library tagged for MinimServer without
retagging. This page says what carries over, what differs, and what Kantele does not do yet.

## Trying it beside MinimServer

Both can run on the same NAS, on the same music folder. Kantele never writes inside the music
folder. Give it a different **Server name** so your player lists both, and compare menu by menu.

## What is the same

- **Menus that narrow.** Choosing a genre offers the other menus that split it, until few enough
  albums are left to list them.
- **Multi-disc albums.** A disc number alone plays the discs as one list; a disc subtitle, or a
  disc marker in the album title, opens the album disc by disc. An album that showed one way in
  MinimServer shows the same way here.
- **`GROUPING`** joins consecutive tracks into one entry inside the album.
- **Playlists**: `.m3u`, `.m3u8` and `.pls`, one entry per file, in the file's order.
- **Covers**: the picture in the file wins, and the folder's image fills in where a file has none.
- **Quality**: the same words, from Lossy through CD and HD to DSD.
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
- **Recently added** uses the date Kantele first saw a file, so retagging an album does not bring
  it back to the top.
- **`Various Artists`** and its variants are not listed as an artist.
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
