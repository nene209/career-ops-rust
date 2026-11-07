//! Application logging: one `tracing` seam, two sinks.
//!
//! `tracing` is already the instrumentation everywhere else in the app, so it
//! stays the only seam. This module adds the two things it was missing:
//!
//! | Sink | Purpose | Lifetime |
//! | :--- | :--- | :--- |
//! | `<data_dir>/logs/career-ops.log` | The durable record; survives a crash and is what a bug report attaches | Rotated, [`KEEP_ARCHIVES`] archives |
//! | [`LogBuffer`] | What the UI renders, live, with structure intact | This session |
//!
//! The UI never reads the file: the buffer already carries level, target,
//! message, and fields, and re-parsing formatted text to recover them would be
//! a second, lossier implementation.
//!
//! # Capture policy
//!
//! Without `RUST_LOG`, everything is captured at `info` and the app's own
//! targets at `debug`. The file keeps the quieter `info` view — a `debug` line
//! per enriched description would bury the per-page lines — while the bounded
//! buffer takes the deeper app-only view so the Logs screen's level chips have
//! something to reveal. `RUST_LOG` overrides both, and an empty value is
//! ignored rather than silencing the log.
//!
//! # Privacy
//!
//! The database is encrypted; this file is not. **Never log** passphrases, key
//! material, CV or profile text, prompts and completions, job description
//! bodies, or HTTP response bodies. Log URLs, ids, counts, sizes, statuses,
//! tiers, stop reasons, and error strings. Anything body-adjacent goes through
//! [`truncate_for_log`] first.

mod buffer;
mod file;
mod line;
mod panic_hook;

pub use buffer::{BufferLayer, LogBuffer, Wake, DEFAULT_CAPACITY};
pub use file::{
    delete_logs, prune_archives, rotate_if_needed, should_rotate, LogFile, KEEP_ARCHIVES,
    LOG_FILE_NAME, ROTATE_BYTES,
};
pub use line::LogLine;
pub use panic_hook::install_panic_hook;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use tracing_subscriber::filter::EnvFilter;
use tracing_subscriber::prelude::*;

/// The directory under the data dir that holds the log.
pub const LOG_DIR: &str = "logs";

/// What is captured when `RUST_LOG` says nothing.
const DEFAULT_CAPTURE: &str = "info,career_ops_core=debug,career_ops_app=debug";

/// What the file keeps under that policy.
const FILE_CAPTURE: &str = "info";

/// Where the logs go. `main` uses [`LogMode::File`]; tests and
/// [`handle`] use [`LogMode::BufferOnly`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogMode {
    File,
    BufferOnly,
}

/// Where the capture level came from, so the Logs screen can say so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapturePolicy {
    /// The built-in `info` policy, with `debug` for the app's own targets.
    Default,
    /// Whatever `RUST_LOG` says.
    Env,
}

impl CapturePolicy {
    pub fn label(self) -> &'static str {
        match self {
            Self::Default => "info (debug for career-ops targets)",
            Self::Env => "RUST_LOG",
        }
    }
}

/// A shared handle to the installed sinks.
#[derive(Clone)]
pub struct LogHandle {
    inner: Arc<Installed>,
}

struct Installed {
    buffer: LogBuffer,
    path: Option<PathBuf>,
    policy: CapturePolicy,
}

impl LogHandle {
    pub fn buffer(&self) -> &LogBuffer {
        &self.inner.buffer
    }

    /// The file being written, or `None` when only the buffer is installed.
    pub fn path(&self) -> Option<&Path> {
        self.inner.path.as_deref()
    }

    pub fn policy(&self) -> CapturePolicy {
        self.inner.policy
    }

    pub fn latest_seq(&self) -> u64 {
        self.inner.buffer.latest_seq()
    }

    pub fn snapshot(&self) -> Vec<LogLine> {
        self.inner.buffer.snapshot()
    }

    pub fn dropped(&self) -> u64 {
        self.inner.buffer.dropped()
    }

    /// Called after every stored line; the app installs a repaint request here
    /// so a line from a background task appears without waiting for the next
    /// incidental frame.
    pub fn set_wake(&self, wake: Wake) {
        self.inner.buffer.set_wake(wake);
    }

    pub fn clear(&self) {
        self.inner.buffer.clear();
    }
}

static HANDLE: OnceLock<LogHandle> = OnceLock::new();

/// Install the sinks once and return the handle.
///
/// Idempotent, and **the first call wins**: a later [`LogMode::File`] after
/// [`handle`] cannot add the file sink, because the global subscriber is
/// already installed. `main` is the only caller of `File`, and it runs before
/// anything else.
pub fn init(mode: LogMode) -> LogHandle {
    HANDLE.get_or_init(|| install(mode)).clone()
}

/// The installed handle, installing a buffer-only one if nothing has yet.
///
/// This is what the app and the tests get: no file, no data directory.
pub fn handle() -> LogHandle {
    HANDLE.get_or_init(|| install(LogMode::BufferOnly)).clone()
}

/// Shorten a value for a log line, cutting on a character boundary.
pub fn truncate_for_log(value: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (index, character) in value.chars().enumerate() {
        if index == max_chars {
            out.push('…');
            break;
        }
        out.push(character);
    }
    out
}

fn env_spec() -> Option<String> {
    std::env::var("RUST_LOG")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn install(mode: LogMode) -> LogHandle {
    let buffer = LogBuffer::new(DEFAULT_CAPACITY);
    let (global_spec, file_spec, policy) = match env_spec() {
        Some(spec) => (spec.clone(), spec, CapturePolicy::Env),
        None => (
            DEFAULT_CAPTURE.to_string(),
            FILE_CAPTURE.to_string(),
            CapturePolicy::Default,
        ),
    };

    let mut path = None;

    match mode {
        LogMode::File => {
            let dir = crate::data_dir().join(LOG_DIR);
            match file::LogFile::open(&dir) {
                Ok((sink, log_path)) => {
                    let subscriber = tracing_subscriber::registry()
                        .with(EnvFilter::new(global_spec))
                        .with(BufferLayer::new(buffer.clone()))
                        .with(
                            tracing_subscriber::fmt::layer()
                                .with_ansi(false)
                                .with_writer(sink)
                                .with_filter(EnvFilter::new(file_spec)),
                        );
                    // `try_init` installs the `log` bridge itself (that is what
                    // `fmt::init` used to do) and fails only if a subscriber is
                    // already installed. Neither is fatal: the buffer still
                    // exists, it just receives nothing.
                    let _ = subscriber.try_init();
                    panic_hook::install_panic_hook(&log_path);
                    path = Some(log_path);
                }
                Err(error) => {
                    // No file sink is not "no logging": the buffer is the part
                    // the user can actually see.
                    let _ = writeln!(
                        std::io::stderr(),
                        "career-ops: the log file could not be opened ({error}); logging to the in-app buffer only"
                    );
                    let subscriber = tracing_subscriber::registry()
                        .with(EnvFilter::new(global_spec))
                        .with(BufferLayer::new(buffer.clone()));
                    let _ = subscriber.try_init();
                }
            }
        }
        LogMode::BufferOnly => {
            let subscriber = tracing_subscriber::registry()
                .with(EnvFilter::new(global_spec))
                .with(BufferLayer::new(buffer.clone()));
            let _ = subscriber.try_init();
        }
    }

    LogHandle {
        inner: Arc::new(Installed {
            buffer,
            path,
            policy,
        }),
    }
}

/// A poisoned lock must not take the app down: this is diagnostics.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncation_cuts_on_a_character_boundary() {
        assert_eq!(truncate_for_log("abc", 10), "abc");
        assert_eq!(truncate_for_log("abcdef", 3), "abc…");
        assert_eq!(truncate_for_log("héllo", 2), "hé…");
        assert_eq!(truncate_for_log("", 0), "");
        assert_eq!(truncate_for_log("abc", 0), "…");
    }

    #[test]
    fn the_capture_policy_says_where_it_came_from() {
        assert!(CapturePolicy::Default.label().contains("debug"));
        assert_eq!(CapturePolicy::Env.label(), "RUST_LOG");
    }
}
