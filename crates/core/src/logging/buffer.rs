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
    /// The sequence number of the last stored line, and the number of lines the
    /// cap has evicted. Both live under the same lock as `lines` on purpose:
    /// assigning a number outside it let two threads claim 5 and 6 and append
    /// them in the other order, which broke the "snapshot order follows the
    /// sequence" invariant the UI relies on when it keys a refresh on
    /// `latest_seq`.
    next_seq: u64,
    dropped: u64,
}

struct Shared {
    capacity: usize,
    state: Mutex<State>,
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
        // and "N new lines" stays correct across eviction. Taking the number
        // under the lock is what makes sequence order and storage order the
        // same thing.
        let wake = {
            let mut state = lock(&self.shared.state);
            let seq = state.next_seq + 1;
            state.next_seq = seq;
            let line = LogLine {
                seq,
                timestamp,
                level,
                target,
                message,
                fields,
            };

            if state.lines.len() == self.shared.capacity {
                state.lines.pop_front();
                state.dropped += 1;
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
    ///
    /// Read under the buffer lock, so a caller that sees `N` here also sees
    /// every line up to `N` in [`snapshot`](Self::snapshot).
    pub fn latest_seq(&self) -> u64 {
        lock(&self.shared.state).next_seq
    }

    /// How many lines the cap has evicted this session.
    pub fn dropped(&self) -> u64 {
        lock(&self.shared.state).dropped
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
        state.dropped = 0;
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

    /// Sequence order and storage order must agree even under concurrent
    /// writers: the UI refreshes its snapshot when `latest_seq` changes, so a
    /// line appended "behind" a newer one would be missed.
    #[test]
    fn concurrent_writers_keep_the_snapshot_in_sequence_order() {
        let buffer = LogBuffer::new(1_000);
        let mut threads = Vec::new();
        for thread in 0..4 {
            let buffer = buffer.clone();
            threads.push(std::thread::spawn(move || {
                for index in 0..50 {
                    buffer.push(
                        Level::INFO,
                        "test",
                        format!("t{thread}-{index}"),
                        Vec::new(),
                    );
                }
            }));
        }
        for thread in threads {
            thread.join().expect("writer thread");
        }

        let lines = buffer.snapshot();
        assert_eq!(lines.len(), 200);
        assert!(
            lines.windows(2).all(|pair| pair[0].seq < pair[1].seq),
            "snapshot order must follow the sequence"
        );
        assert_eq!(buffer.latest_seq(), 200);
    }
}
