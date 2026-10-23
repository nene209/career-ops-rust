//! Backend traits per §4a and §6 of the main plan.
//!
//! The two traits are intentionally separate:
//!   - `BrowserBackend` is for scraping (Obscura).
//!   - `PdfBackend` is for rendering (rust-headless-chrome).
//!
//! `BrowserBackend::print_to_pdf` exists for interface completeness but may
//! return `BackendUnsupported` — Obscura does not render PDFs.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::error::{CoreError, Result};

pub mod obscura;

#[cfg(feature = "chromium-fallback")]
pub mod chromium;

pub use obscura::{ObscuraBrowserBackend, ObscuraConfig};

#[cfg(feature = "chromium-fallback")]
pub use chromium::{ChromiumConfig, ChromiumoxideBrowserBackend};

#[derive(Debug, Clone, Default)]
pub struct RenderOpts {
    pub wait_for_selector: Option<String>,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone)]
pub struct RenderedPage {
    pub final_url: Url,
    pub html: String,
    pub status: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PdfOptions {
    pub page_width_mm: f32,
    pub page_height_mm: f32,
    pub margin_mm: f32,
    pub print_background: bool,
    pub prefer_css_page_size: bool,
}

impl Default for PdfOptions {
    fn default() -> Self {
        Self {
            page_width_mm: 210.0, // A4
            page_height_mm: 297.0,
            margin_mm: 10.0,
            print_background: false,
            prefer_css_page_size: false,
        }
    }
}

#[async_trait]
pub trait BrowserBackend: Send + Sync {
    async fn fetch_rendered(&self, url: &Url, opts: RenderOpts) -> Result<RenderedPage>;

    async fn print_to_pdf(&self, _html: &str, _opts: PdfOptions) -> Result<Vec<u8>> {
        Err(CoreError::BackendUnsupported(
            "print_to_pdf not implemented",
        ))
    }

    fn name(&self) -> &'static str;

    /// Gracefully shut down the backend. Default is a no-op.
    ///
    /// Implementations that own a subprocess should send SIGTERM, wait
    /// for exit, and escalate to SIGKILL after a timeout. Called by
    /// `AppState` on application exit.
    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }
}

#[async_trait]
pub trait PdfBackend: Send + Sync {
    async fn render_pdf(&self, html: &str, opts: PdfOptions) -> Result<Vec<u8>>;
    fn name(&self) -> &'static str;
    fn requires_browser(&self) -> bool;
}

// -------------------- mocks --------------------

pub struct MockBrowserBackend {
    pub canned: RenderedPage,
    pub pdf_unsupported: bool,
}

#[async_trait]
impl BrowserBackend for MockBrowserBackend {
    async fn fetch_rendered(&self, _url: &Url, _opts: RenderOpts) -> Result<RenderedPage> {
        Ok(self.canned.clone())
    }

    async fn print_to_pdf(&self, _html: &str, _opts: PdfOptions) -> Result<Vec<u8>> {
        if self.pdf_unsupported {
            Err(CoreError::BackendUnsupported(
                "mock: print_to_pdf not supported",
            ))
        } else {
            Ok(b"%PDF-mock".to_vec())
        }
    }

    fn name(&self) -> &'static str {
        "mock"
    }
}

pub struct MockPdfBackend;

#[async_trait]
impl PdfBackend for MockPdfBackend {
    async fn render_pdf(&self, _html: &str, _opts: PdfOptions) -> Result<Vec<u8>> {
        Ok(b"%PDF-mock".to_vec())
    }
    fn name(&self) -> &'static str {
        "mock"
    }
    fn requires_browser(&self) -> bool {
        false
    }
}
