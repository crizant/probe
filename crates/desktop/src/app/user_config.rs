use super::*;

impl ProbeApp {
    /// Effective built-in theme for the current preference and OS appearance.
    pub(super) fn theme(&self, window: &Window) -> Theme {
        Theme::for_preference(self.user_config.theme, window.appearance())
    }
}
