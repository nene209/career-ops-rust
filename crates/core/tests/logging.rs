//! The logging seam, end to end: one event stream, two sinks.
//!
//! These tests share one process-global subscriber, which is the point: `init`
//! must be idempotent, and a second caller must land on the same buffer. They
//! therefore assert on content (unique markers) rather than on the buffer being
//! empty, and they never install the file sink (that is `main`'s job), so
//! `path()` staying `None` is deterministic.
//!
//! The file writer, rotation, and the panic hook are exercised directly: they
//! are standalone, and a per-process subscriber is the wrong tool for testing
//! them.

use std::io::Write;

use career_ops_core::logging::{
    self, install_panic_hook, rotate_if_needed, should_rotate, CapturePolicy, LogBuffer, LogFile,
    LogMode, KEEP_ARCHIVES, LOG_FILE_NAME,
};
use tracing::Level;
use tracing_subscriber::fmt::MakeWriter;

/// A marker unique to this test, so a shared buffer does not need to be empty.
fn marker(name: &str) -> String {
    format!("marker-{name}-{}", std::process::id())
}

#[test]
fn a_line_reaches_the_buffer_with_its_fields() {
    let handle = logging::handle();
    let message = marker("fields");

    tracing::info!(
        target: "career_ops_core::test",
        page = 30,
        jobs = 40,
        url = "https://example.com/jobs",
        "{message}"
    );

    let lines = handle.snapshot();
    let line = lines
        .iter()
        .find(|line| line.message == message)
        .expect("the event reached the buffer");

    assert_eq!(line.level, Level::INFO);
    assert_eq!(line.target, "career_ops_core::test");
    assert!(
        line.seq > 0,
        "sequence numbers start at one so zero can mean 'nothing seen'"
    );
    assert_eq!(line.fields.len(), 3, "{:?}", line.fields);
    assert!(line
        .fields
        .contains(&("page".to_string(), "30".to_string())));
    assert!(line
        .fields
        .contains(&("jobs".to_string(), "40".to_string())));
    assert!(
        line.fields
            .contains(&("url".to_string(), "https://example.com/jobs".to_string())),
        "string fields are recorded verbatim, not Debug-quoted: {:?}",
        line.fields
    );

    let compact = line.compact();
    assert!(compact.contains("INFO"), "{compact}");
    assert!(compact.contains(&message), "{compact}");
    assert!(compact.contains("page=30"), "{compact}");

    // The buffer is ordered, always.
    assert!(
        lines.windows(2).all(|pair| pair[0].seq < pair[1].seq),
        "snapshot order follows the sequence"
    );
}

#[test]
fn the_level_policy_admits_debug_only_for_the_app_targets() {
    let handle = logging::handle();
    if handle.policy() == CapturePolicy::Env {
        // `RUST_LOG` owns the policy; the assertion would be about the
        // environment, not the code.
        return;
    }

    let visible = marker("debug-app-target");
    let hidden = marker("debug-other-target");

    tracing::debug!(target: "career_ops_core::test", "{visible}");
    tracing::debug!(target: "not_career_ops", "{hidden}");

    let lines = handle.snapshot();
    assert!(
        lines.iter().any(|line| line.message == visible),
        "the app's own targets are captured at debug so the level chips have something to reveal"
    );
    assert!(
        !lines.iter().any(|line| line.message == hidden),
        "other targets stay at info, or sqlx and hyper would drown the buffer"
    );
}

/// Replacing `fmt::init` with a manual registry would drop `log`-crate records
/// (some `sqlx` paths emit them) unless the bridge is installed. `try_init`
/// installs it; this test pins that.
#[test]
fn the_log_bridge_carries_log_crate_records() {
    let handle = logging::handle();
    let message = marker("log-bridge");

    log::warn!(target: "career_ops_core::test", "{message}");

    assert!(
        handle.snapshot().iter().any(|line| line.message == message),
        "a `log` record must reach the buffer through the LogTracer bridge"
    );
}

#[test]
fn init_is_idempotent_and_keeps_one_buffer() {
    let first = logging::handle();
    let message = marker("idempotent");
    tracing::info!(target: "career_ops_core::test", "{message}");
    let seq = first.latest_seq();

    let second = logging::init(LogMode::BufferOnly);

    assert_eq!(
        second.latest_seq(),
        seq,
        "the second call returns the installed handle, not a second buffer"
    );
    assert!(second.snapshot().iter().any(|line| line.message == message));

    let after = marker("after-second-init");
    tracing::info!(target: "career_ops_core::test", "{after}");
    assert!(
        first.snapshot().iter().any(|line| line.message == after),
        "both handles see the same buffer"
    );
}

#[test]
fn buffer_only_installs_no_file_sink() {
    assert_eq!(
        logging::handle().path(),
        None,
        "tests must not write into the user's data directory"
    );
}

#[test]
fn a_cap_is_bounded_and_honest_about_what_it_dropped() {
    let buffer = LogBuffer::new(2);
    for index in 0..5 {
        buffer.push(Level::INFO, "test", format!("ring-{index}"), Vec::new());
    }

    let lines = buffer.snapshot();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].message, "ring-3");
    assert_eq!(lines[1].message, "ring-4");
    assert_eq!(buffer.dropped(), 3);
    assert_eq!(buffer.capacity(), 2);
}

#[test]
fn the_file_writer_appends_and_flushes_every_line() {
    let dir = tempfile::tempdir().unwrap();
    let (log, path) = LogFile::open_with(dir.path(), 1 << 20).unwrap();

    {
        let mut writer = log.make_writer();
        writer.write_all(b"one\n").unwrap();
        writer.write_all(b"two\n").unwrap();
        // Deliberately no flush: a crash must not lose the tail.
    }

    assert_eq!(std::fs::read_to_string(&path).unwrap(), "one\ntwo\n");
}

#[test]
fn the_file_writer_appends_across_opens() {
    let dir = tempfile::tempdir().unwrap();

    let (first, path) = LogFile::open_with(dir.path(), 1 << 20).unwrap();
    first.write_line("first\n");
    drop(first);

    let (second, second_path) = LogFile::open_with(dir.path(), 1 << 20).unwrap();
    second.write_line("second\n");

    assert_eq!(second_path, path);
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "first\nsecond\n",
        "reopening appends rather than truncates"
    );
}

#[test]
fn the_rotation_threshold_is_inclusive_and_stable() {
    assert!(!should_rotate(0, 10));
    assert!(!should_rotate(9, 10));
    assert!(should_rotate(10, 10));
    assert!(should_rotate(11, 10));
}

#[test]
fn a_rotation_keeps_the_newest_archives() {
    let dir = tempfile::tempdir().unwrap();
    for day in 1..=7 {
        std::fs::write(
            dir.path()
                .join(format!("career-ops.2026010{day}-000000.000.log")),
            b"x",
        )
        .unwrap();
    }
    std::fs::write(dir.path().join(LOG_FILE_NAME), vec![b'x'; 100]).unwrap();

    let archive = rotate_if_needed(dir.path(), 50).unwrap().expect("rotated");
    assert!(archive.exists());

    let mut names: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();

    assert_eq!(
        names.len(),
        KEEP_ARCHIVES,
        "seven archives plus the new one, pruned to five: {names:?}"
    );
    assert!(
        !names.contains(&"career-ops.20260101-000000.000.log".to_string()),
        "the oldest went first: {names:?}"
    );
    assert!(
        names
            .iter()
            .any(|name| name.starts_with("career-ops.20260104")),
        "the newest of the old archives survive: {names:?}"
    );
    assert!(
        !dir.path().join(LOG_FILE_NAME).exists(),
        "the current file is renamed, not copied"
    );
}

#[test]
fn a_panic_is_written_with_its_location() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("panic.log");
    let previous = std::panic::take_hook();
    install_panic_hook(&path);

    let message = marker("panic");
    let outcome = std::panic::catch_unwind(|| panic!("{message}"));

    std::panic::set_hook(previous);
    assert!(outcome.is_err(), "the hook must not swallow the panic");

    let text = std::fs::read_to_string(&path).expect("the hook wrote the log");
    assert!(text.contains(&message), "{text}");
    assert!(
        text.contains("logging.rs"),
        "the record names the panic location: {text}"
    );
    assert!(text.contains("ERROR"), "{text}");
}
