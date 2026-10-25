//! Sidebar navigation for the main (post-unlock) layout.
//!
//! Distinct from `crate::state::Screen`, which tracks the boot/unlock
//! state machine (`FirstLaunch`, `Unlocking`, `Recovery`, `Ready`).
//! Once the user reaches `state::Screen::Ready`, the sidebar is the
//! only navigation surface.
//!
//! `NavigationState` is pure data. It knows nothing about GPUI, which
//! means every navigation transition can be exercised in a unit test
//! that constructs the state by hand and asserts on it.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainScreen {
    Pipeline,
    Portals,
    CvProfile,
    Prompts,
    Settings,
}

impl MainScreen {
    pub fn label(self) -> &'static str {
        match self {
            MainScreen::Pipeline => "Pipeline",
            MainScreen::Portals => "Portals",
            MainScreen::CvProfile => "CV / Profile",
            MainScreen::Prompts => "Prompts",
            MainScreen::Settings => "Settings",
        }
    }

    pub fn all() -> &'static [MainScreen] {
        &[
            MainScreen::Pipeline,
            MainScreen::Portals,
            MainScreen::CvProfile,
            MainScreen::Prompts,
            MainScreen::Settings,
        ]
    }
}

#[derive(Debug, Clone)]
pub struct NavigationState {
    current: MainScreen,
    history: Vec<MainScreen>,
}

impl Default for NavigationState {
    fn default() -> Self {
        Self {
            current: MainScreen::Pipeline,
            history: Vec::new(),
        }
    }
}

impl NavigationState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn current(&self) -> MainScreen {
        self.current
    }

    /// Navigate to `screen`. A no-op if it is already current. Otherwise
    /// pushes the previous screen onto the history stack.
    pub fn navigate_to(&mut self, screen: MainScreen) {
        if self.current != screen {
            self.history.push(self.current);
            self.current = screen;
        }
    }

    /// Pop the history stack. Returns `false` if the stack is empty.
    pub fn back(&mut self) -> bool {
        if let Some(prev) = self.history.pop() {
            self.current = prev;
            true
        } else {
            false
        }
    }

    pub fn history_len(&self) -> usize {
        self.history.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_screen_is_pipeline() {
        let nav = NavigationState::new();
        assert_eq!(nav.current(), MainScreen::Pipeline);
        assert_eq!(nav.history_len(), 0);
    }

    #[test]
    fn navigate_pushes_previous_onto_history() {
        let mut nav = NavigationState::new();
        nav.navigate_to(MainScreen::Portals);
        assert_eq!(nav.current(), MainScreen::Portals);
        assert_eq!(nav.history_len(), 1);

        nav.navigate_to(MainScreen::Settings);
        assert_eq!(nav.current(), MainScreen::Settings);
        assert_eq!(nav.history_len(), 2);
    }

    #[test]
    fn navigate_to_current_is_a_no_op() {
        let mut nav = NavigationState::new();
        nav.navigate_to(MainScreen::Pipeline);
        assert_eq!(nav.current(), MainScreen::Pipeline);
        assert_eq!(nav.history_len(), 0, "no-op should not push history");
    }

    #[test]
    fn back_pops_history() {
        let mut nav = NavigationState::new();
        nav.navigate_to(MainScreen::Portals);
        nav.navigate_to(MainScreen::Settings);

        assert!(nav.back());
        assert_eq!(nav.current(), MainScreen::Portals);
        assert_eq!(nav.history_len(), 1);

        assert!(nav.back());
        assert_eq!(nav.current(), MainScreen::Pipeline);
        assert_eq!(nav.history_len(), 0);

        assert!(!nav.back(), "back on empty history returns false");
    }

    #[test]
    fn all_screens_have_unique_labels() {
        let labels: Vec<&str> = MainScreen::all().iter().map(|s| s.label()).collect();
        let unique: std::collections::HashSet<&str> = labels.iter().copied().collect();
        assert_eq!(labels.len(), unique.len(), "duplicate labels: {labels:?}");
    }
}
