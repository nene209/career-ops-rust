//! Backend trait tests.
//!
//! These tests pin the two-backend architecture at the test level. The
//! architecture is documented in §4a and §6 of the main plan: `BrowserBackend`
//! is for scraping (Obscura), `PdfBackend` is for rendering
//! (`rust-headless-chrome`), and `BrowserBackend::print_to_pdf` may return
//! `BackendUnsupported`.
//!
//! If a future refactor makes `print_to_pdf` mandatory — for example, by
//! removing the default implementation — these tests fail loudly. That is
//! the intent.

use career_ops_core::backend::{
    BrowserBackend, MockBrowserBackend, MockPdfBackend, PdfBackend, PdfOptions, RenderOpts,
    RenderedPage,
};
use career_ops_core::error::CoreError;
use url::Url;

fn canned_page() -> RenderedPage {
    RenderedPage {
        final_url: Url::parse("https://example.com/jobs/123").unwrap(),
        html: "<html><body><h1>Senior Engineer</h1></body></html>".to_string(),
        status: 200,
    }
}

#[tokio::test]
async fn browser_backend_returns_canned_page() {
    let backend = MockBrowserBackend {
        canned: canned_page(),
        pdf_unsupported: false,
    };

    let url = Url::parse("https://example.com/jobs/123").unwrap();
    let page = backend
        .fetch_rendered(&url, RenderOpts::default())
        .await
        .expect("fetch_rendered");

    assert_eq!(page.status, 200);
    assert_eq!(page.final_url.as_str(), "https://example.com/jobs/123");
    assert!(page.html.contains("Senior Engineer"));
}

#[tokio::test]
async fn browser_backend_pdf_returns_bytes_when_supported() {
    let backend = MockBrowserBackend {
        canned: canned_page(),
        pdf_unsupported: false,
    };

    let bytes = backend
        .print_to_pdf("<html></html>", PdfOptions::default())
        .await
        .expect("print_to_pdf");
    assert_eq!(bytes, b"%PDF-mock");
}

/// The load-bearing test. Obscura cannot render PDFs, so
/// `BrowserBackend::print_to_pdf` must be able to return `BackendUnsupported`.
/// If a future refactor changes the default implementation to `todo!()` or
/// makes the method required (no default), this test breaks.
#[tokio::test]
async fn browser_backend_pdf_returns_unsupported_when_configured() {
    let backend = MockBrowserBackend {
        canned: canned_page(),
        pdf_unsupported: true,
    };

    let result = backend
        .print_to_pdf("<html></html>", PdfOptions::default())
        .await;

    match result {
        Err(CoreError::BackendUnsupported(_)) => {} // expected
        Err(other) => panic!("expected BackendUnsupported, got {other:?}"),
        Ok(_) => panic!("print_to_pdf must not silently produce bytes when unsupported"),
    }
}

/// The trait object must be usable behind `dyn`. This is what allows the
/// application to hold a `Box<dyn BrowserBackend>` and swap Obscura for
/// `rust-headless-chrome` at runtime.
#[tokio::test]
async fn browser_backend_is_object_safe() {
    let backend: Box<dyn BrowserBackend> = Box::new(MockBrowserBackend {
        canned: canned_page(),
        pdf_unsupported: true,
    });

    assert_eq!(backend.name(), "mock");

    let url = Url::parse("https://example.com/").unwrap();
    let page = backend
        .fetch_rendered(&url, RenderOpts::default())
        .await
        .unwrap();
    assert_eq!(page.status, 200);

    // And the unsupported path is reachable through the trait object.
    match backend.print_to_pdf("", PdfOptions::default()).await {
        Err(CoreError::BackendUnsupported(_)) => {}
        other => panic!("expected BackendUnsupported, got {other:?}"),
    }
}

#[tokio::test]
async fn pdf_backend_returns_deterministic_bytes() {
    let backend = MockPdfBackend;

    let a = backend
        .render_pdf("<html></html>", PdfOptions::default())
        .await
        .expect("render_pdf");
    let b = backend
        .render_pdf("<html></html>", PdfOptions::default())
        .await
        .expect("render_pdf");

    assert_eq!(a, b);
    assert_eq!(backend.name(), "mock");
    assert!(!backend.requires_browser());
}

#[tokio::test]
async fn pdf_backend_is_object_safe() {
    let backend: Box<dyn PdfBackend> = Box::new(MockPdfBackend);
    assert!(!backend.requires_browser());

    let bytes = backend
        .render_pdf("<html></html>", PdfOptions::default())
        .await
        .expect("render_pdf");
    assert!(!bytes.is_empty());
}
