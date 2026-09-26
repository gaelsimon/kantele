# Contributing

## Building

    cargo build --release

A stable Rust toolchain is all it needs. The configuration page is committed as a built file, so a
clone builds without node. To work on the page:

    cd web && npm ci && npm run dev

`npm run build` writes `assets/web/index.html`, which is committed. CI refuses a commit where that
file is not what `web/` builds.

Three rules hold the page together. Breaking one of them changes what the page claims about the
server, which is a heavier change than it looks.

- The rail beside a setting says where its value came from, read off `source.layer`, so a page read
  from a metre away shows which decisions are the owner's.
- Failure is loud. The block listing what was skipped appears only while something was, and the
  all-clear line waits for `refused.walked`, so a server that has not checked anything yet never
  claims a clean library.
- The hero carries the claim the server is making: serving, indexing, or serving with something
  missing.

Every sentence the page shows about a setting comes from the server. `api::describe::says` is the
only copy of the five modes, and it sits in the API because the words a reader sees are not the
core's business; the page renders `setting.says` and keeps none of its own.

`tools/package-spk.sh` builds the Synology package, one file carrying both architectures and
declared `noarch`, because a package source serves one catalogue to every model. It wants
`cargo-zigbuild` for the musl targets and ImageMagick to render the DSM icons from
`assets/icon.svg`, and it runs on any platform. `tools/package-index.py` writes the catalogue that
package source serves, reading the package it offers rather than being told what is in it.

## Testing

    cargo test
    cd web && npm test

These gates stand between a change and a device that stops working.

`tests/wire_snapshots.rs` freezes every document that goes on the wire. A reordered element or a
dropped attribute compiles and passes everything else.

    INSTA_UPDATE=always cargo test --test wire_snapshots

Read the diff before accepting it. Accepting one unread is how a wire change ships unnoticed.

`tests/the_wire_answers_a_client.rs` drives the router: status codes, SOAP faults, byte ranges,
the DLNA headers a renderer asks for, and the eventing some devices will not browse without.
Players read a file in two opposite ways: one asks for it whole in a single request, another reads
the tags at the end of the file and then moves through it in windows of a couple of megabytes. Both
are covered there.

`tests/the_binary_answers_over_a_real_socket.rs` starts the built binary and asks it over TCP,
including a stop and a restart, because the wiring in `main` is not covered by anything else. It
advertises over SSDP while it runs, so devices on your network will see it for a few seconds.

`tools/probe_upnp.py` is a control point with no opinions: it discovers a server, walks down to a
track, fetches it plain and with a range, and prints pass or fail.

    python3 tools/probe_upnp.py --name Kantele

`tools/probe_library.py` asks the same server the questions a smoke test skips, and wants a real
library to be worth anything: every menu, the far end of the longest list, the A-Z index, a
playlist played by range, cover art, every search capability the server declares, and eight clients
at once.

    python3 tools/probe_library.py --name Kantele

`tools/probe_changes.py` drops a file into a served folder and waits: the library grows, the update
id moves, a subscriber is told, the new track plays, and all of it comes back when the file goes. It
writes where the server reads, so it runs on the machine serving the folder.

    python3 tools/probe_changes.py --folder /volume1/music --file track.mp3

`tools/drive_renderer.py` goes the step the probe cannot: it hands a track to a real amplifier,
plays it, seeks, pauses, resumes, queues the track after it and waits for the change, then stops.
It makes sound in the room, so it refuses a renderer that is already playing or louder than
`--max-volume`, and `--dry-run` says what it would play without playing it.

    python3 tools/drive_renderer.py --renderer Marantz --server Kantele

Some amplifiers answer `GetTransportInfo` with empty fields and say what they are doing only in an
event; the driver subscribes and reads those where that happens.

A fourth gate runs here and not in a clone: captures of another server answering the same requests,
which are that server's output and are not redistributed. Several answers on the wire are copied
from it, and those captures are what hold them in place. Say in the pull request when you change a
value that goes on the wire, and it will be run against them.

## Implementing from the specification

Implement from the UPnP AV specification. Where the specification permits more than one answer,
the choice is made once, written down with its reason, and frozen in a snapshot, because a device
that works today has to keep working after the next commit.

A few of those choices follow a server that devices already work with, against the letter of the
specification. `contentFeatures.dlna.org` is one: it carries the short form the reference sends,
where the specification asks for the fourth field of `res@protocolInfo`. Each such choice says so
where it is written.

Devices are the hard part. The specification is public and the standard is old; what breaks a
renderer is an element order or a class it does not know, and no document lists those.

## Reporting a device

A device nobody here owns is the one thing this project cannot test for itself. Open an issue with:

- what the device is,
- which of discover, browse, search, play and seek it did,
- what the log said where it did not.

Setting `capture_dir` writes every control exchange, one folder per device, which is what makes
"it shows nothing" diagnosable. `tools/replay_clients.py` plays those files back against a build
and diffs the answers. `captures/` holds one such recording, and its README says what a replay of
it does and does not prove.

## Style

Comments are for what the code cannot say: an invariant, a trap, a deliberate choice that looks
wrong. One line. Rationale belongs in the commit message.

Prose in this repository states what is true and stops. No hedging, and no figures that will be
wrong at the next commit.
