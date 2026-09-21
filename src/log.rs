//! The log file beside the index, rolled by the server itself.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Past this the file is moved aside as `kantele.log.1` and a new one begun, so two of them are
/// the most a state folder ever carries.
pub const CAP: u64 = 8 * 1024 * 1024;

/// How far back a tail looks. Lines are short; this is thousands of them.
const TAIL_WINDOW: u64 = 512 * 1024;

pub fn path(state_dir: &Path) -> PathBuf {
    state_dir.join("kantele.log")
}

/// Where the lines go besides stdout. Lines written before the state folder is known wait in
/// memory, so a start that fails under a service manager still leaves them in the file.
#[derive(Clone, Default)]
pub struct Sink(Arc<Mutex<Held>>);

enum Held {
    Pending(Vec<u8>),
    Open(Rolling),
    Off,
}

impl Default for Held {
    fn default() -> Self {
        Self::Pending(Vec::new())
    }
}

struct Rolling {
    path: PathBuf,
    file: File,
    written: u64,
    cap: u64,
}

impl Sink {
    /// Opens the file and gives it what was written so far.
    pub fn open(&self, path: PathBuf) -> io::Result<()> {
        self.open_capped(path, CAP)
    }

    fn open_capped(&self, path: PathBuf, cap: u64) -> io::Result<()> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        let written = file.metadata()?.len();
        let mut rolling = Rolling {
            path,
            file,
            written,
            cap,
        };
        let mut held = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Held::Pending(early) = &*held {
            rolling.write_all(early)?;
        }
        *held = Held::Open(rolling);
        Ok(())
    }

    /// A terminal run: stdout is the log, and what waited is dropped.
    pub fn off(&self) {
        let mut held = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *held = Held::Off;
    }

    fn write(&self, buf: &[u8]) -> io::Result<()> {
        let mut held = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match &mut *held {
            Held::Pending(early) => {
                early.extend_from_slice(buf);
                Ok(())
            }
            Held::Open(rolling) => rolling.write_all(buf),
            Held::Off => Ok(()),
        }
    }
}

impl Write for Rolling {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.written + buf.len() as u64 > self.cap && self.written > 0 {
            let aside = self.path.with_extension("log.1");
            std::fs::rename(&self.path, &aside)?;
            self.file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)?;
            self.written = 0;
        }
        self.file.write_all(buf)?;
        self.written += buf.len() as u64;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

/// One formatted line's writer: stdout, then the file.
pub struct Tee {
    sink: Sink,
}

impl Write for Tee {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // A closed stdout is not a reason to lose the line the file would have kept.
        let _ = io::stdout().write_all(buf);
        self.sink.write(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        io::stdout().flush()
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Sink {
    type Writer = Tee;

    fn make_writer(&'a self) -> Tee {
        Tee { sink: self.clone() }
    }
}

/// A panic is a line in the log first, then what it was before.
pub fn catch_panics() {
    let before = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!(%info, "a task panicked");
        before(info);
    }));
}

/// The last `lines` complete lines of the file, oldest first.
pub fn tail(path: &Path, lines: usize) -> io::Result<String> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let from = len.saturating_sub(TAIL_WINDOW);
    file.seek(SeekFrom::Start(from))?;
    let mut bytes = Vec::with_capacity((len - from) as usize);
    file.read_to_end(&mut bytes)?;
    let text = String::from_utf8_lossy(&bytes);
    let mut kept: Vec<&str> = text.lines().collect();
    // A window that opened mid-line begins with a fragment.
    if from > 0 && !kept.is_empty() {
        kept.remove(0);
    }
    let skip = kept.len().saturating_sub(lines);
    let mut out: String = kept[skip..].join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kantele-log-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch folder");
        dir
    }

    #[test]
    fn what_was_said_before_the_file_opened_is_the_first_thing_in_it() {
        let dir = scratch("early");
        let sink = Sink::default();
        sink.write(b"first\n").expect("held");
        sink.open(path(&dir)).expect("opened");
        sink.write(b"second\n").expect("written");
        assert_eq!(
            std::fs::read_to_string(path(&dir)).expect("the log"),
            "first\nsecond\n"
        );
    }

    #[test]
    fn a_file_past_its_cap_is_moved_aside_and_begun_again() {
        let dir = scratch("roll");
        let sink = Sink::default();
        sink.open_capped(path(&dir), 20).expect("opened");
        sink.write(b"one line of ten\n").expect("written");
        sink.write(b"the line that tips it over\n")
            .expect("written");
        assert_eq!(
            std::fs::read_to_string(path(&dir)).expect("the log"),
            "the line that tips it over\n"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("kantele.log.1")).expect("the one set aside"),
            "one line of ten\n"
        );
    }

    #[test]
    fn a_tail_is_the_last_lines_oldest_first_and_no_more_than_asked() {
        let dir = scratch("tail");
        std::fs::write(path(&dir), "a\nb\nc\nd\n").expect("a log");
        assert_eq!(tail(&path(&dir), 2).expect("a tail"), "c\nd\n");
        assert_eq!(tail(&path(&dir), 10).expect("a tail"), "a\nb\nc\nd\n");
        std::fs::write(path(&dir), "").expect("an empty log");
        assert_eq!(tail(&path(&dir), 10).expect("a tail"), "");
    }

    #[test]
    fn a_tail_of_a_file_larger_than_its_window_begins_on_a_whole_line() {
        let dir = scratch("window");
        let line = "x".repeat(1000) + "\n";
        let many: String = (0..600).map(|_| line.as_str()).collect();
        std::fs::write(path(&dir), &many).expect("a large log");
        let got = tail(&path(&dir), 3).expect("a tail");
        assert_eq!(got.lines().count(), 3);
        assert!(got.lines().all(|l| l.len() == 1000), "no fragment leads");
    }

    #[test]
    fn a_missing_file_is_an_error_the_caller_can_name() {
        let dir = scratch("missing");
        let error = tail(&path(&dir), 5).expect_err("no file");
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }
}
