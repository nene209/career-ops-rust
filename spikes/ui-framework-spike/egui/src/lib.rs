#[derive(Default)]
pub struct UnlockApp {
    pub passphrase: String,
    pub error: Option<String>,
}

impl UnlockApp {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Career-Ops");

        ui.label("Passphrase");
        ui.add(egui::TextEdit::singleline(&mut self.passphrase).hint_text("Passphrase"));
        if ui.button("Unlock").clicked() {
            if self.passphrase == "correct" {
                self.error = None;
                // Transition to Ready
            } else {
                self.error = Some("Incorrect passphrase".into());
            }
        }
        if ui.button("Switch to Recovery").clicked() {
            // Navigate to Recovery
        }
        if let Some(err) = &self.error {
            ui.colored_label(egui::Color32::RED, err);
        }
    }
}
