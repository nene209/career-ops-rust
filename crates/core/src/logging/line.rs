//! One captured log event.
//!
//! The buffer keeps structure rather than a formatted line: the Logs screen
//! filters by level and shows `key=value` pairs, and recovering either from
//! formatted text would be a second, lossier implementation of what `tracing`
//! already knows.

use chrono::{DateTime, Local};
use tracing::Level;

/// A log event as both sinks see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    /// Monotonic for the life of the process; never `0`, so `0` can mean
    /// "nothing seen yet".
    pub seq: u64,
    pub timestamp: DateTime<Local>,
    pub level: Level,
    /// The `tracing` target, normally the emitting module path.
    pub target: &'static str,
    pub message: String,
    /// Structured fields in the order they were recorded.
    pub fields: Vec<(String, String)>,
}

impl LogLine {
    /// `12:34:56.789`
    pub fn timestamp_short(&self) -> String {
        self.timestamp.format("%H:%M:%S%.3f").to_string()
    }

    /// One line, the shape **Copy visible** puts on the clipboard:
    /// `12:34:56.789 INFO  acquisition::router: message page=30 jobs=40`
    pub fn compact(&self) -> String {
        let mut out = format!(
            "{} {:<5} {}: {}",
            self.timestamp_short(),
            self.level.as_str(),
            self.target,
            self.message
        );
        for (key, value) in &self.fields {
            out.push(' ');
            out.push_str(key);
            out.push('=');
            out.push_str(value);
        }
        out
    }
}
