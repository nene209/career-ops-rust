//! Fallback backend tests.
//!
//! The compile-time proof is that this file builds only when
//! `--features chromium-fallback` is enabled, and that
//! `ChromiumoxideBrowserBackend` implements the same `BrowserBackend`
//! trait as `ObscuraBrowserBackend`. The live test needs a Chrome
//! installation and is environment-gated.

#![cfg(feature = "chromium-fallback")]

use career_ops_core::backend::{
    BrowserBackend, ChromiumConfig, ChromiumoxideBrowserBackend, RenderOpts,
};
use url::Url;

#[test]
fn chromium_backend_implements_trait() {
    // Compile-time proof: the type is object-safe behind the trait.
    let backend: Box<dyn BrowserBackend> = Box::new(ChromiumoxideBrowserBackend::with_defaults());
    assert_eq!(backend.name(), "chromiumoxide");
}

#[tokio::test]
async fn chromium_backend_fetches_rendered_dom() {
    if std::env::var("RUN_CHROMIUM_FALLBACK_TESTS").is_err() {
        eprintln!("skipping: RUN_CHROMIUM_FALLBACK_TESTS not set");
        return;
    }

    // Each test gets its own Chrome profile. Without this, parallel
    // tests race for the same SingletonLock and one of them aborts
    // with RESULT_CODE_PROFILE_IN_USE.
    let profile = tempfile::tempdir().expect("profile dir");
    let backend = ChromiumoxideBrowserBackend::new(ChromiumConfig {
        user_data_dir: Some(profile.path().to_path_buf()),
        ..Default::default()
    });

    let url = Url::parse("https://github.com/nene209/career-ops-rust").unwrap();
    let page = backend
        .fetch_rendered(&url, RenderOpts::default())
        .await
        .expect("fetch_rendered");

    assert!(!page.html.is_empty());
    assert!(page.html.contains("career-ops-rust") || page.html.contains("nene209"));

    backend.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn chromium_backend_produces_pdf() {
    if std::env::var("RUN_CHROMIUM_FALLBACK_TESTS").is_err() {
        eprintln!("skipping: RUN_CHROMIUM_FALLBACK_TESTS not set");
        return;
    }

    // Each test gets its own Chrome profile. Without this, parallel
    // tests race for the same SingletonLock and one of them aborts
    // with RESULT_CODE_PROFILE_IN_USE.
    let profile = tempfile::tempdir().expect("profile dir");
    let backend = ChromiumoxideBrowserBackend::new(ChromiumConfig {
        user_data_dir: Some(profile.path().to_path_buf()),
        ..Default::default()
    });
    let bytes = backend
        .print_to_pdf(
            "<html><body><h1>Test</h1></body></html>",
            Default::default(),
        )
        .await
        .expect("print_to_pdf");

    assert!(bytes.starts_with(b"%PDF"));
    backend.shutdown().await.expect("shutdown");
}

/// Full lifecycle on a single profile.
///
/// Covers four properties the per-operation tests do not:
///
/// 1. **State is reused within one backend.** The pdf call between two
///    fetches goes through the same `ensure_started` guard. If reuse
///    were broken, the second operation would spawn a second Chrome on
///    the same profile while the first is still running, and Chrome
///    would abort with `RESULT_CODE_PROFILE_IN_USE` — the exact failure
///    this test would catch.
/// 2. **A PDF render does not corrupt subsequent fetches.** The second
///    fetch has to return non-empty HTML, not just succeed.
/// 3. **Shutdown releases the profile.** A new backend constructed on
///    the same directory after shutdown must be able to launch.
/// 4. **The trait object is the production shape.** The first phase
///    holds the backend as `Box<dyn BrowserBackend>`, which is what the
///    router would do.
#[tokio::test]
async fn chromium_backend_integrated_lifecycle() {
    if std::env::var("RUN_CHROMIUM_FALLBACK_TESTS").is_err() {
        eprintln!("skipping: RUN_CHROMIUM_FALLBACK_TESTS not set");
        return;
    }

    let profile_dir = tempfile::tempdir().expect("profile dir");
    let config = ChromiumConfig {
        user_data_dir: Some(profile_dir.path().to_path_buf()),
        ..Default::default()
    };
    let url = Url::parse("https://github.com/nene209/career-ops-rust").unwrap();

    // ---- Phase A: one backend, sequential fetch → pdf → fetch ----
    {
        let backend: Box<dyn BrowserBackend> =
            Box::new(ChromiumoxideBrowserBackend::new(config.clone()));

        // First fetch: launches Chrome and navigates.
        let page1 = backend
            .fetch_rendered(&url, RenderOpts::default())
            .await
            .expect("first fetch");
        assert!(!page1.html.is_empty(), "first fetch returned empty HTML");
        assert!(
            page1.html.contains("career-ops-rust") || page1.html.contains("nene209"),
            "first fetch did not reach the target page"
        );

        // PDF render on the same backend. This exercises `ensure_started`
        // with state already present — the reuse path.
        let pdf = backend
            .print_to_pdf(
                "<html><body><h1>Lifecycle</h1></body></html>",
                Default::default(),
            )
            .await
            .expect("print_to_pdf");
        assert!(pdf.starts_with(b"%PDF"), "pdf is not a PDF file");
        assert!(
            pdf.len() > 1000,
            "pdf is suspiciously small ({} bytes) — Chrome may not have laid out the page",
            pdf.len()
        );

        // Second fetch: proves the pdf render did not leave the backend
        // in a state where navigation fails.
        let page2 = backend
            .fetch_rendered(&url, RenderOpts::default())
            .await
            .expect("second fetch");
        assert!(!page2.html.is_empty(), "second fetch returned empty HTML");

        backend.shutdown().await.expect("shutdown");
    }

    // ---- Phase B: same profile, new backend — proves shutdown ----
    //
    // `Browser.close()` sends the CDP command and awaits the response,
    // but does not guarantee the Chrome process has fully exited. On a
    // loaded machine the process may still hold `SingletonLock` for a
    // brief window after `shutdown()` returns. Retry the launch until
    // the profile is reusable or the timeout elapses. A pass proves
    // shutdown released everything; a failure after the timeout proves
    // it did not.
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(15);
    let mut attempts: u32 = 0;
    let page3 = loop {
        attempts += 1;
        let backend = ChromiumoxideBrowserBackend::new(config.clone());
        match backend.fetch_rendered(&url, RenderOpts::default()).await {
            Ok(p) => {
                backend.shutdown().await.ok();
                break p;
            }
            Err(e) if tokio::time::Instant::now() < deadline => {
                tracing::debug!(
                    attempt = attempts,
                    error = %e,
                    "profile not yet released; retrying"
                );
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
            Err(e) => {
                panic!(
                    "profile was not reusable after shutdown: \
                     failed on attempt {attempts} over {deadline:?}: {e}"
                );
            }
        }
    };
    assert!(
        !page3.html.is_empty(),
        "post-shutdown fetch returned empty HTML"
    );
}
