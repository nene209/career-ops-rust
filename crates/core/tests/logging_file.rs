//! The file sink, in a process of its own.
//!
//! `init` installs a process-global subscriber, so this cannot share a test
//! binary with the buffer-only tests in `logging.rs`: whichever ran first would
//! decide the mode for the other. Here the first call *is* [`LogMode::File`],
//! which is what `main` does.

use std::path::PathBuf;

use career_ops_core::logging::{self, LogMode, LOG_DIR, LOG_FILE_NAME};

#[test]
fn the_file_sink_installs_and_receives_events() {
    // `data_dir()` reads these, and the file policy reads `RUST_LOG`: clear it
    // so an outer environment cannot turn an `info` line into a filtered one.
    std::env::remove_var("RUST_LOG");

    let dir = tempfile::tempdir().expect("tempdir");
    std::env::set_var("CAREER_OPS_DATA_DIR", dir.path());

    let handle = logging::init(LogMode::File);

    let expected: PathBuf = dir.path().join(LOG_DIR).join(LOG_FILE_NAME);
    assert_eq!(
        handle.path(),
        Some(expected.as_path()),
        "file mode names the log under the data dir"
    );
    assert!(expected.exists(), "the log file is created at startup");

    tracing::info!(
        target: "career_ops_core::test",
        page = 7,
        url = "https://example.com/jobs",
        "file-marker"
    );

    // The `fmt` layer writes on the emitting thread, and the writer flushes
    // every line, so the content is readable immediately.
    let text = std::fs::read_to_string(&expected).expect("the log file is readable");
    assert!(text.contains("file-marker"), "{text}");
    assert!(text.contains("INFO"), "{text}");
    assert!(text.contains("career_ops_core::test"), "{text}");
    assert!(text.contains("page=7"), "{text}");
    assert!(text.contains("https://example.com/jobs"), "{text}");

    assert!(
        handle
            .snapshot()
            .iter()
            .any(|line| line.message == "file-marker"),
        "the same event reaches the in-app buffer"
    );
    assert_eq!(handle.policy(), logging::CapturePolicy::Default);
}
