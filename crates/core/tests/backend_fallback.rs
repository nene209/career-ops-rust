//! Fallback backend tests.
//!
//! The compile-time proof is that this file builds only when
//! `--features chromium-fallback` is enabled, and that
//! `ChromiumoxideBrowserBackend` implements the same `BrowserBackend`
//! trait as `ObscuraBrowserBackend`. The live test needs a Chrome
//! installation and is environment-gated.

#![cfg(feature = "chromium-fallback")]

use career_ops_core::backend::{BrowserBackend, ChromiumoxideBrowserBackend, RenderOpts};
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

    let backend = ChromiumoxideBrowserBackend::with_defaults();
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

    let backend = ChromiumoxideBrowserBackend::with_defaults();
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
