# Captures

What a real client sent and what it was answered, written verbatim by `capture_dir`. One folder per
client. `tools/replay_clients.py captures` plays them back against a running server and diffs each
answer, normalising the host so the address it was recorded on does not matter.

    python3 tools/replay_clients.py captures --target http://127.0.0.1:8200

An answer names the objects of the library it was recorded against, so a replay only agrees when the
server holds that same library. Against another library the diff is noise, not a failure of the
server. What these are always good for is reading what a client actually asks for.

`vlc-3.0.23` is VLC on macOS browsing a small library: `Browse` and nothing else, five thousand
children at a time, no sort criteria, and media fetched with an open-ended byte range.
