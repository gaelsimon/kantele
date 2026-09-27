# Your library on your player

What your player shows when you open Kantele, and how to get from there to an album.

## The first screen

| Entry | What it holds |
|---|---|
| **Albums** | Every album, in order of title |
| **Artists** | The album artist, or the track artist where an album names none |
| **Genres** | The genres your tags name |
| **Composers** | The composers your tags name |
| **Years** | Years |
| **Track artists** | Everyone credited on a track, including the guests on a compilation |
| **Audio quality** | One word per file: Lossy, Below CD, CD, CD+, HD, HD+, DXD, DSD64 and up |
| **Compilations** | The albums by nobody in particular, which no entry in **Artists** stands for |
| **Recently added** | The albums you added last, newest first |
| **Playlists** | Your `.m3u`, `.m3u8` and `.pls` files |
| **Folders** | Your music folder as folders and files |
| **Tracks** | Every track, as one list |
| **Untagged tracks** | Tracks no menu above can reach, because their tags are missing |

An entry with nothing in it is not shown, so a library with no playlists has no **Playlists**.

The menus from **Artists** to **Audio quality** are the default ones. You choose which appear, and
in what order, on the Settings page. Also available: **Works**, and the four figures behind
**Audio quality**: **Bit depths**, **Channels**, **Sample rates** and **Formats**.

On Denon and Marantz players using HEOS, **Folders** is called **📁 Directories**, because HEOS
opens any first-screen entry with "folder" in its name on its own.

## How a menu narrows

Open **Genres**, then **Jazz**. If Jazz holds only a few albums, you see them. If it holds many, you
are offered the other menus instead, **Artists**, **Years**, **Audio quality**, and so on, each narrowing
Jazz further. Choose **Artists**, then **Miles Davis**, and you see his jazz albums.

Those menus are headed by **All albums** and **All tracks**, every album and every track of Jazz
as two lists, so the whole of a genre can be played or shuffled from there.

A menu that would not narrow anything is left out. If every jazz track in your library is from the
same year, **Years** is not offered inside Jazz.

**Albums shown in a list** on the Settings page decides when the albums are shown instead of more
menus.

## Long lists

A long list, such as every artist in a large library, is one list your player scrolls through. Its
first entry is **A-Z**, which opens onto one entry per letter, so you can jump to M. Names starting
with a digit or a sign are under **#**. Names are sorted and lettered the same way, so an artist
sorted under B is also under B in the index.

`The` is skipped when sorting, so The Beatles are under B. **Words to ignore in the sort order**
on the Settings page holds that list. A sort tag in the file wins over it; see
[tagging](tagging.md).

## Albums

An album opens onto its tracks, in disc and track order.

An album with several discs either plays as one list or opens onto one entry per disc, depending
on how it is tagged. [Tagging your music](tagging.md#several-discs) explains which.

Tracks that share a `GROUPING` tag and follow each other on the disc, such as the movements of a
symphony or the acts of an opera, show as one entry inside the album, which opens onto them.

## Recently added

The albums the newest files belong to, newest first. The date is when Kantele first saw the file,
so retagging an album does not bring it back to the top. **Recently added** on the
Settings page sets how far back it reaches.

## Playlists

Every `.m3u`, `.m3u8` and `.pls` file in your music folder, playing in the order the file lists
them. An entry that names a file that is not in the library is left out. Two playlists with the same
name are told apart by the folder they are in.

## Folder view

Your music folder as it is on disk. Useful when the tags of a folder are poor, or to
reach music the menus do not.

## Searching

The search box of your player finds tracks, albums and artists by title, artist, album, composer
and genre. Accents and case do not matter: `elegie` finds `Élégie`. Not every player has a search
box for media servers.

## What is not there

- A track whose file could not be read. The Library tab of the page lists them, folder by folder.
- A file in a format Kantele does not read, such as a video.
- A folder or file matched by **Exclusions** on the Settings page.
- A folder that is a link to somewhere outside your music folders. Add that place as another music
  folder instead.
