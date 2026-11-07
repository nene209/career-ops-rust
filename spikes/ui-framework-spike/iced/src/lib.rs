use iced::widget::{button, column, text, text_input};
use iced::{Element, Task};

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    PassphraseChanged(String),
    UnlockClicked,
    SwitchToRecovery,
}

#[derive(Default)]
pub struct UnlockApp {
    pub passphrase: String,
    pub error: Option<String>,
}

impl UnlockApp {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::PassphraseChanged(p) => self.passphrase = p,
            Message::UnlockClicked => {
                if self.passphrase == "correct" {
                    self.error = None;
                    // Transition to Ready
                } else {
                    self.error = Some("Incorrect passphrase".into());
                }
            }
            Message::SwitchToRecovery => {
                // Navigate to Recovery
            }
        }
        Task::none()
    }

    pub fn view(&self) -> Element<'_, Message> {
        column![
            text_input("Passphrase", &self.passphrase).on_input(Message::PassphraseChanged),
            button("Unlock").on_press(Message::UnlockClicked),
            button("Switch to Recovery").on_press(Message::SwitchToRecovery),
            text(self.error.as_deref().unwrap_or("")),
        ]
        .padding(24)
        .spacing(12)
        .into()
    }
}
