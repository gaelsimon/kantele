# Probes

These are not samples of how to use the crate. Each one measures something, and they are here
because they answer questions that keep coming back: what a walk costs against what a tag read
costs, and where the index's memory goes.

Four of them build a synthetic library and need no music, so they run anywhere. Pass a track count
to size it:

    cargo run --release --example search_cost -- 42000

`walk_cost` and `index_memory` read a real folder, because what they measure is the disk:

    cargo run --release --example walk_cost -- /volume1/music

They print to stdout and write nothing.

They also build under `cargo clippy --all-targets`, so one that stops compiling is a change to the
library's own interface that nothing else caught. Compiling is all they have to do, though: a probe
has no assertions, so one can go on timing a route the server no longer takes. Read the labels
against the code before trusting a number.
