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
    match held(&path).with_context(|| format!("locking {}", path.display()))? {
        Some(file) => Ok(Some(StateLock { _file: file, path })),
        None => Ok(None),
    }
}

fn opened() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.create(true).read(true).write(true).truncate(false);
    options
}

#[cfg(unix)]
fn held(path: &Path) -> std::io::Result<Option<File>> {
    use std::os::unix::io::AsRawFd;

    let file = opened().open(path)?;
    // The lock belongs to the open file description, so a second open in this same process is
    // refused exactly as another process would be.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
        return Ok(Some(file));
    }
    let error = std::io::Error::last_os_error();
    match error.raw_os_error() {
        Some(code) if code == libc::EWOULDBLOCK => Ok(None),
        _ => Err(error),
    }
}

/// Windows has no advisory lock to take once the file is open: the exclusion is the open itself,
/// granting no sharing right to anybody else, and the next asker is told the file is in use.
#[cfg(windows)]
fn held(path: &Path) -> std::io::Result<Option<File>> {
    use std::os::windows::fs::OpenOptionsExt;

    const SHARING_VIOLATION: i32 = 32;
    const LOCK_VIOLATION: i32 = 33;

    match opened().share_mode(0).open(path) {
        Ok(file) => Ok(Some(file)),
        Err(error) => match error.raw_os_error() {
            Some(SHARING_VIOLATION | LOCK_VIOLATION) => Ok(None),
            _ => Err(error),
        },
    }
}

#[cfg(not(any(unix, windows)))]
fn held(_path: &Path) -> std::io::Result<Option<File>> {
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
