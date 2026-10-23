//! Obscura sidecar `BrowserBackend`.
//!
//! Spawns `obscura serve` as a child process on an ephemeral port,
//! connects to it with `chromiumoxide`, and reuses the connection
//! across fetches. The child is killed on drop (`kill_on_drop`), and
//! `shutdown()` sends SIGTERM before SIGKILL for a clean exit.
//!
//! ## Lifecycle
//!
//! - **Lazy spawn.** The sidecar is not started at construction; the
//!   first `fetch_rendered` call spawns it. A user who never fetches a
//!   JS-required portal never pays for the browser.
//! - **Reuse.** Subsequent fetches use the same sidecar. The
//!   `tokio::sync::Mutex` serializes fetches; the router already fetches
//!   sequentially, so this is not a throughput cost.
//! - **Health check.** Before each fetch, `try_wait()` on the child
//!   detects a dead process. If dead, the state is discarded and a new
//!   sidecar is spawned.
//! - **Stale cleanup.** On first spawn, a lock file at
//!   `<storage_dir>/.obscura.lock` identifies a previous run. If the PID
//!   is alive, it is terminated before the new sidecar starts.
//!
//! ## `print_to_pdf`
//!
//! Returns `BackendUnsupported` in Phase 2. Obscura v0.2.5 with the
//! `render` feature can produce PDFs (verified during the Week 1
//! spike), but PDF rendering is a Phase 4 concern. The trait default is
//! inherited rather than overridden.

use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use chromiumoxide::browser::Browser;
use futures_util::StreamExt;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use url::Url;

use super::{BrowserBackend, RenderOpts, RenderedPage};
use crate::error::{CoreError, Result};

#[derive(Debug, Clone)]
pub struct ObscuraConfig {
    /// Path to the `obscura` binary.
    pub binary: PathBuf,
    /// Directory for the browser profile and lock file.
    pub storage_dir: PathBuf,
    /// How long to wait for the CDP port to become reachable.
    pub spawn_timeout: Duration,
    /// How long to wait for graceful exit after SIGTERM.
    pub shutdown_timeout: Duration,
    /// Default page-settle delay when `RenderOpts::wait_for_selector`
    /// is `None`.
    pub default_settle_delay: Duration,
}

impl Default for ObscuraConfig {
    fn default() -> Self {
        Self {
            binary: PathBuf::from("obscura"),
            storage_dir: std::env::temp_dir().join("career-ops-obscura"),
            spawn_timeout: Duration::from_secs(10),
            shutdown_timeout: Duration::from_secs(5),
            default_settle_delay: Duration::from_millis(500),
        }
    }
}

struct SidecarState {
    child: Child,
    browser: Browser,
    handler: JoinHandle<()>,
    #[allow(dead_code)]
    port: u16,
}

pub struct ObscuraBrowserBackend {
    config: ObscuraConfig,
    state: Mutex<Option<SidecarState>>,
}

impl ObscuraBrowserBackend {
    pub fn new(config: ObscuraConfig) -> Self {
        Self {
            config,
            state: Mutex::new(None),
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(ObscuraConfig::default())
    }

    /// Ensure a live sidecar is available, returning the locked guard.
    ///
    /// The guard must be held for the duration of the fetch: the
    /// `Browser` inside is not `Sync` in a way that permits concurrent
    /// page creation, and the router fetches sequentially anyway.
    async fn ensure_started(&self) -> Result<tokio::sync::MutexGuard<'_, Option<SidecarState>>> {
        let mut guard = self.state.lock().await;

        let alive = match guard.as_mut() {
            None => false,
            Some(state) => matches!(state.child.try_wait(), Ok(None)),
        };

        if !alive {
            // Discard any dead state before spawning a new one.
            if let Some(mut old) = guard.take() {
                let _ = old.browser.close().await;
                old.handler.abort();
                // The child is killed by `kill_on_drop` when `old` drops.
            }
            let fresh = self.spawn_sidecar().await?;
            *guard = Some(fresh);
        }

        Ok(guard)
    }

    async fn spawn_sidecar(&self) -> Result<SidecarState> {
        std::fs::create_dir_all(&self.config.storage_dir).map_err(|e| {
            CoreError::Message(format!(
                "could not create storage dir {}: {e}",
                self.config.storage_dir.display()
            ))
        })?;

        cleanup_stale_lock(&self.config.storage_dir).await?;

        let port = allocate_port()?;

        let child = Command::new(&self.config.binary)
            .arg("serve")
            .arg("--port")
            .arg(port.to_string())
            .arg("--host")
            .arg("127.0.0.1")
            .arg("--storage-dir")
            .arg(&self.config.storage_dir)
            .arg("--stealth")
            .kill_on_drop(true)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| {
                CoreError::Message(format!(
                    "failed to spawn `{} serve`: {e}. \
                     Set ObscuraConfig::binary to an absolute path, or add the binary to PATH, \
                     or (for tests) place it at `vendor/obscura` relative to the workspace root.",
                    self.config.binary.display()
                ))
            })?;

        write_lock_file(&self.config.storage_dir, child.id());

        wait_for_port(port, self.config.spawn_timeout).await?;

        let cdp_url = format!("ws://127.0.0.1:{port}");
        let (browser, mut handler) = Browser::connect(&cdp_url).await.map_err(|e| {
            CoreError::Message(format!("Browser::connect to {cdp_url} failed: {e}"))
        })?;

        // The handler must be drained continuously; without an active
        // consumer, every CDP command hangs.
        let handler_task = tokio::spawn(async move { while handler.next().await.is_some() {} });

        tracing::debug!(port, "obscura sidecar ready");

        Ok(SidecarState {
            child,
            browser,
            handler: handler_task,
            port,
        })
    }
}

#[async_trait]
impl BrowserBackend for ObscuraBrowserBackend {
    async fn fetch_rendered(&self, url: &Url, opts: RenderOpts) -> Result<RenderedPage> {
        let mut guard = self.ensure_started().await?;
        let state = guard
            .as_mut()
            .expect("ensure_started guarantees Some after success");

        let page = state
            .browser
            .new_page("about:blank")
            .await
            .map_err(|e| CoreError::Message(format!("new_page failed: {e}")))?;

        page.goto(url.as_str())
            .await
            .map_err(|e| CoreError::Message(format!("goto failed: {e}")))?;

        if let Some(selector) = opts.wait_for_selector.as_deref() {
            let timeout = if opts.timeout_ms == 0 {
                15_000
            } else {
                opts.timeout_ms
            };
            wait_for_selector(&page, selector, Duration::from_millis(timeout)).await?;
        } else {
            // No explicit selector: wait a fixed delay. Obscura fires
            // lifecycle events at domcontentloaded, before XHRs settle,
            // so a fixed delay is more reliable than waiting for
            // `load` or `networkidle` events.
            tokio::time::sleep(self.config.default_settle_delay).await;
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

        // Dropping the page releases the tab; the sidecar keeps running.
        drop(page);

        Ok(RenderedPage {
            final_url,
            html,
            status: 200,
        })
    }

    // print_to_pdf inherits the trait default (BackendUnsupported).

    fn name(&self) -> &'static str {
        "obscura-sidecar"
    }

    async fn shutdown(&self) -> Result<()> {
        let mut guard = self.state.lock().await;
        if let Some(mut state) = guard.take() {
            // Abort the handler first: it is the task that would try to
            // dispatch any subsequent CDP command over the socket, and
            // once the child is gone the socket is dead.
            state.handler.abort();

            // Give the child a chance to exit cleanly, then force-kill.
            graceful_shutdown(&mut state.child, self.config.shutdown_timeout).await;

            // Drop the browser *without* awaiting `close()`. `close()`
            // sends `Browser.close` over CDP and waits for a response;
            // with the child already dead, that await never returns.
            // The handler is aborted and the socket is gone, so dropping
            // the `Browser` value is both sufficient and correct here.
            drop(state.browser);

            remove_lock_file(&self.config.storage_dir);
        }
        Ok(())
    }
}

// ----------------------------------------------------------------------
// Helpers
// ----------------------------------------------------------------------

fn allocate_port() -> Result<u16> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))
        .map_err(|e| CoreError::Message(format!("port allocation failed: {e}")))?;
    let port = listener
        .local_addr()
        .map_err(|e| CoreError::Message(format!("local_addr failed: {e}")))?
        .port();
    drop(listener);
    Ok(port)
}

async fn wait_for_port(port: u16, timeout: Duration) -> Result<()> {
    let start = tokio::time::Instant::now();
    loop {
        if tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_ok()
        {
            return Ok(());
        }
        if start.elapsed() > timeout {
            return Err(CoreError::Message(format!(
                "obscura did not start listening on port {port} within {timeout:?}"
            )));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
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

async fn graceful_shutdown(child: &mut Child, timeout: Duration) {
    #[cfg(unix)]
    {
        use nix::sys::signal::{kill, Signal};
        use nix::unistd::Pid;
        if let Some(pid) = child.id() {
            let _ = kill(Pid::from_raw(pid as i32), Signal::SIGTERM);
            match tokio::time::timeout(timeout, child.wait()).await {
                Ok(Ok(_)) => {}
                _ => {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                }
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = child.kill().await;
        let _ = child.wait().await;
        let _ = timeout;
    }
}

// ----------------------------------------------------------------------
// Stale lock file
// ----------------------------------------------------------------------

fn lock_path(storage_dir: &Path) -> PathBuf {
    storage_dir.join(".obscura.lock")
}

fn write_lock_file(storage_dir: &Path, pid: Option<u32>) {
    if let Some(pid) = pid {
        let _ = std::fs::write(lock_path(storage_dir), pid.to_string());
    }
}

fn remove_lock_file(storage_dir: &Path) {
    let _ = std::fs::remove_file(lock_path(storage_dir));
}

async fn cleanup_stale_lock(storage_dir: &Path) -> Result<()> {
    let path = lock_path(storage_dir);
    if !path.exists() {
        return Ok(());
    }
    let pid: Option<u32> = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| s.trim().parse().ok());

    if let Some(pid) = pid {
        if process_alive(pid) {
            tracing::warn!(
                pid,
                "stale obscura lock found from a previous run; sending SIGTERM"
            );
            #[cfg(unix)]
            {
                use nix::sys::signal::{kill, Signal};
                use nix::unistd::Pid;
                let _ = kill(Pid::from_raw(pid as i32), Signal::SIGTERM);
                // Give it a moment to exit before we spawn a new one.
                for _ in 0..20 {
                    if !process_alive(pid) {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
    }

    remove_lock_file(storage_dir);
    Ok(())
}

#[cfg(unix)]
fn process_alive(pid: u32) -> bool {
    use nix::sys::signal::kill;
    use nix::unistd::Pid;
    kill(Pid::from_raw(pid as i32), None).is_ok()
}

#[cfg(not(unix))]
fn process_alive(_pid: u32) -> bool {
    // On Windows, checking a PID for liveness requires OpenProcess.
    // Skipping the check is acceptable: the stale process, if any,
    // holds an ephemeral port we did not allocate, so it does not
    // conflict with the new sidecar.
    false
}
