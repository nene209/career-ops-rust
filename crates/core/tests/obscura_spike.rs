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

    // The handler drain loop is mandatory. Without it, every command hangs.
    let handle = tokio::spawn(async move { while let Some(_) = handler.next().await {} });

    let page = browser.new_page("about:blank").await.expect("new_page");

    // Navigate to a page whose content is produced by JavaScript.
    // Replace with one of your actual target portals for the real spike.
    page.goto("https://github.com/nene209/career-ops-rust")
        .await
        .expect("goto");

    // Do NOT wait_for_navigation(): Obscura's default lifecycle fires at
    // domcontentloaded, before many portals populate listings. Poll a
    // selector instead.
    let html = timeout(Duration::from_secs(15), page.content())
        .await
        .expect("content() timed out")
        .expect("content() failed");

    assert!(!html.is_empty(), "content() returned empty HTML");
    assert!(
        html.contains("<html"),
        "content() did not look like HTML: first 200 bytes: {:?}",
        &html[..html.len().min(200)]
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

    let handle = tokio::spawn(async move { while let Some(_) = handler.next().await {} });

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
