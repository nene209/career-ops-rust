//! Chromium-backed `BrowserBackend`, using `chromiumoxide`.
//!
//! This is the fallback backend: it launches or connects to a real
//! Chrome/Chromium instance and drives it over CDP. Unlike
//! `ObscuraBrowserBackend`, it can produce PDFs (`Page::pdf`), because
//! Chrome has a layout and paint engine.
//!
//! ## Why it exists
//!
//! The trait abstraction is what makes the sidecar model portable. This
//! module proves it: swapping the concrete backend is a new trait impl,
//! not a rewrite of the router or the acquisition pipeline.
//!
//! ## Feature-gated
//!
//! Compiled only under `--features chromium-fallback`. The default
//! build uses Obscura.

use std::time::Duration;

use async_trait::async_trait;
use chromiumoxide::browser::Browser;
use futures_util::StreamExt;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use url::Url;

use super::{BrowserBackend, PdfOptions, RenderOpts, RenderedPage};
use crate::error::{CoreError, Result};

#[derive(Debug, Clone)]
pub struct ChromiumConfig {
    /// `true` launches a bundled Chrome; `false` connects to an
    /// already-running instance at `connect_url`.
    pub launch: bool,
    pub connect_url: Option<String>,
    pub settle_delay: Duration,
}

impl Default for ChromiumConfig {
    fn default() -> Self {
        Self {
            launch: true,
            connect_url: None,
            settle_delay: Duration::from_millis(500),
        }
    }
}

struct ChromiumState {
    browser: Browser,
    handler: JoinHandle<()>,
}

pub struct ChromiumoxideBrowserBackend {
    config: ChromiumConfig,
    state: Mutex<Option<ChromiumState>>,
}

impl ChromiumoxideBrowserBackend {
    pub fn new(config: ChromiumConfig) -> Self {
        Self {
            config,
            state: Mutex::new(None),
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(ChromiumConfig::default())
    }

    async fn ensure_started(&self) -> Result<tokio::sync::MutexGuard<'_, Option<ChromiumState>>> {
        let mut guard = self.state.lock().await;
        if guard.is_some() {
            return Ok(guard);
        }

        let (browser, mut handler) = if self.config.launch {
            Browser::launch(
                chromiumoxide::browser::BrowserConfig::builder()
                    .build()
                    .map_err(|e| CoreError::Message(format!("chromium launch config: {e}")))?,
            )
            .await
            .map_err(|e| CoreError::Message(format!("chromium launch failed: {e}")))?
        } else {
            let url = self.config.connect_url.as_deref().ok_or_else(|| {
                CoreError::Message("connect_url required when launch=false".into())
            })?;
            Browser::connect(url)
                .await
                .map_err(|e| CoreError::Message(format!("chromium connect failed: {e}")))?
        };

        let handler_task = tokio::spawn(async move { while handler.next().await.is_some() {} });

        *guard = Some(ChromiumState {
            browser,
            handler: handler_task,
        });
        Ok(guard)
    }
}

#[async_trait]
impl BrowserBackend for ChromiumoxideBrowserBackend {
    async fn fetch_rendered(&self, url: &Url, opts: RenderOpts) -> Result<RenderedPage> {
        let mut guard = self.ensure_started().await?;
        let state = guard.as_mut().expect("ensure_started guarantees Some");

        let page = state
            .browser
            .new_page(url.as_str())
            .await
            .map_err(|e| CoreError::Message(format!("new_page failed: {e}")))?;

        if let Some(selector) = opts.wait_for_selector.as_deref() {
            let timeout = if opts.timeout_ms == 0 {
                15_000
            } else {
                opts.timeout_ms
            };
            wait_for_selector(&page, selector, Duration::from_millis(timeout)).await?;
        } else {
            tokio::time::sleep(self.config.settle_delay).await;
        }

        let html = page
            .content()
            .await
            .map_err(|e| CoreError::Message(format!("content failed: {e}")))?;

        let final_url = page
            .url()
            .await
            .map_err(|e| CoreError::Message(format!("page.url failed: {e}")))?
            .and_then(|s| Url::parse(&s).ok())
            .unwrap_or_else(|| url.clone());

        Ok(RenderedPage {
            final_url,
            html,
            status: 200,
        })
    }

    async fn print_to_pdf(&self, html: &str, _opts: PdfOptions) -> Result<Vec<u8>> {
        let mut guard = self.ensure_started().await?;
        let state = guard.as_mut().expect("ensure_started guarantees Some");

        let page = state
            .browser
            .new_page("about:blank")
            .await
            .map_err(|e| CoreError::Message(format!("new_page for pdf failed: {e}")))?;

        page.set_content(html)
            .await
            .map_err(|e| CoreError::Message(format!("set_content failed: {e}")))?;

        // Chrome needs a moment to lay out before printToPDF is meaningful.
        tokio::time::sleep(Duration::from_millis(200)).await;

        let bytes = page
            .pdf(chromiumoxide::cdp::browser_protocol::page::PrintToPdfParams::default())
            .await
            .map_err(|e| CoreError::Message(format!("pdf failed: {e}")))?;

        Ok(bytes)
    }

    fn name(&self) -> &'static str {
        "chromiumoxide"
    }

    async fn shutdown(&self) -> Result<()> {
        let mut guard = self.state.lock().await;
        if let Some(mut state) = guard.take() {
            let _ = state.browser.close().await;
            state.handler.abort();
        }
        Ok(())
    }
}

async fn wait_for_selector(
    page: &chromiumoxide::Page,
    selector: &str,
    timeout: Duration,
) -> Result<()> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let found: bool = page
            .evaluate(format!(
                "document.querySelectorAll({selector:?}).length > 0"
            ))
            .await
            .map_err(|e| CoreError::Message(format!("evaluate failed: {e}")))?
            .into_value()
            .unwrap_or(false);
        if found {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(CoreError::Message(format!(
                "selector {selector:?} not found within {timeout:?}"
            )));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
