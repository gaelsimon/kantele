# Settings

The Settings tab of the page, section by section. Each setting carries a tag saying when a change
takes effect:

| Tag | What happens |
|---|---|
| applies at once | Your player sees the change the next time it opens a menu |
| next check | It takes effect the next time the server looks at your music |
| reads the library again | Every file is read again, which takes as long as the first time |
| needs a restart | Restart Kantele: in Package Center on a Synology, or [as below](#restarting-outside-a-synology) |
| read only | Change it in `kantele.toml` |

A greyed setting is held by the command line or the environment, which win over the page. The page
says which.

## Your music

**Music folders.** The folders your music is in. Several folders become one library, and the folder
view opens onto one entry per folder. Two folders with the same name, or one inside another, are
refused.

**Exclusions.** Folders and files to leave out, such as `Podcasts` or `*.iso`. A name without a `/`
is matched against every file and folder name; a path with a `/` is matched from the top of your
music folder. `*` stands for any text within a name, `?` for one character, and case does not
matter. Everything inside an excluded folder is left out too.

## Menu on your players

**Server name.** The name your player lists the server under.

**Menu sequence.** Which menus the first screen offers, and in what order. The choices are Genre,
Artist, All Artists, Composer, Work, Date, Quality, Bits, Channels, Frequency and Type.
[Your library on your player](on-your-player.md) says what each holds.

**Recently added.** How many of the newest files **Recently added** is built from. Zero removes it.

**Albums shown in a list.** When you narrow a menu down to this many albums or fewer, you see the
albums instead of more menus.

**A to Z index.** A list at least this long starts with an **A-Z** entry for jumping to a letter.
Zero turns it off.

## Advanced options

**Scan interval.** How often, in minutes, the server looks for music you added, changed or removed.
Only the folders that changed are read. Zero turns it off, and new music then appears only when you
press **Rescan all**. See [getting started](getting-started.md#new-music) for why a Synology relies
on it.

**Files read at the same time.** How many files the server reads at once. On a NAS with spinning
disks, more is slower, because the disk spends its time seeking.

**Preferred cover.** Whether the image in the folder or the picture embedded in the file wins when
an album has both. See [covers](tagging.md#covers).

**Words to ignore in the sort order.** Leading words skipped when sorting a name, such as `The`,
`Les` or `L'`. A sort tag in the file wins over this list.

**Port.** The port for the music and the page. On a Synology the DSM icon keeps pointing at the old
one.

**Icon.** A PNG or JPEG your player shows beside the server's name.

**Capture folder.** For reporting a player that misbehaves: every exchange with each player is
written to this folder. Leave it off otherwise.

**Index folder.** Where the server keeps what it has read from your files. Never inside your music
folder. Two libraries on one machine need one index folder each.

**Device profiles.** Adjustments for a player that needs them, set in `kantele.toml`.
`kantele.example.toml` has an example. One is built in, for Denon and Marantz players using HEOS.

**Log level.** How much the log says. Set it with the `RUST_LOG` environment variable:
`kantele=debug` for more detail, `kantele=info` by default.

## Restarting outside a Synology

On a Mac:

    launchctl kickstart -k gui/$(id -u)/com.kantele.server

On Linux:

    sudo systemctl restart kantele

## The settings file

Every setting lives in one file, `kantele.toml`, which the page writes. [Getting
started](getting-started.md#where-things-are-kept) says where it is. `kantele.example.toml` lists
every setting with its default. A setting the server does not recognise stops it at startup, with a
message in the log naming it, so a typo cannot go unnoticed.

The command line and the environment win over the file; `kantele --help` lists both.

## The page has no password

Anyone on your home network can open the page, as anyone on it can already play your music. The page
refuses any request that changes a setting, starts a rescan or lists your disks when it comes from
outside your local network. Do not forward the port to the internet anyway.
