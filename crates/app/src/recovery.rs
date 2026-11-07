//! Filesystem operations used by the recovery screen.
//!
//! The state machine owns unlocking; these helpers only handle the actions the
//! state machine does not model: opening the data directory, restoring a key
//! file, exporting the encrypted bundle, and the explicit last-resort reset.
//!
//! Every destructive operation refuses when it would overwrite existing data.
//! The reset is the only operation that removes files, and the UI requires the
//! user to type a confirmation phrase before it can be triggered.

use std::ffi::OsStr;
use std::io;
use std::path::Path;

use career_ops_core::keystore::{WrappedDek, DB_FILE, WRAPPED_FILE};

/// Open the data directory in the platform file browser.
pub fn open_data_dir(data_dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(data_dir)?;
    let opener = opener().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::Unsupported,
            "no folder opener is known for this platform",
        )
    })?;
    std::process::Command::new(opener).arg(data_dir).spawn()?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn opener() -> Option<&'static str> {
    Some("open")
}

#[cfg(target_os = "windows")]
fn opener() -> Option<&'static str> {
    Some("explorer")
}

#[cfg(all(unix, not(target_os = "macos")))]
fn opener() -> Option<&'static str> {
    Some("xdg-open")
}

#[cfg(not(any(target_os = "macos", target_os = "windows", unix)))]
fn opener() -> Option<&'static str> {
    None
}

/// Copy a `dek.wrapped.json` into the data directory.
///
/// Refuses if the destination already has a wrapped key. The caller is
/// responsible for reopening the application state afterwards; if that fails,
/// it should remove the copied file again.
pub fn install_key_file(data_dir: &Path, source: &Path) -> io::Result<()> {
    if source.file_name() != Some(OsStr::new(WRAPPED_FILE)) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("select the {WRAPPED_FILE} file"),
        ));
    }
    if !source.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("key file not found: {}", source.display()),
        ));
    }

    let destination = data_dir.join(WRAPPED_FILE);
    if destination.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!(
                "{} already exists. Move it aside before restoring a different key.",
                destination.display()
            ),
        ));
    }

    // Validate the bundle before copying it into the data directory. A JSON
    // document that is not a wrapped-key bundle would otherwise leave the app
    // in a state where it sees a key but cannot use it.
    let raw = std::fs::read_to_string(source)?;
    let wrapped: WrappedDek = serde_json::from_str(&raw).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("not a wrapped-key bundle: {error}"),
        )
    })?;
    if wrapped.wraps.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the key file contains no key wraps",
        ));
    }

    std::fs::create_dir_all(data_dir)?;
    std::fs::copy(source, &destination)?;
    Ok(())
}

/// Copy `career-ops.db` and `dek.wrapped.json` into `destination`.
///
/// Refuses if either destination file already exists, so an export cannot
/// silently overwrite an older backup. If the second copy fails, the first is
/// removed again.
pub fn export_database(data_dir: &Path, destination: &Path) -> io::Result<()> {
    let database = data_dir.join(DB_FILE);
    let wrapped = data_dir.join(WRAPPED_FILE);

    if !database.is_file() || !wrapped.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "{} must contain both {DB_FILE} and {WRAPPED_FILE}",
                data_dir.display()
            ),
        ));
    }

    std::fs::create_dir_all(destination)?;

    let database_destination = destination.join(DB_FILE);
    let wrapped_destination = destination.join(WRAPPED_FILE);

    for path in [&database_destination, &wrapped_destination] {
        if path.exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!(
                    "{} already exists. Choose an empty directory.",
                    path.display()
                ),
            ));
        }
    }

    std::fs::copy(&database, &database_destination)?;
    if let Err(error) = std::fs::copy(&wrapped, &wrapped_destination) {
        let _ = std::fs::remove_file(&database_destination);
        return Err(error);
    }

    Ok(())
}

/// Remove the encrypted database, the wrapped key, and their transient
/// sidecars. Other files in the directory are left alone.
///
/// This is the implementation of the "start fresh" recovery action. It is only
/// called after the UI has required the user to type a confirmation phrase.
pub fn reset_data_dir(data_dir: &Path) -> io::Result<()> {
    let files = [
        DB_FILE,
        WRAPPED_FILE,
        "career-ops.db-wal",
        "career-ops.db-shm",
        "career-ops.db-journal",
        "dek.wrapped.json.tmp",
    ];

    for file in files {
        let path = data_dir.join(file);
        if path.exists() {
            std::fs::remove_file(path)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn install_key_file_refuses_a_different_filename() {
        let dir = tempdir().expect("tempdir");
        let source = dir.path().join("something.json");
        std::fs::write(&source, b"{}").expect("write source");

        let error = install_key_file(dir.path(), &source).expect_err("must refuse");
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn install_key_file_refuses_to_overwrite_an_existing_wrapped_key() {
        let dir = tempdir().expect("tempdir");
        let source = dir.path().join("source");
        std::fs::create_dir_all(&source).expect("source dir");
        let source = source.join(WRAPPED_FILE);
        std::fs::write(&source, b"new").expect("write source");
        std::fs::write(dir.path().join(WRAPPED_FILE), b"existing").expect("write destination");

        let error = install_key_file(dir.path(), &source).expect_err("must refuse");
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    }

    #[test]
    fn install_key_file_copies_the_selected_file() {
        let dir = tempdir().expect("tempdir");
        let source_dir = tempdir().expect("source tempdir");
        let source = source_dir.path().join(WRAPPED_FILE);
        let bundle = r#"{
            "version": 1,
            "wraps": [{
                "method": "keychain",
                "created_at": "2026-01-01T00:00:00Z",
                "nonce": "AAAA",
                "ciphertext": "AAAA"
            }]
        }"#;
        std::fs::write(&source, bundle).expect("write source");

        install_key_file(dir.path(), &source).expect("install");

        let copied = std::fs::read_to_string(dir.path().join(WRAPPED_FILE)).expect("read copied");
        assert_eq!(copied, bundle);
    }

    #[test]
    fn install_key_file_rejects_malformed_json() {
        let dir = tempdir().expect("tempdir");
        let source_dir = tempdir().expect("source tempdir");
        let source = source_dir.path().join(WRAPPED_FILE);
        std::fs::write(&source, b"not json").expect("write source");

        let error = install_key_file(dir.path(), &source).expect_err("must refuse");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn install_key_file_rejects_a_bundle_without_wraps() {
        let dir = tempdir().expect("tempdir");
        let source_dir = tempdir().expect("source tempdir");
        let source = source_dir.path().join(WRAPPED_FILE);
        std::fs::write(&source, br#"{"version":1,"wraps":[]}"#).expect("write source");

        let error = install_key_file(dir.path(), &source).expect_err("must refuse");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn export_copies_both_files() {
        let data_dir = tempdir().expect("data tempdir");
        let destination = tempdir().expect("destination tempdir");
        std::fs::write(data_dir.path().join(DB_FILE), b"database").expect("write db");
        std::fs::write(data_dir.path().join(WRAPPED_FILE), b"key").expect("write key");

        export_database(data_dir.path(), destination.path()).expect("export");

        assert_eq!(
            std::fs::read(destination.path().join(DB_FILE)).expect("read db"),
            b"database"
        );
        assert_eq!(
            std::fs::read(destination.path().join(WRAPPED_FILE)).expect("read key"),
            b"key"
        );
    }

    #[test]
    fn export_refuses_to_overwrite_an_existing_backup() {
        let data_dir = tempdir().expect("data tempdir");
        let destination = tempdir().expect("destination tempdir");
        std::fs::write(data_dir.path().join(DB_FILE), b"database").expect("write db");
        std::fs::write(data_dir.path().join(WRAPPED_FILE), b"key").expect("write key");
        std::fs::write(destination.path().join(DB_FILE), b"older").expect("write existing");

        let error = export_database(data_dir.path(), destination.path()).expect_err("must refuse");
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    }

    #[test]
    fn reset_removes_known_files_and_leaves_others() {
        let dir = tempdir().expect("tempdir");
        for file in [
            DB_FILE,
            WRAPPED_FILE,
            "career-ops.db-wal",
            "career-ops.db-shm",
            "career-ops.db-journal",
            "dek.wrapped.json.tmp",
            "notes.txt",
        ] {
            std::fs::write(dir.path().join(file), b"x").expect("write fixture");
        }

        reset_data_dir(dir.path()).expect("reset");

        for file in [
            DB_FILE,
            WRAPPED_FILE,
            "career-ops.db-wal",
            "career-ops.db-shm",
            "career-ops.db-journal",
            "dek.wrapped.json.tmp",
        ] {
            assert!(!dir.path().join(file).exists(), "{file} should be gone");
        }
        assert!(
            dir.path().join("notes.txt").exists(),
            "unrelated files must be preserved"
        );
    }
}
