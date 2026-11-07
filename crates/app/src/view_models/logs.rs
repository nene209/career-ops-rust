//! Logs view-model: which of the captured lines the screen shows.
//!
//! Pure — no egui, no clock. The screen and the drawer both render through this
//! type, which is what makes pausing in one pause the other, and what lets the
//! filtering and the pause watermark be tested without a UI.

use career_ops_core::logging::LogLine;
use tracing::Level;

/// How severe a line must be to appear.
///
/// The variants are the `tracing` levels plus `All`; the order in [`all`] is
/// most-to-least severe so the chips read like a log viewer's.
///
/// [`all`]: LevelFilter::all
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LevelFilter {
    #[default]
    All,
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl LevelFilter {
    pub fn all() -> &'static [LevelFilter] {
        &[
            Self::All,
            Self::Error,
            Self::Warn,
            Self::Info,
            Self::Debug,
            Self::Trace,
        ]
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Error => "Error",
            Self::Warn => "Warn",
            Self::Info => "Info",
            Self::Debug => "Debug",
            Self::Trace => "Trace",
        }
    }

    /// `All` admits everything; every other chip admits that level and anything
    /// at least as severe.
    ///
    /// `tracing::Level` orders severity as ERROR < WARN < INFO < DEBUG < TRACE,
    /// so "at least as severe" is `<=`.
    pub fn admits(self, level: Level) -> bool {
        match self {
            Self::All | Self::Trace => true,
            Self::Error => level <= Level::ERROR,
            Self::Warn => level <= Level::WARN,
            Self::Info => level <= Level::INFO,
            Self::Debug => level <= Level::DEBUG,
        }
    }
}

/// The Logs screen's state: what to show, and whether it is frozen.
#[derive(Debug, Clone)]
pub struct LogsView {
    pub level: LevelFilter,
    pub query: String,
    /// The sequence number the view is frozen at; `None` is live.
    ///
    /// A watermark rather than a copy: resuming is a single assignment, and the
    /// count of lines that arrived while paused falls out of the same number.
    pub paused_at: Option<u64>,
    /// Keep the newest line in view. Cleared when the user scrolls up.
    pub follow: bool,
    pub drawer_open: bool,
}

impl Default for LogsView {
    fn default() -> Self {
        Self {
            level: LevelFilter::All,
            query: String::new(),
            paused_at: None,
            follow: true,
            drawer_open: false,
        }
    }
}

impl LogsView {
    pub fn is_paused(&self) -> bool {
        self.paused_at.is_some()
    }

    /// Freeze at `latest_seq`, or resume.
    pub fn toggle_pause(&mut self, latest_seq: u64) {
        self.paused_at = match self.paused_at {
            Some(_) => None,
            None => Some(latest_seq),
        };
    }

    /// How many lines arrived while paused.
    ///
    /// `tracing` sequence numbers advance for every emitted event, including one
    /// the buffer evicted, so this counts what was emitted, not what is still
    /// resident — which is what the user means by "new".
    pub fn new_since(&self, latest_seq: u64) -> u64 {
        self.paused_at
            .map_or(0, |seq| latest_seq.saturating_sub(seq))
    }

    /// The lines the screen shows, oldest first.
    pub fn visible<'a>(&self, lines: &'a [LogLine]) -> Vec<&'a LogLine> {
        let needle = self.query.trim().to_lowercase();
        let watermark = self.paused_at;

        lines
            .iter()
            .filter(|line| watermark.is_none_or(|seq| line.seq <= seq))
            .filter(|line| self.level.admits(line.level))
            .filter(|line| needle.is_empty() || matches_query(line, &needle))
            .collect()
    }

    /// Everything visible, as the text **Copy visible** puts on the clipboard.
    pub fn to_plain_text(&self, lines: &[LogLine]) -> String {
        let mut out = String::new();
        for line in self.visible(lines) {
            out.push_str(&line.compact());
            out.push('\n');
        }
        out
    }
}

/// Case-insensitive match against the message, the target, or a field.
fn matches_query(line: &LogLine, needle: &str) -> bool {
    line.message.to_lowercase().contains(needle)
        || line.target.to_lowercase().contains(needle)
        || line.fields.iter().any(|(key, value)| {
            key.to_lowercase().contains(needle) || value.to_lowercase().contains(needle)
        })
}

#[cfg(test)]
mod tests {
    use chrono::Local;

    use super::*;

    fn line(seq: u64, level: Level, message: &str, fields: Vec<(&str, &str)>) -> LogLine {
        LogLine {
            seq,
            timestamp: Local::now(),
            level,
            target: "career_ops_core::test",
            message: message.into(),
            fields: fields
                .into_iter()
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect(),
        }
    }

    fn sample() -> Vec<LogLine> {
        vec![
            line(1, Level::INFO, "page read", vec![("page", "1")]),
            line(
                2,
                Level::WARN,
                "fetch-all chunk",
                vec![("stop", "fetch-failed")],
            ),
            line(3, Level::ERROR, "panic", Vec::new()),
        ]
    }

    #[test]
    fn a_level_chip_admits_its_level_and_everything_more_severe() {
        assert!(LevelFilter::Warn.admits(Level::ERROR));
        assert!(LevelFilter::Warn.admits(Level::WARN));
        assert!(!LevelFilter::Warn.admits(Level::INFO));
        assert!(LevelFilter::All.admits(Level::TRACE));
        assert!(LevelFilter::Trace.admits(Level::TRACE));
        assert!(!LevelFilter::Error.admits(Level::WARN));
    }

    #[test]
    fn the_default_view_shows_everything_and_follows() {
        let view = LogsView::default();
        assert_eq!(view.visible(&sample()).len(), 3);
        assert!(view.follow);
        assert!(!view.is_paused());
    }

    #[test]
    fn a_level_filter_narrows_the_list() {
        let lines = sample();
        let mut view = LogsView {
            level: LevelFilter::Warn,
            ..Default::default()
        };

        let visible = view.visible(&lines);
        assert_eq!(visible.len(), 2);
        assert!(visible.iter().all(|line| line.level <= Level::WARN));

        view.level = LevelFilter::All;
        assert_eq!(view.visible(&lines).len(), 3);
    }

    #[test]
    fn a_query_matches_the_message_the_target_or_a_field() {
        let mut view = LogsView {
            query: "page".into(),
            ..Default::default()
        };
        assert_eq!(view.visible(&sample()).len(), 1);

        view.query = "fetch-failed".into();
        assert_eq!(view.visible(&sample()).len(), 1);

        view.query = "CAREER_OPS_CORE".into();
        assert_eq!(
            view.visible(&sample()).len(),
            3,
            "the target matches case-insensitively"
        );

        view.query = "  ".into();
        assert_eq!(
            view.visible(&sample()).len(),
            3,
            "whitespace is not a query"
        );
    }

    #[test]
    fn pause_freezes_the_list_at_the_watermark_and_counts_what_arrived() {
        let mut view = LogsView::default();
        let mut lines = sample();

        view.toggle_pause(2);
        assert!(view.is_paused());
        assert_eq!(
            view.visible(&lines).len(),
            2,
            "line 3 is after the watermark"
        );
        assert_eq!(view.new_since(5), 3);

        lines.push(line(4, Level::INFO, "later", Vec::new()));
        assert!(
            !view
                .visible(&lines)
                .iter()
                .any(|line| line.message == "later"),
            "a paused view does not move"
        );

        view.toggle_pause(0);
        assert!(!view.is_paused());
        assert_eq!(view.visible(&lines).len(), 4);
        assert_eq!(view.new_since(5), 0);
    }

    #[test]
    fn copy_takes_the_visible_lines_only() {
        let view = LogsView {
            level: LevelFilter::Warn,
            ..Default::default()
        };

        let text = view.to_plain_text(&sample());

        assert!(text.contains("fetch-all chunk"), "{text}");
        assert!(text.contains("panic"), "{text}");
        assert!(
            !text.contains("page read"),
            "a hidden line must not be copied: {text}"
        );
        assert_eq!(text.lines().count(), 2);
    }
}
