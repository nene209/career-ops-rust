//! GPUI view layer — placeholder.
//!
//! The views are intentionally minimal. GPUI is pre-1.0 and its API is
//! unstable; these functions exist to declare the shape of the UI, not to
//! render it. When GPUI stabilises, each function becomes a `Render` impl.
//!
//! The state machine in `state.rs` is the load-bearing part of Phase 1 and
//! is fully testable without these views.

use crate::state::{AppState, Screen};

pub fn render_unlock(_state: &AppState) -> &'static str {
    "Unlock screen — passphrase entry, keychain retry, error display.\n\
     Buttons: Unlock with passphrase / Try keychain / Show recovery options."
}

pub fn render_first_launch() -> &'static str {
    "First-launch wizard — welcome, encryption explanation, passphrase creation,\n\
     acknowledgement (typed confirmation), backup guidance, optional AI provider setup."
}

pub fn render_recovery(_reason: &str) -> &'static str {
    "Recovery screen — five options:\n\
     1. Unlock with passphrase\n\
     2. Retry keychain\n\
     3. Locate key file\n\
     4. Export encrypted database\n\
     5. Last resort: start fresh (requires typed confirmation)"
}

pub fn render_keychain_unavailable(_reason: &str) -> &'static str {
    "Keychain unavailable — explain the cause, offer passphrase fallback."
}

pub fn render_for(state: &AppState) -> &'static str {
    match state.current() {
        Screen::Booting => "Boot...",
        Screen::FirstLaunch => render_first_launch(),
        Screen::Unlocking => render_unlock(state),
        Screen::KeychainUnavailable { reason } => render_keychain_unavailable(reason),
        Screen::Recovery { reason } => render_recovery(reason),
        Screen::Ready => "Ready — navigation shell, placeholder screens.",
    }
}