# Settings

Everything is one TOML file, and `kantele.example.toml` is the reference: every key, with its
default, and a paragraph saying what it does. The page edits the ones a page can edit.

What is here is what the settings mean to a listener, in the blocks the page groups them into and
in the page's order. What each one costs to change is written beside it there, and there are five
answers:

| It says | It means |
|---|---|
| applies at once | Saved and in force |
| applies at the next check | In force once the server next looks at the library |
| reads the library again | Every file is read again, which takes as long as the first run did |
| needs a restart | Stop and start the server |
| not written from this page | Set somewhere the page cannot reach; the line under the value says where |

A setting the environment or the command line holds is shown greyed, with the layer that holds it
named, because that layer outranks the file and a write would not take.

## Folders

**Music folder:** what is served. Several folders are a list, and they become one library told apart
by their names.

**Left out of the library:** names and paths never walked. A pattern with no slash is held against
every file and folder name, one with a slash against the path under the music folder. `*` stands for
anything within one name, `?` for one character, and case does not matter. A folder that matches is
not entered, so nothing under it is either. Useful for a `Podcasts` folder, or for `*.iso`.

## Scanning

**Files read at once:** beyond a handful, an ARM NAS saturates its disk and goes slower. Four is the
default for that reason.

**Checks for changes:** minutes between looks for music nothing announced. Zero turns it off, and
then new music appears only when you ask. See [the first run](first-run.md) for why a Synology
depends on this.

## Network

**Server name:** what control points display.

**Port:** content, control and the page, all on one port. Changing it on a Synology also means the
DSM icon points at the old one.

**Icon:** what a control point shows beside the server's name.

**Index folder:** where the saved index lives. Never inside the music folder: a share may be
read-only, and a database beside the music writes to the array on every check. One index serves one
library, so two libraries on one machine want one each, or every start of either reads every file
again.

**Capture folder:** off unless you set it. With it set, every control exchange is written verbatim,
one folder per device. It is how you find out what your amplifier asks for, and it costs a
disk write per request, so it is not for a server in daily service.

## Menus

**Show albums directly up to:** how many albums a selection may hold before the menus stop asking
you to narrow it further and show them.

**A-Z index on lists of at least:** long lists carry an `A-Z` entry as their first child, opening
onto one container per letter. The flat list stays exactly as it was, so nothing is taken away.
Everything not starting with a letter is under `#`.

**Recently added reaches back over:** how many of the newest files the `Recently added` menu is
built from. It lists the albums those files belong to, newest first, and the loose files among them.
The date is when this server first saw the file, so retagging moves nothing and a library has no
history from before its first run. Zero removes the menu.

**Menus, in order:** which menus the root offers and in what order. The order here is the order on
the amplifier. The choices are Genre, Artist, All Artists, Composer, Work, Date, Quality, Bits,
Channels, Frequency and Type, and `kantele.example.toml` shows which of them start on. *Artist* is
the album artist where a file carries one, *All Artists* is everyone credited on the track, and
*Quality* sums a file up in one word from Lossy through CD, HD, DXD and DSD64.

## Names

**Ignored when sorting:** leading words a sort should skip, so `The Beatles` files under B. It
applies where a file carries no sort tag of its own. Whole words only, or an apostrophe-final one
like `L'`.

## Cover art

**Cover art:** which wins when a track has both an image beside it in the folder and one inside the
file. Whichever you do not prefer still serves where the other is missing. This one reads the
library again, because the cover is chosen while a file is read.

## In the file

**Device profiles:** per-device overrides, matched on what the device calls itself. They can hold a
different MIME type or DLNA profile for one format, join repeated values into one string, and cap a
stream to a multiple of the rate the file plays at, for a renderer that chokes on a faster one.
`kantele.example.toml` has a commented example.

**Log level:** set with the `RUST_LOG` environment variable, not in the file. `kantele=debug` is
what makes eventing and per-device decisions visible; `kantele=info` is the default.

## There is no password

The page reads and writes with no credentials. A write changes a menu setting or starts a check of
the library, and every device on your network can already browse and stream all of it, so the
firewall is what protects the port. Do not forward it to the internet.
