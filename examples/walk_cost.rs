//! Times a walk of the folder given as argument.

use std::path::PathBuf;
use std::time::Instant;

use kantele::index::scan;

fn main() {
    let root = PathBuf::from(
        std::env::args()
            .nth(1)
            .expect("usage: walk_cost <music folder>"),
    );

    let started = Instant::now();
    let walked = scan::walk(&root).expect("walking the library");
    let elapsed = started.elapsed();

    let folders: std::collections::HashSet<&std::path::Path> = walked
        .files
        .iter()
        .filter_map(|found| found.path.parent())
        .collect();
    println!(
        "{} files in {} folders holding music, {} of them with a cover file",
        walked.files.len(),
        folders.len(),
        walked.covers.len()
    );
    println!("walk {:.1} s", elapsed.as_secs_f64());
    println!(
        "{:.1} ms per folder holding music",
        elapsed.as_secs_f64() * 1000.0 / folders.len().max(1) as f64
    );
}
