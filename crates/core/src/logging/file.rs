//! The durable sink: a rotated plain-text log file.
//!
//! This is what survives a crash and what a bug report attaches, so it is
//! deliberately plain: `fmt`-formatted lines, no ansi, flushed per line. The
//! volume is tiny — a fifty-page walk is about fifty lines — so a buffered
//! writer would only cost the tail of a crashed run.
//!
//! Rotation happens once, at startup, when the current file has reached
//! [`ROTATE_BYTES`]. Rotating on every write would mean stat-ing the file on a
//! hot path for an app that writes tens of lines a day.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use chrono::Local;
use tracing_subscriber::fmt::MakeWriter;

use super::lock;

/// The current log file's name, inside the log directory.
pub const LOG_FILE_NAME: &str = "career-ops.log";

/// The size at which the current file is archived at startup.
pub const ROTATE_BYTES: u64 = 5 * 1024 * 1024;

/// How many archived files are kept.
pub const KEEP_ARCHIVES: usize = 5;

/// `true` when a file of `size` bytes should be archived.
pub fn should_rotate(size: u64, limit: u64) -> bool {
    size >= limit
}

/// A `MakeWriter` that appends every line to one file and flushes it.
#[derive(Clone)]
pub struct LogFile {
    file: Arc<Mutex<fs::File>>,
}

impl LogFile {
    /// Open `<dir>/career-ops.log`, rotating first at [`ROTATE_BYTES`].
    pub fn open(dir: &Path) -> io::Result<(Self, PathBuf)> {
        Self::open_with(dir, ROTATE_BYTES)
    }

    /// Open with an explicit rotation limit; tests use this.
    pub fn open_with(dir: &Path, limit: u64) -> io::Result<(Self, PathBuf)> {
        fs::create_dir_all(dir)?;
        rotate_if_needed(dir, limit)?;

        let path = dir.join(LOG_FILE_NAME);
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        Ok((
            Self {
                file: Arc::new(Mutex::new(file)),
            },
            path,
        ))
    }

    /// Write one already-formatted chunk, ignoring a write failure after
    /// reporting it once: a full disk must not fail a fetch.
    pub fn write_line(&self, line: &str) {
        let mut writer = LogWriter {
            file: Arc::clone(&self.file),
        };
        let _ = writer.write_all(line.as_bytes());
        let _ = writer.flush();
    }
}

impl<'a> MakeWriter<'a> for LogFile {
    type Writer = LogWriter;

    fn make_writer(&'a self) -> Self::Writer {
        LogWriter {
            file: Arc::clone(&self.file),
        }
    }
}

/// One write handle onto the log file.
pub struct LogWriter {
    file: Arc<Mutex<fs::File>>,
}

impl Write for LogWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut file = lock(&self.file);
        match file.write(buf) {
            Ok(written) => {
                // Flushed per line: the tail is what a crash report needs, and
                // the volume is far too low to make buffering worthwhile.
                if let Err(error) = file.flush() {
                    report_failure(&error);
                }
                Ok(written)
            }
            Err(error) => {
                report_failure(&error);
                Ok(buf.len())
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        lock(&self.file).flush()
    }
}

/// Say once that the log file is unusable, on stderr rather than through
/// `tracing` (a failing sink must not feed itself).
fn report_failure(error: &io::Error) {
    static REPORTED: AtomicBool = AtomicBool::new(false);
    if REPORTED.swap(true, Ordering::Relaxed) {
        return;
    }
    let _ = writeln!(
        io::stderr(),
        "career-ops: the log file could not be written ({error}); file logging is off for this session"
    );
}

/// Archive the current file if it is at or above `limit`.
///
/// Returns the archive's path when a rotation happened.
pub fn rotate_if_needed(dir: &Path, limit: u64) -> io::Result<Option<PathBuf>> {
    let current = dir.join(LOG_FILE_NAME);
    let size = match fs::metadata(&current) {
        Ok(metadata) => metadata.len(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if !should_rotate(size, limit) {
        return Ok(None);
    }

    let archive = next_archive_path(dir);
    fs::rename(&current, &archive)?;
    prune_archives(dir, KEEP_ARCHIVES)?;
    Ok(Some(archive))
}

/// Delete the oldest archives so at most `keep` remain.
///
/// Returns the paths that were removed. Names are timestamped
/// (`career-ops.YYYYMMDD-HHMMSS.mmm.log`), so a lexical sort is a
/// chronological one.
pub fn prune_archives(dir: &Path, keep: usize) -> io::Result<Vec<PathBuf>> {
    let mut archives = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if is_archive(&name) {
            archives.push(entry.path());
        }
    }
    archives.sort();

    let mut removed = Vec::new();
    while archives.len() > keep {
        let oldest = archives.remove(0);
        fs::remove_file(&oldest)?;
        removed.push(oldest);
    }
    Ok(removed)
}

/// Delete the current log file and its archives; returns how many files went.
///
/// The writer keeps its open handle, so the unlinked file is only really gone
/// once the process exits and a fresh file appears on the next launch. Callers
/// that say otherwise would be lying.
pub fn delete_logs(dir: &Path) -> io::Result<usize> {
    let mut removed = prune_archives(dir, 0)?.len();
    match fs::remove_file(dir.join(LOG_FILE_NAME)) {
        Ok(()) => removed += 1,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    Ok(removed)
}

/// The next free archive path for "now".
fn next_archive_path(dir: &Path) -> PathBuf {
    let prefix = ARCHIVE_PREFIX;
    let stamp = Local::now().format("%Y%m%d-%H%M%S%.3f").to_string();
    let mut candidate = dir.join(format!("{prefix}{stamp}.log"));
    // Two rotations inside the same millisecond would collide; suffix instead
    // of overwriting a record.
    let mut attempt = 1;
    while candidate.exists() {
        candidate = dir.join(format!("{prefix}{stamp}-{attempt}.log"));
        attempt += 1;
    }
    candidate
}

/// `career-ops.` — the prefix every archive shares.
const ARCHIVE_PREFIX: &str = "career-ops.";

fn is_archive(name: &str) -> bool {
    name != LOG_FILE_NAME && name.starts_with(ARCHIVE_PREFIX) && name.ends_with(".log")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rotation_threshold_is_inclusive() {
        assert!(!should_rotate(0, 10));
        assert!(!should_rotate(9, 10));
        assert!(should_rotate(10, 10));
        assert!(should_rotate(11, 10));
    }

    #[test]
    fn the_current_file_is_not_mistaken_for_an_archive() {
        assert!(!is_archive(LOG_FILE_NAME));
        assert!(is_archive("career-ops.20260101-000000.000.log"));
        assert!(!is_archive("career-ops.20260101-000000.000.log.gz"));
    }

    #[test]
    fn opening_creates_the_directory_and_an_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("a").join("b");

        let (_log, path) = LogFile::open(&nested).unwrap();

        assert_eq!(path, nested.join(LOG_FILE_NAME));
        assert!(path.exists());
    }

    #[test]
    fn a_rotation_archives_the_file_and_leaves_the_path_free() {
        let dir = tempfile::tempdir().unwrap();
        let current = dir.path().join(LOG_FILE_NAME);
        fs::write(&current, vec![b'x'; 100]).unwrap();

        let archive = rotate_if_needed(dir.path(), 50).unwrap().expect("rotated");

        assert!(archive.exists());
        assert!(!current.exists(), "the file is renamed, not copied");
        let name = archive.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("career-ops."));
        assert!(name.ends_with(".log"));
    }

    #[test]
    fn deleting_removes_the_current_file_and_every_archive() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(LOG_FILE_NAME), b"current").unwrap();
        for day in 1..=3 {
            fs::write(
                dir.path()
                    .join(format!("career-ops.2026010{day}-000000.000.log")),
                b"archive",
            )
            .unwrap();
        }

        let removed = delete_logs(dir.path()).unwrap();

        assert_eq!(removed, 4);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
        assert_eq!(
            delete_logs(dir.path()).unwrap(),
            0,
            "deleting again is a no-op, not an error"
        );
    }

    #[test]
    fn a_file_below_the_limit_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(LOG_FILE_NAME), b"short").unwrap();

        assert!(rotate_if_needed(dir.path(), 50).unwrap().is_none());
        assert!(dir.path().join(LOG_FILE_NAME).exists());
    }
}
