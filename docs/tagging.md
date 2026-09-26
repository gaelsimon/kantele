# Tagging your music

Kantele builds its menus from the tags in your files and from how your folders are arranged. This
page says which tags it reads and what each one does. Any tagger works; the tag names below are the
ones MusicBrainz Picard, Mp3tag and foobar2000 write.

When something looks wrong on your player, the **Library** tab of the page shows, for every file,
the tags it carries and where they put it.

## The tags it reads

| Tag | What it does |
|---|---|
| `TITLE` | The track title. With none, the file name is used |
| `ARTIST`, `ARTISTS` | The track's performers. **All Artists** lists them |
| `ALBUMARTIST`, `ALBUMARTISTS` | Who the album is by. **Artist** lists it |
| `ALBUM` | The album title |
| `TRACKNUMBER`, `DISCNUMBER`, `DISCTOTAL` | The order of the tracks, and which disc they are on |
| `DISCSUBTITLE` | The name of a disc, which also shows the album disc by disc |
| `DATE` or `YEAR` | The **Date** menu, which keeps the year only |
| `GENRE` | The **Genre** menu |
| `COMPOSER` | The **Composer** menu |
| `WORK` | The **Work** menu, when you turn it on |
| `GROUPING` | Joins consecutive tracks into one entry, such as the movements of a symphony |
| `COMPILATION` | Marks an album by various artists |
| `ARTISTSORT`, `ALBUMARTISTSORT`, `COMPOSERSORT` | How a name is sorted, such as `Bach, Johann Sebastian` |
| `MUSICBRAINZ_ALBUMID`, `MUSICBRAINZ_RELEASETRACKID` | Keep an album and its tracks recognised when you retag or move them |
| An embedded picture | The cover, when the folder holds no image |

The format decides where these tags live: Vorbis comments in FLAC, Ogg and Opus; ID3 in MP3, WAV,
AIFF, DSF and DFF; MP4 tags in M4A; APE tags in Monkey's Audio and WavPack. Your tagger takes care
of that.

## Albums

**One folder is one album.** The tracks of a folder that share an album title are one album, even
if their dates or artists differ from track to track. Two albums in one folder, with different album
titles, stay two albums.

**Two folders are one album only when something says so.** For example:

    Pink Floyd - The Wall/CD1/…
    Pink Floyd - The Wall/CD2/…

Folders whose name holds a disc number, such as `CD1`, `Disc 2`, `Disque 1` or `The Wall (CD 2)`,
are joined with the other discs beside them into one album. So are folders side by side whose tracks
share an album title and say the album has more than one disc (`DISCTOTAL` of 2 or more). A disc
marker at the end of the album title, such as `The Wall [Disc 2]`, is taken off the title, so both
discs share one title.

**Two folders with the same album tags are two albums.** Two editions of one record, in two
folders, stay apart. The Library tab notes them as *Identical album tags*.

## Several discs

How a multi-disc album shows depends on what you tagged:

- With `DISCNUMBER` only, the album is one list, disc 1 then disc 2.
- With `DISCSUBTITLE`, or a disc marker in the album title, the album opens onto one entry per disc,
  named by its subtitle or `Disc 2`. Each disc keeps its own track numbers.

## Artists

**Artist** shows who the album is by: `ALBUMARTIST` where it is set, and the track artist where it
is not.

**All Artists** shows everyone who plays on a track, so a guest on a compilation can be found by
name.

**A compilation** is an album by nobody in particular. Set `COMPILATION` to 1, or `ALBUMARTIST` to
`Various Artists`. The album is then not listed under any one artist in **Artist**, and each track
is still found under its performer in **All Artists**. `Various`, `VA`, `V/A`, `Unknown Artist` and
`None` are treated the same way as `Various Artists`. **Unmarked compilations** on the Library tab
finds the albums by many artists that carry neither.

## Several values in one tag

Write two artists on one track as two values of `ARTIST`. Kantele never splits a value on a
separator, because `Simon & Garfunkel` is one act and `Barbra Streisand & Barry Gibb` is two,
and nothing in the text says which is which.

How you enter several values depends on the tagger: foobar2000 splits a field on `;`, and Mp3tag
on `\\`. MusicBrainz Picard writes an `ARTISTS` tag with one value per person, which Kantele prefers
over `ARTIST` where both are there.

MP3 files tagged in ID3v2.3 cannot hold several values. Save them as ID3v2.4 to have them read as
separate artists.

## Sorting

Names are sorted as they are written, skipping a leading `The`, so The Beatles are under B. To
sort `Johann Sebastian Bach` under B, give the file `ARTISTSORT` or `COMPOSERSORT` as
`Bach, Johann Sebastian`. The sort tag decides both the order and the letter in the **A-Z** index.
When several files disagree on how one name sorts, the spelling most of them use wins.

## Dates

Any date works: `1963`, `1963-04-12` or `12/04/1963`. The **Date** menu keeps the year.

## Covers

Kantele uses the picture embedded in the file, as MinimServer does. For a file with none, it looks
in the file's folder for an image called `cover`, `folder`, `front` or `album`, in JPEG or PNG. With
none of those, it takes an image whose name says it is a front cover, never one named `back`,
`booklet`, `cd` and the like, or else the only image in the folder.

**Preferred cover** on the Settings page can make the folder's image win instead, for a library
whose folder images are the better ones. Even then, a folder holding tracks of several albums, such
as a selection you made, is not an album: there each file keeps the picture it carries, and the
folder's image is used only for the files that have none. Changing the setting reads the library
again.

## Tracks with no tags

A track with no tags at all is under **[untagged]**, titled by its file name, and in the folder
view. On the Library tab, the **Tags** menu keeps the folders holding tracks with no artist, no
album, no genre, no cover or no date.
