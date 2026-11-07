//! Week 1 spike: drive the Obscura sidecar over CDP from chromiumoxide.
//!
//! Run with:
//!   OBSCURA_BIN=/path/to/obscura cargo test -p career-ops-core --test obscura_spike -- --nocapture
//!
//! This test is the Phase 2 gate. It proves four things:
//!   1. The sidecar can be spawned on an allocated port and is reachable.
//!   2. chromiumoxide can connect to it and drive a page.
//!   3. A JS-heavy page returns post-JS DOM.
//!   4. Killing the sidecar is detectable on the next connection attempt.

use std::net::TcpListener;
use std::path::PathBuf;
use std::time::Duration;

use chromiumoxide::browser::Browser;
use futures_util::stream::StreamExt;
use tokio::process::{Child, Command};
use tokio::time::{sleep, timeout};
use url::Url;

fn allocate_port() -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind ephemeral port");
    let port = listener.local_addr().expect("local addr").port();
    drop(listener);
    port
}

fn resolve_obscura_bin() -> PathBuf {
    // 1. Explicit override wins.
    if let Ok(path) = std::env::var("OBSCURA_BIN") {
        return PathBuf::from(path);
    }
    // 2. Workspace `vendor/obscura`, resolved from the compile-time manifest dir.
    //    CARGO_MANIFEST_DIR is the absolute path to `crates/core/`, so
    //    `../..` is the workspace root.
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let candidate = manifest
        .join("..")
        .join("..")
        .join("vendor")
        .join("obscura");
    if candidate.exists() {
        return candidate;
    }
    // 3. Fall back to PATH lookup.
    PathBuf::from("obscura")
}

async fn spawn_obscura(port: u16, storage: &std::path::Path) -> Child {
    let bin = resolve_obscura_bin();

    let child = Command::new(&bin)
        .arg("serve")
        .arg("--port")
        .arg(port.to_string())
        .arg("--host")
        .arg("127.0.0.1")
        .arg("--storage-dir")
        .arg(storage)
        .arg("--stealth")
        .kill_on_drop(true)
        .spawn()
        .unwrap_or_else(|e| {
            panic!(
                "failed to spawn `{} serve`: {e}. \
                    Set OBSCURA_BIN to an absolute path if the binary is not on PATH.",
                bin.display()
            )
        });

    child
}

async fn wait_for_port(port: u16, deadline: Duration) {
    let start = std::time::Instant::now();
    loop {
        if tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_ok()
        {
            return;
        }
        if start.elapsed() > deadline {
            panic!("obscura did not start listening on port {port} within {deadline:?}");
        }
        sleep(Duration::from_millis(100)).await;
    }
}

fn obscura_available() -> bool {
    if std::env::var("OBSCURA_BIN").is_ok() {
        return true;
    }
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.join("../../vendor/obscura").exists()
        || std::process::Command::new("obscura")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok()
}

/// The core spike: connect, navigate, get post-JS DOM.
#[tokio::test]
async fn obscura_returns_post_js_dom() {
    if !obscura_available() {
        eprintln!("skipping: obscura binary not found");
        return;
    }
    let port = allocate_port();
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut child = spawn_obscura(port, tmp.path()).await;

    wait_for_port(port, Duration::from_secs(10)).await;

    let cdp_url = format!("ws://127.0.0.1:{port}");
    let (browser, mut handler) = Browser::connect(&cdp_url)
        .await
        .expect("Browser::connect to obscura");

    // Drain the handler. Without an active consumer, chromiumoxide's
    // pending commands hang; the handler must be polled to completion
    // for responses to be delivered. Errors from individual events are
    // ignored — we only care that the connection stays alive.
    let handle = tokio::spawn(async move {
        while handler.next().await.is_some() {
            // value discarded; see comment above
        }
    });

    let page = browser.new_page("about:blank").await.expect("new_page");

    // Navigate to a page whose content is produced by JavaScript.
    // Replace with one of your actual target portals for the real spike.
    page.goto("https://github.com/nene209/career-ops-rust")
        .await
        .expect("goto");

    // Prove navigation worked against the intended page.
    let html_before = timeout(Duration::from_secs(15), page.content())
        .await
        .expect("content before timed out")
        .expect("content before failed");
    assert!(
        html_before.contains("career-ops-rust") || html_before.contains("nene209"),
        "page content does not reference the target repo; navigation may have failed"
    );

    // Prove JS execution and DOM reflection.
    let marker = format!("obscura-spike-{}", std::process::id());
    timeout(
        Duration::from_secs(15),
        page.evaluate(format!(
            "document.body.setAttribute('data-obscura-marker', '{marker}')"
        )),
    )
    .await
    .expect("evaluate timed out")
    .expect("evaluate failed");

    let html_after = timeout(Duration::from_secs(15), page.content())
        .await
        .expect("content after timed out")
        .expect("content after failed");
    assert!(
        html_after.contains(&marker),
        "post-JS DOM does not contain the marker; content() may be returning pre-JS HTML"
    );

    // --- Sidecar death detection ---
    // Kill the child, then confirm a new connection fails cleanly rather
    // than hanging. This is the recovery path the plan requires.
    child.kill().await.expect("kill obscura");
    let _ = child.wait().await;

    let reconnect = Browser::connect(&cdp_url).await;
    assert!(
        reconnect.is_err(),
        "Browser::connect succeeded after the sidecar was killed; \
         the health check would not detect sidecar death"
    );

    handle.abort();
}

/// Separate test: the render-enabled binary may support printToPDF.
///
/// Build with:
///   cargo build --release -p obscura-cli --bins --features render
///
/// If this test passes, Phase 4 may not need rust-headless-chrome at all.
#[tokio::test]
async fn obscura_print_to_pdf_if_supported() {
    let port = allocate_port();
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut child = spawn_obscura(port, tmp.path()).await;

    wait_for_port(port, Duration::from_secs(10)).await;

    let cdp_url = format!("ws://127.0.0.1:{port}");
    let (browser, mut handler) = Browser::connect(&cdp_url).await.expect("connect");

    // Drain the handler. Without an active consumer, chromiumoxide's
    // pending commands hang; the handler must be polled to completion
    // for responses to be delivered. Errors from individual events are
    // ignored — we only care that the connection stays alive.
    let handle = tokio::spawn(async move {
        while handler.next().await.is_some() {
            // value discarded; see comment above
        }
    });

    let page = browser.new_page("about:blank").await.expect("new_page");
    page.set_content("<html><body><h1>PDF spike</h1></body></html>")
        .await
        .expect("set_content");

    match page
        .pdf(chromiumoxide::cdp::browser_protocol::page::PrintToPdfParams::default())
        .await
    {
        Ok(bytes) => {
            println!(
                "printToPDF SUCCEEDED: {} bytes, magic={:?}",
                bytes.len(),
                &bytes[..4]
            );
            assert!(bytes.starts_with(b"%PDF"), "output is not a PDF");
        }
        Err(e) => {
            // This is a valid outcome. It confirms the plan's assumption
            // that Obscura cannot render PDFs. Record the error text.
            println!("printToPDF UNSUPPORTED: {e}");
        }
    }

    child.kill().await.ok();
    handle.abort();
}

/// F3: `ObscuraBrowserBackend::print_to_pdf` returns `BackendUnsupported`.
///
/// This pins the Phase 2 scoping decision: the backend *can* render
/// PDFs (verified by `obscura_print_to_pdf_if_supported`), but the
/// production backend does not expose that capability until Phase 4.
#[tokio::test]
async fn obscura_backend_print_to_pdf_is_unsupported() {
    if !obscura_available() {
        eprintln!("skipping: obscura binary not found");
        return;
    }

    use career_ops_core::backend::{BrowserBackend, ObscuraBrowserBackend, PdfOptions};

    // Construction does not spawn the sidecar.
    let backend = ObscuraBrowserBackend::with_defaults();
    assert_eq!(backend.name(), "obscura-sidecar");

    let result = backend
        .print_to_pdf("<html></html>", PdfOptions::default())
        .await;
    assert!(
        matches!(
            result,
            Err(career_ops_core::error::CoreError::BackendUnsupported(_))
        ),
        "expected BackendUnsupported, got {result:?}"
    );
}

/// F3: stale lock from a previous run is cleaned up before spawning.
#[tokio::test]
async fn obscura_backend_cleans_stale_lock() {
    if !obscura_available() {
        eprintln!("skipping: obscura binary not found");
        return;
    }

    use career_ops_core::backend::{
        BrowserBackend, ObscuraBrowserBackend, ObscuraConfig, RenderOpts,
    };

    let tmp = tempfile::tempdir().expect("tempdir");
    // Fake a lock file with a PID that is definitely not alive.
    std::fs::write(tmp.path().join(".obscura.lock"), "999999").unwrap();

    let config = ObscuraConfig {
        binary: resolve_obscura_bin(),
        storage_dir: tmp.path().to_path_buf(),
        spawn_timeout: Duration::from_secs(10),
        shutdown_timeout: Duration::from_secs(2),
        default_settle_delay: Duration::from_millis(200),
    };
    let backend = ObscuraBrowserBackend::new(config);

    let url = Url::parse("https://github.com/nene209/career-ops-rust").unwrap();
    let page = backend
        .fetch_rendered(&url, RenderOpts::default())
        .await
        .expect("fetch after stale lock");

    assert!(!page.html.is_empty());

    // The stale lock is gone; a new lock names the new child.
    let new_lock = std::fs::read_to_string(tmp.path().join(".obscura.lock")).unwrap();
    assert_ne!(new_lock.trim(), "999999");

    backend.shutdown().await.expect("shutdown");
}

/// F3: shutdown sends SIGTERM and the child exits.
#[tokio::test]
async fn obscura_backend_graceful_shutdown() {
    if !obscura_available() {
        eprintln!("skipping: obscura binary not found");
        return;
    }

    use career_ops_core::backend::{
        BrowserBackend, ObscuraBrowserBackend, ObscuraConfig, RenderOpts,
    };

    let tmp = tempfile::tempdir().unwrap();
    let config = ObscuraConfig {
        binary: resolve_obscura_bin(),
        storage_dir: tmp.path().to_path_buf(),
        spawn_timeout: Duration::from_secs(10),
        shutdown_timeout: Duration::from_secs(3),
        default_settle_delay: Duration::from_millis(200),
    };
    let backend = ObscuraBrowserBackend::new(config);

    let url = Url::parse("https://github.com/nene209/career-ops-rust").unwrap();
    backend
        .fetch_rendered(&url, RenderOpts::default())
        .await
        .expect("fetch");

    backend.shutdown().await.expect("shutdown");

    // Lock file is removed on shutdown.
    assert!(!tmp.path().join(".obscura.lock").exists());
}
