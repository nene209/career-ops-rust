//! Pre-`Ready` screens: unlock, first launch, and recovery.
//!
//! These views are deliberately thin. `AppState` decides which screen is
//! visible and performs the state transitions; the views collect input and
//! return a [`LockAction`] for `CareerOpsApp` to execute. Keeping the actions
//! as data means the screens can be tested with `egui_kittest` without a
//! keystore, a database, or a tokio runtime.
//!
//! Recovery is a full-screen flow, not a modal. A user who sees it is already
//! worried; a calm page with explicit options is less alarming than a dialog
//! over a locked shell.

use std::path::{Path, PathBuf};

use egui::{Align2, Button, Color32, RichText, Ui, Window};

use super::components::{labelled_input, LoadingSpinner};
use crate::state::Screen;

/// The exact phrase the user must type before the vault is created.
pub const ACK_PHRASE: &str =
    "I understand that if I lose my passphrase and my keychain entry, my data cannot be recovered";

/// The exact phrase the user must type before the last-resort reset.
pub const RESET_PHRASE: &str = "DELETE MY DATA";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FirstLaunchStep {
    #[default]
    Welcome,
    Passphrase,
    Backup,
}

#[derive(Default)]
pub struct FirstLaunchState {
    pub step: FirstLaunchStep,
    pub passphrase: String,
    pub confirmation: String,
    pub acknowledgement: String,
}

#[derive(Default)]
pub struct RecoveryState {
    pub passphrase: String,
    pub key_path: String,
    pub export_dir: String,
    pub confirmation: String,
    pub status: Option<String>,
}

/// UI-only state for the pre-`Ready` screens.
///
/// Secrets live here only while the user is typing. `CareerOpsApp` clears them
/// when it starts an async operation.
#[derive(Default)]
pub struct LockView {
    pub passphrase: String,
    pub error: Option<String>,
    pub first_launch: FirstLaunchState,
    pub recovery: RecoveryState,
}

/// An intent emitted by a lock screen. `CareerOpsApp` owns the side effects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockAction {
    TryKeychain,
    RetryKeychain,
    TryPassphrase(String),
    CompleteFirstLaunch(String),
    OpenDataDir,
    LocateKeyFile(PathBuf),
    ExportDatabase(PathBuf),
    StartFresh,
}

pub fn show(
    ui: &mut Ui,
    screen: &Screen,
    view: &mut LockView,
    data_dir: &Path,
) -> Option<LockAction> {
    match screen {
        Screen::Booting => {
            busy(ui, "Loading application state...");
            None
        }
        Screen::FirstLaunch => show_first_launch(ui, view, data_dir),
        Screen::Unlocking => show_unlock(ui, view, None),
        Screen::KeychainUnavailable { reason } => show_unlock(ui, view, Some(reason)),
        Screen::Recovery { reason } => show_recovery(ui, view, reason, data_dir),
        Screen::Ready => None,
    }
}

/// A centered spinner used while an async operation owns the state.
pub fn busy(ui: &mut Ui, message: &str) {
    egui::CentralPanel::default().show(ui, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(64.0);
            LoadingSpinner::show(ui);
            ui.add_space(8.0);
            ui.label(message);
        });
    });
}

/// Shown only if an interrupted operation lost the state and the data
/// directory could not be reopened. This is intentionally not a recovery
/// screen: the app cannot reason about the files anymore, so restarting is the
/// only safe instruction.
pub fn fatal(ui: &mut Ui, message: &str) {
    egui::CentralPanel::default().show(ui, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(64.0);
            ui.heading("Career-Ops could not reopen");
            ui.add_space(8.0);
            ui.label(message);
            ui.add_space(12.0);
            ui.label("Close and restart the application.");
        });
    });
}

fn show_unlock(
    ui: &mut Ui,
    view: &mut LockView,
    keychain_reason: Option<&str>,
) -> Option<LockAction> {
    let mut action = None;

    egui::CentralPanel::default().show(ui, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(48.0);
            ui.heading("Unlock Career-Ops");
            ui.add_space(4.0);
            ui.label(RichText::new("Your data is encrypted on this machine.").weak());

            if let Some(reason) = keychain_reason {
                ui.add_space(8.0);
                ui.colored_label(
                    Color32::from_rgb(0xc9, 0x8a, 0x00),
                    format!("Keychain unavailable: {reason}"),
                );
            }

            ui.add_space(16.0);
            ui.group(|ui| {
                ui.set_width(420.0);

                let response = labelled_input(ui, "Passphrase", &mut view.passphrase, true);
                let can_unlock = !view.passphrase.is_empty();
                let enter = can_unlock
                    && response.has_focus()
                    && ui.input(|input| input.key_pressed(egui::Key::Enter));

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.add_enabled(can_unlock, Button::new("Unlock")).clicked() || enter {
                        action = Some(LockAction::TryPassphrase(view.passphrase.clone()));
                    }

                    let keychain_label = if keychain_reason.is_some() {
                        "Retry keychain"
                    } else {
                        "Try keychain"
                    };
                    if ui.button(keychain_label).clicked() {
                        action = Some(LockAction::TryKeychain);
                    }
                });

                if let Some(error) = &view.error {
                    ui.add_space(8.0);
                    ui.colored_label(Color32::from_rgb(0xc0, 0x39, 0x2b), error.as_str());
                }
            });
        });
    });

    action
}

fn show_first_launch(ui: &mut Ui, view: &mut LockView, data_dir: &Path) -> Option<LockAction> {
    let mut action = None;

    egui::CentralPanel::default().show(ui, |ui| {
        Window::new("Set up Career-Ops")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .default_width(500.0)
            .show(ui.ctx(), |ui| {
                ui.label(RichText::new(step_label(view.first_launch.step)).strong());
                ui.separator();
                ui.add_space(4.0);

                match view.first_launch.step {
                    FirstLaunchStep::Welcome => welcome_step(ui, view),
                    FirstLaunchStep::Passphrase => passphrase_step(ui, view),
                    FirstLaunchStep::Backup => backup_step(ui, view, data_dir, &mut action),
                }

                if let Some(error) = &view.error {
                    ui.add_space(8.0);
                    ui.colored_label(Color32::from_rgb(0xc0, 0x39, 0x2b), error.as_str());
                }
            });
    });

    action
}

fn step_label(step: FirstLaunchStep) -> &'static str {
    match step {
        FirstLaunchStep::Welcome => "Step 1 of 3 — Welcome",
        FirstLaunchStep::Passphrase => "Step 2 of 3 — Create a passphrase",
        FirstLaunchStep::Backup => "Step 3 of 3 — Back up your data",
    }
}

fn welcome_step(ui: &mut Ui, view: &mut LockView) {
    ui.label("Career-Ops keeps your job-search data on this machine.");
    ui.label(
        "The database is encrypted at rest. You will set a passphrase in the next step. \
         The passphrase and your OS keychain each protect a copy of the database key, so \
         either one can unlock your data.",
    );
    ui.add_space(12.0);
    if ui.button("Continue").clicked() {
        view.first_launch.step = FirstLaunchStep::Passphrase;
    }
}

fn passphrase_step(ui: &mut Ui, view: &mut LockView) {
    ui.label(
        "Choose a passphrase. It is the only recovery path if the OS keychain becomes \
         unavailable, and it cannot be recovered for you.",
    );
    ui.add_space(8.0);
    labelled_input(ui, "Passphrase", &mut view.first_launch.passphrase, true);
    labelled_input(
        ui,
        "Confirm passphrase",
        &mut view.first_launch.confirmation,
        true,
    );

    if !view.first_launch.passphrase.is_empty() {
        let strength = passphrase_strength(&view.first_launch.passphrase);
        ui.label(RichText::new(format!("Strength: {}", strength.label())).color(strength.color()));
    }
    if !view.first_launch.confirmation.is_empty()
        && view.first_launch.passphrase != view.first_launch.confirmation
    {
        ui.colored_label(
            Color32::from_rgb(0xc0, 0x39, 0x2b),
            "The passphrases do not match.",
        );
    }

    ui.add_space(12.0);
    ui.label("Type this phrase to confirm that you understand the risk:");
    ui.label(RichText::new(ACK_PHRASE).monospace().weak());
    labelled_input(
        ui,
        "Acknowledgement",
        &mut view.first_launch.acknowledgement,
        false,
    );

    ui.add_space(8.0);
    if ui
        .add_enabled(
            first_launch_passphrase_valid(&view.first_launch),
            Button::new("Continue"),
        )
        .clicked()
    {
        view.first_launch.step = FirstLaunchStep::Backup;
    }
}

fn backup_step(ui: &mut Ui, view: &mut LockView, data_dir: &Path, action: &mut Option<LockAction>) {
    ui.label("Back up these two files together:");
    ui.label(RichText::new(format!("{} / dek.wrapped.json", data_dir.display())).monospace());
    ui.label(RichText::new(format!("{} / career-ops.db", data_dir.display())).monospace());
    ui.add_space(8.0);
    ui.label(
        "The wrapped key file is as sensitive as the database. If you lose the passphrase \
         and the keychain entry, the data cannot be recovered.",
    );
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        if ui.button("Open data folder").clicked() {
            *action = Some(LockAction::OpenDataDir);
        }
        if ui.button("Back").clicked() {
            view.first_launch.step = FirstLaunchStep::Passphrase;
        }
        let can_create = !view.first_launch.passphrase.is_empty();
        if ui
            .add_enabled(can_create, Button::new("Create vault"))
            .clicked()
        {
            *action = Some(LockAction::CompleteFirstLaunch(
                view.first_launch.passphrase.clone(),
            ));
        }
    });
}

fn show_recovery(
    ui: &mut Ui,
    view: &mut LockView,
    reason: &str,
    data_dir: &Path,
) -> Option<LockAction> {
    let mut action = None;

    egui::CentralPanel::default().show(ui, |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.heading("Recovery required");
            ui.add_space(4.0);
            ui.colored_label(Color32::from_rgb(0xc9, 0x8a, 0x00), reason);
            ui.add_space(8.0);
            ui.label(format!("Data directory: {}", data_dir.display()));
            ui.add_space(12.0);

            if let Some(error) = &view.error {
                ui.colored_label(Color32::from_rgb(0xc0, 0x39, 0x2b), error.as_str());
                ui.add_space(8.0);
            }
            if let Some(status) = &view.recovery.status {
                ui.colored_label(Color32::from_rgb(0x2e, 0xa0, 0x43), status.as_str());
                ui.add_space(8.0);
            }

            ui.group(|ui| {
                ui.heading("1. Unlock with your passphrase");
                ui.label(
                    "If you remember your passphrase, enter it here. This is the primary \
                     recovery path.",
                );
                labelled_input(
                    ui,
                    "Recovery passphrase",
                    &mut view.recovery.passphrase,
                    true,
                );
                let can_unlock = !view.recovery.passphrase.is_empty();
                if ui
                    .add_enabled(can_unlock, Button::new("Unlock with passphrase"))
                    .clicked()
                {
                    action = Some(LockAction::TryPassphrase(view.recovery.passphrase.clone()));
                }
            });
            ui.add_space(8.0);

            ui.group(|ui| {
                ui.heading("2. Retry the keychain");
                ui.label(
                    "A locked or temporarily unavailable OS keychain can sometimes be retried.",
                );
                if ui.button("Retry keychain").clicked() {
                    action = Some(LockAction::RetryKeychain);
                }
            });
            ui.add_space(8.0);

            ui.group(|ui| {
                ui.heading("3. Locate your key file");
                ui.label("If you restored dek.wrapped.json from a backup, point to it here.");
                labelled_input(ui, "Key file path", &mut view.recovery.key_path, false);

                let path = PathBuf::from(view.recovery.key_path.trim());
                let can_install = !view.recovery.key_path.trim().is_empty() && path.is_file();
                if ui
                    .add_enabled(can_install, Button::new("Use this key file"))
                    .clicked()
                {
                    action = Some(LockAction::LocateKeyFile(path));
                }
            });
            ui.add_space(8.0);

            ui.group(|ui| {
                ui.heading("4. Export the encrypted database");
                ui.label(
                    "Copy career-ops.db and dek.wrapped.json into an empty folder before \
                     trying anything destructive.",
                );
                labelled_input(
                    ui,
                    "Destination folder",
                    &mut view.recovery.export_dir,
                    false,
                );

                let can_export = !view.recovery.export_dir.trim().is_empty();
                if ui.add_enabled(can_export, Button::new("Export")).clicked() {
                    action = Some(LockAction::ExportDatabase(PathBuf::from(
                        view.recovery.export_dir.trim(),
                    )));
                }
            });
            ui.add_space(8.0);

            ui.group(|ui| {
                ui.heading("5. Start fresh (last resort)");
                ui.colored_label(
                    Color32::from_rgb(0xc0, 0x39, 0x2b),
                    "This deletes career-ops.db and dek.wrapped.json. The data cannot be \
                     recovered.",
                );
                ui.label("Type DELETE MY DATA to enable the button.");
                labelled_input(ui, "Confirmation", &mut view.recovery.confirmation, false);

                let confirmed = view.recovery.confirmation.trim() == RESET_PHRASE;
                if ui
                    .add_enabled(confirmed, Button::new("Delete my data and start fresh"))
                    .clicked()
                {
                    action = Some(LockAction::StartFresh);
                }
            });
        });
    });

    action
}

/// The Continue button is enabled only when the two passphrases match and the
/// acknowledgement is typed exactly. Strength is feedback, not a gate.
pub fn first_launch_passphrase_valid(state: &FirstLaunchState) -> bool {
    !state.passphrase.is_empty()
        && state.passphrase == state.confirmation
        && state.acknowledgement.trim() == ACK_PHRASE
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PassphraseStrength {
    Weak,
    Fair,
    Strong,
}

impl PassphraseStrength {
    fn label(self) -> &'static str {
        match self {
            Self::Weak => "Weak",
            Self::Fair => "Fair",
            Self::Strong => "Strong",
        }
    }

    fn color(self) -> Color32 {
        match self {
            Self::Weak => Color32::from_rgb(0xc0, 0x39, 0x2b),
            Self::Fair => Color32::from_rgb(0xc9, 0x8a, 0x00),
            Self::Strong => Color32::from_rgb(0x2e, 0xa0, 0x43),
        }
    }
}

fn passphrase_strength(passphrase: &str) -> PassphraseStrength {
    let length = passphrase.chars().count();
    let mut classes = 0;

    if passphrase.chars().any(|c| c.is_lowercase()) {
        classes += 1;
    }
    if passphrase.chars().any(|c| c.is_uppercase()) {
        classes += 1;
    }
    if passphrase.chars().any(|c| c.is_ascii_digit()) {
        classes += 1;
    }
    if passphrase
        .chars()
        .any(|c| !c.is_alphanumeric() && !c.is_whitespace())
    {
        classes += 1;
    }

    if length < 8 {
        PassphraseStrength::Weak
    } else if length >= 12 && classes >= 3 {
        PassphraseStrength::Strong
    } else {
        PassphraseStrength::Fair
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_state() -> FirstLaunchState {
        FirstLaunchState {
            step: FirstLaunchStep::Passphrase,
            passphrase: "correct horse battery staple".into(),
            confirmation: "correct horse battery staple".into(),
            acknowledgement: ACK_PHRASE.into(),
        }
    }

    #[test]
    fn continue_requires_a_passphrase_confirmation_and_acknowledgement() {
        assert!(first_launch_passphrase_valid(&valid_state()));

        let mut empty = valid_state();
        empty.passphrase.clear();
        empty.confirmation.clear();
        assert!(!first_launch_passphrase_valid(&empty));

        let mut mismatch = valid_state();
        mismatch.confirmation = "something else".into();
        assert!(!first_launch_passphrase_valid(&mismatch));

        let mut wrong_acknowledgement = valid_state();
        wrong_acknowledgement.acknowledgement = "I understand".into();
        assert!(!first_launch_passphrase_valid(&wrong_acknowledgement));
    }

    #[test]
    fn acknowledgement_is_trimmed_at_the_edges_only() {
        let mut state = valid_state();
        state.acknowledgement = format!("  {}  ", ACK_PHRASE);
        assert!(first_launch_passphrase_valid(&state));

        state.acknowledgement = ACK_PHRASE.to_lowercase();
        assert!(
            !first_launch_passphrase_valid(&state),
            "the phrase is case-sensitive"
        );
    }

    #[test]
    fn passphrase_strength_is_feedback_only() {
        assert_eq!(passphrase_strength(""), PassphraseStrength::Weak);
        assert_eq!(passphrase_strength("short"), PassphraseStrength::Weak);
        assert_eq!(passphrase_strength("longenough"), PassphraseStrength::Fair);
        assert_eq!(
            passphrase_strength("Long-Enough-123"),
            PassphraseStrength::Strong
        );
    }
}
