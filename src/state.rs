//! The lock one serving process holds on its state folder.

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// The name of the lock inside the state folder.
const LOCK: &str = "kantele.lock";

/// A held lock. Dropping it, or the process dying, releases it.
pub struct StateLock {
    _file: File,
    path: PathBuf,
}

impl StateLock {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// What came of asking for the lock.
pub enum Lock {
    Taken(StateLock),
    /// Another live process holds it.
    Busy(PathBuf),
    /// The lock could not be attempted, and the caller carries on without one.
    Skipped(anyhow::Error),
}

/// Takes the state folder's lock without waiting for it.
pub fn lock(state_dir: &Path) -> Lock {
    match attempt(state_dir) {
        Ok(Some(held)) => Lock::Taken(held),
        Ok(None) => Lock::Busy(state_dir.join(LOCK)),
        Err(error) => Lock::Skipped(error),
    }
}

fn attempt(state_dir: &Path) -> Result<Option<StateLock>> {
    std::fs::create_dir_all(state_dir)
        .with_context(|| format!("creating {}", state_dir.display()))?;
    let path = state_dir.join(LOCK);
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&path)
        .with_context(|| format!("opening {}", path.display()))?;
    if take(&file).with_context(|| format!("locking {}", path.display()))? {
        Ok(Some(StateLock { _file: file, path }))
    } else {
        Ok(None)
    }
}

#[cfg(unix)]
fn take(file: &File) -> std::io::Result<bool> {
    use std::os::unix::io::AsRawFd;

    // The lock belongs to the open file description, so a second open in this same process is
    // refused exactly as another process would be.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
        return Ok(true);
    }
    let error = std::io::Error::last_os_error();
    match error.raw_os_error() {
        Some(code) if code == libc::EWOULDBLOCK => Ok(false),
        _ => Err(error),
    }
}

#[cfg(not(unix))]
fn take(_file: &File) -> std::io::Result<bool> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "no file locking on this platform",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("kantele-lock-{name}"));
        let _ = std::fs::remove_dir_all(&path);
        path
    }

    #[test]
    fn the_first_asker_takes_it() {
        let dir = folder("first");
        assert!(matches!(lock(&dir), Lock::Taken(_)));
    }

    #[test]
    fn a_second_asker_is_refused_while_the_first_holds_it() {
        let dir = folder("second");
        let held = lock(&dir);
        assert!(matches!(held, Lock::Taken(_)));
        assert!(
            matches!(lock(&dir), Lock::Busy(_)),
            "a held state folder must refuse the next server"
        );
        drop(held);
    }

    #[test]
    fn letting_go_lets_the_next_one_in() {
        let dir = folder("released");
        match lock(&dir) {
            Lock::Taken(held) => drop(held),
            _ => panic!("the first asker should have taken it"),
        }
        assert!(
            matches!(lock(&dir), Lock::Taken(_)),
            "a released lock must not outlive its holder"
        );
    }

    #[test]
    fn two_folders_do_not_share_a_lock() {
        let one = lock(&folder("one"));
        let two = lock(&folder("two"));
        assert!(matches!(one, Lock::Taken(_)));
        assert!(
            matches!(two, Lock::Taken(_)),
            "instances on separate state folders are exactly what is wanted"
        );
    }
}
