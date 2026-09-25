# Your library on your player

What your player shows when you open Kantele, and how to get from there to an album.

## The first screen

| Entry | What it holds |
|---|---|
| **2481 albums** | Every album, in order of title |
| **30112 items** | Every track, as one list |
| **Genre** | The genres your tags name |
| **Artist** | The album artist, or the track artist where an album names none |
| **All Artists** | Everyone credited on a track, including the guests on a compilation |
| **Composer** | The composers your tags name |
| **Date** | Years |
| **Quality** | One word per file: Lossy, Below CD, CD, CD+, HD, HD+, DXD, DSD64 and up |
| **[untagged]** | Tracks no menu above can reach, because their tags are missing |
| **Playlists** | Your `.m3u`, `.m3u8` and `.pls` files |
| **Recently added** | The albums you added last, newest first |
| **[folder view]** | Your music folder as folders and files |

The two counts are your library's. An entry with nothing in it is not shown, so a library with no
playlists has no **Playlists**.

The menus from **Genre** to **Quality** are the default ones. You choose which appear, and in what
order, on the Settings page. Also available: **Work**, and the four figures behind **Quality**: **Bits**, **Channels**,
**Frequency** and **Type**.

On Denon and Marantz players using HEOS, the folder view is called **📁 Directories**, because HEOS
opens any first-screen entry with "folder" in its name on its own.

## How a menu narrows

Open **Genre**, then **Jazz**. If Jazz holds only a few albums, you see them. If it holds many, you
are offered the other menus instead, **Artist**, **Date**, **Quality**, and so on, each narrowing
Jazz further. Choose **Artist**, then **Miles Davis**, and you see his jazz albums.

A menu that would not narrow anything is left out. If every jazz track in your library is from the
same year, **Date** is not offered inside Jazz.

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
