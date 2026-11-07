//! The in-process ring buffer, and the `Layer` that fills it.
//!
//! The buffer is what the Logs screen renders. It is bounded on purpose: an
//! unbounded buffer in a long session is a slow leak, and a silently truncated
//! one is a lie, so the oldest lines are dropped and counted.
//!
//! Pushing a line never blocks on the UI: the wake callback is called after the
//! lock is released, so a callback that reads the buffer cannot deadlock.

use std::collections::VecDeque;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Local};
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::Context;
use tracing_subscriber::Layer;

use super::line::LogLine;
use super::lock;

/// How many lines the UI can scroll back through.
pub const DEFAULT_CAPACITY: usize = 2_000;

/// The wake callback: the app installs `egui::Context::request_repaint` here so
/// a line from a background task appears without waiting for an incidental
/// repaint.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

#[derive(Default)]
struct State {
    lines: VecDeque<LogLine>,
    wake: Option<Wake>,
}

struct Shared {
    capacity: usize,
    state: Mutex<State>,
    next_seq: AtomicU64,
    dropped: AtomicU64,
}

/// A bounded, cloneable handle to the captured lines.
#[derive(Clone)]
pub struct LogBuffer {
    shared: Arc<Shared>,
}

impl LogBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            shared: Arc::new(Shared {
                capacity: capacity.max(1),
                state: Mutex::new(State::default()),
                next_seq: AtomicU64::new(0),
                dropped: AtomicU64::new(0),
            }),
        }
    }

    /// Record one event, timestamped now.
    pub fn push(
        &self,
        level: Level,
        target: &'static str,
        message: String,
        fields: Vec<(String, String)>,
    ) {
        self.push_at(Local::now(), level, target, message, fields);
    }

    /// Record one event with an explicit timestamp; tests use this.
    pub fn push_at(
        &self,
        timestamp: DateTime<Local>,
        level: Level,
        target: &'static str,
        message: String,
        fields: Vec<(String, String)>,
    ) {
        // Every event consumes a sequence number, including one that is about
        // to be dropped, so `latest_seq` is a watermark of what was *emitted*
        // and "N new lines" stays correct across eviction.
        let seq = self.shared.next_seq.fetch_add(1, Ordering::Relaxed) + 1;
        let line = LogLine {
            seq,
            timestamp,
            level,
            target,
            message,
            fields,
        };

        let wake = {
            let mut state = lock(&self.shared.state);
            if state.lines.len() == self.shared.capacity {
                state.lines.pop_front();
                self.shared.dropped.fetch_add(1, Ordering::Relaxed);
            }
            state.lines.push_back(line);
            state.wake.clone()
        };

        if let Some(wake) = wake {
            wake();
        }
    }

    /// The captured lines, oldest first.
    pub fn snapshot(&self) -> Vec<LogLine> {
        lock(&self.shared.state).lines.iter().cloned().collect()
    }

    /// The sequence number of the most recent event, `0` before the first.
    pub fn latest_seq(&self) -> u64 {
        self.shared.next_seq.load(Ordering::Relaxed)
    }

    /// How many lines the cap has evicted this session.
    pub fn dropped(&self) -> u64 {
        self.shared.dropped.load(Ordering::Relaxed)
    }

    pub fn capacity(&self) -> usize {
        self.shared.capacity
    }

    pub fn len(&self) -> usize {
        lock(&self.shared.state).lines.len()
    }

    pub fn is_empty(&self) -> bool {
        lock(&self.shared.state).lines.is_empty()
    }

    /// Forget the lines and the drop count. Sequence numbers keep growing, so a
    /// stale watermark can never reveal a cleared line.
    pub fn clear(&self) {
        let mut state = lock(&self.shared.state);
        state.lines.clear();
        self.shared.dropped.store(0, Ordering::Relaxed);
    }

    pub fn set_wake(&self, wake: Wake) {
        lock(&self.shared.state).wake = Some(wake);
    }
}

/// A `tracing` layer that copies events into a [`LogBuffer`].
pub struct BufferLayer {
    buffer: LogBuffer,
}

impl BufferLayer {
    pub fn new(buffer: LogBuffer) -> Self {
        Self { buffer }
    }
}

impl<S: Subscriber> Layer<S> for BufferLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut visitor = FieldVisitor::default();
        event.record(&mut visitor);
        let metadata = event.metadata();
        self.buffer.push(
            *metadata.level(),
            metadata.target(),
            visitor.message.unwrap_or_default(),
            visitor.fields,
        );
    }
}

#[derive(Default)]
struct FieldVisitor {
    message: Option<String>,
    fields: Vec<(String, String)>,
}

impl FieldVisitor {
    fn store(&mut self, name: &str, value: String) {
        if name == "message" {
            self.message = Some(value);
        } else {
            self.fields.push((name.to_string(), value));
        }
    }
}

impl Visit for FieldVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.store(field.name(), format!("{value:?}"));
    }

    /// The default implementation forwards to `record_debug`, which would wrap
    /// a plain string in quotes. `&str` fields are the common case for URLs and
    /// ids, so they are recorded verbatim.
    fn record_str(&mut self, field: &Field, value: &str) {
        self.store(field.name(), value.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push_many(buffer: &LogBuffer, count: usize) {
        for index in 0..count {
            buffer.push(Level::INFO, "test", format!("line-{index}"), Vec::new());
        }
    }

    #[test]
    fn the_cap_drops_the_oldest_line_and_counts_it() {
        let buffer = LogBuffer::new(2);
        push_many(&buffer, 5);

        let lines = buffer.snapshot();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].message, "line-3");
        assert_eq!(lines[1].message, "line-4");
        assert_eq!(buffer.dropped(), 3);
        assert_eq!(
            buffer.latest_seq(),
            5,
            "every event consumes a sequence number, including evicted ones"
        );
    }

    #[test]
    fn clearing_resets_the_lines_and_the_drop_count() {
        let buffer = LogBuffer::new(1);
        push_many(&buffer, 3);
        buffer.clear();

        assert!(buffer.is_empty());
        assert_eq!(buffer.dropped(), 0);
        assert_eq!(
            buffer.latest_seq(),
            3,
            "the watermark keeps moving so a stale pause cannot reveal old lines"
        );
    }

    #[test]
    fn the_wake_callback_runs_after_the_line_is_stored() {
        let buffer = LogBuffer::new(8);
        let seen = Arc::new(Mutex::new(None));
        let observed = Arc::clone(&seen);
        let reader = buffer.clone();
        buffer.set_wake(Arc::new(move || {
            // Reading the buffer from the callback must not deadlock.
            *observed.lock().unwrap() = Some(reader.len());
        }));

        buffer.push(Level::WARN, "test", "wake".into(), Vec::new());

        assert_eq!(*seen.lock().unwrap(), Some(1));
    }

    #[test]
    fn a_zero_capacity_still_holds_one_line() {
        let buffer = LogBuffer::new(0);
        push_many(&buffer, 1);
        assert_eq!(buffer.len(), 1);
        assert_eq!(buffer.capacity(), 1);
    }
}
