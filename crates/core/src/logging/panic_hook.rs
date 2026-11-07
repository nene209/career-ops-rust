//! The panic hook.
//!
//! A panic in a spawned task is otherwise a silent disappearance, which is the
//! worst failure mode for background work: the spinner stops and nothing says
//! why. The hook writes to the log file through a handle of its own, so a mutex
//! poisoned by the very panic it is reporting cannot deadlock the report, and
//! then chains to the previous hook so default behaviour is preserved.

use std::any::Any;
use std::fs::OpenOptions;
use std::io::Write;
use std::panic::{self, PanicHookInfo};
use std::path::Path;

use chrono::Local;

/// Install a hook that appends each panic to `path`, then chains.
pub fn install_panic_hook(path: &Path) {
    let path = path.to_path_buf();
    let previous = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        append(&path, info);
        previous(info);
    }));
}

fn append(path: &Path, info: &PanicHookInfo<'_>) {
    let location = info
        .location()
        .map(|location| {
            format!(
                "{}:{}:{}",
                location.file(),
                location.line(),
                location.column()
            )
        })
        .unwrap_or_else(|| "<unknown location>".to_string());
    let payload = payload_message(info.payload());
    let line = format!(
        "{} ERROR career_ops_core::logging::panic: panic at {location}: {payload}\n",
        Local::now().format("%Y-%m-%dT%H:%M:%S%.3f")
    );

    // A fresh handle, not the shared writer: see the module note.
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(line.as_bytes());
        let _ = file.flush();
    }
    let _ = writeln!(
        std::io::stderr(),
        "career-ops: panic at {location}: {payload}"
    );
}

fn payload_message(payload: &(dyn Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_string()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "<non-string panic payload>".to_string()
    }
}
