use super::*;
use crate::user_config::UserConfig;

impl ProbeApp {
    /// Reads the user config file off the UI thread.
    ///
    /// The window is already open. A missing file leaves the defaults in place.
    /// A read or parse failure is shown as a persistent error toast and does not
    /// replace those defaults.
    pub(super) fn load_user_config(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.weak_entity();
        window
            .spawn(cx, async move |cx| {
                let result = cx.background_spawn(async move { UserConfig::load() }).await;
                let _ = view.update_in(cx, |view, _window, cx| {
                    view.finish_user_config_load(result, cx);
                });
            })
            .detach();
    }

    fn finish_user_config_load(
        &mut self,
        result: Result<UserConfig, crate::user_config::ConfigError>,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(config) => self.user_config = config,
            Err(error) => {
                self.show_toast(ToastIntent::Error, error.to_string(), cx);
            }
        }
    }
}
