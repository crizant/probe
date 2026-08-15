//! Shared caret blink so single-line and multiline fields match native text input.

use std::time::Duration;

use gpui::{App, Global};

pub(crate) const CARET_BLINK_INTERVAL: Duration = Duration::from_millis(530);

#[derive(Debug)]
pub(crate) struct CaretBlink {
    visible: bool,
}

impl Default for CaretBlink {
    fn default() -> Self {
        Self { visible: true }
    }
}

impl Global for CaretBlink {}

impl CaretBlink {
    pub(crate) fn is_visible(cx: &App) -> bool {
        cx.try_global::<Self>()
            .map(|blink| blink.visible)
            .unwrap_or(true)
    }

    pub(crate) fn show(cx: &mut App) {
        cx.default_global::<Self>().visible = true;
    }

    pub(crate) fn toggle(cx: &mut App) {
        let blink = cx.default_global::<Self>();
        blink.visible = !blink.visible;
    }
}

#[cfg(test)]
mod tests {
    use gpui::TestAppContext;

    use super::CaretBlink;

    #[gpui::test]
    fn caret_is_visible_until_toggled_off(cx: &mut TestAppContext) {
        cx.update(|cx| {
            assert!(CaretBlink::is_visible(cx));
            CaretBlink::toggle(cx);
            assert!(!CaretBlink::is_visible(cx));
            CaretBlink::show(cx);
            assert!(CaretBlink::is_visible(cx));
        });
    }
}
