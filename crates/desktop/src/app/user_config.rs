use super::*;

impl gpui_kit::Global for LoadedUserConfig {}

impl ProbeApp {
    pub(super) fn save_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ApplicationDialog::Settings { theme: mode }) = self.application_dialog else {
            return;
        };
        if self.saving_user_config || self.user_config.theme == mode {
            return;
        }
        let previous = self.user_config.theme;
        self.saving_user_config = true;
        cx.notify();
        cx.spawn_in(window, async move |view, window| {
            let result = window
                .background_spawn(async move { UserConfig::save_theme(previous, mode) })
                .await;
            let _ = view.update_in(window, |view, window, cx| {
                view.complete_settings_save(mode, result, window, cx);
            });
        })
        .detach();
    }

    fn complete_settings_save(
        &mut self,
        mode: ThemeMode,
        result: Result<(), String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.saving_user_config = false;
        match result {
            Ok(()) => {
                self.apply_theme_mode(mode, window, cx);
                cx.set_global(LoadedUserConfig {
                    config: self.user_config.clone(),
                    error: None,
                });
                self.handle_application_dialog_action(ApplicationDialogAction::Cancel, window, cx);
            }
            Err(error) => {
                self.show_toast(ToastIntent::Error, error, cx);
            }
        }
        cx.notify();
    }

    fn apply_theme_mode(&mut self, mode: ThemeMode, window: &mut Window, cx: &mut Context<Self>) {
        self.user_config.theme = mode;
        cx.set_window_appearance(appearance_override(mode));
        Theme::sync_theme(self.theme(window), cx);
        window.refresh();
        cx.notify();
    }

    /// Effective built-in theme for the current preference and OS appearance.
    pub(super) fn theme(&self, window: &Window) -> Theme {
        Theme::for_preference(self.user_config.theme, window.appearance())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext, size};

    fn init_settings_test(cx: &mut TestAppContext) {
        cx.update(Theme::init);
        cx.update(bind_platform_hotkeys);
    }

    fn settings_window(cx: &mut TestAppContext) -> gpui_kit::WindowHandle<ProbeApp> {
        cx.open_window(size(px(900.0), px(640.0)), ProbeApp::new)
    }

    fn show_settings(window: gpui_kit::WindowHandle<ProbeApp>, cx: &mut TestAppContext) {
        window
            .update(cx, |view, window, cx| {
                view.show_application_dialog(
                    ApplicationDialog::Settings {
                        theme: ThemeMode::System,
                    },
                    window,
                    cx,
                );
            })
            .unwrap();
        cx.run_until_parked();
    }

    fn click_settings_control(
        window: gpui_kit::WindowHandle<ProbeApp>,
        cx: &mut TestAppContext,
        selector: &'static str,
    ) {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let bounds = visual.debug_bounds(selector).unwrap();
        visual.simulate_click(bounds.center(), Modifiers::default());
        visual.run_until_parked();
    }

    #[gpui_kit::test]
    fn settings_shortcut_opens_theme_dropdown_and_escape_closes_in_order(cx: &mut TestAppContext) {
        init_settings_test(cx);
        let window = settings_window(cx);
        cx.simulate_keystrokes(window.into(), crate::app::tests::settings_shortcut());
        cx.run_until_parked();
        window
            .update(cx, |view, _, _| {
                assert!(matches!(
                    view.application_dialog,
                    Some(ApplicationDialog::Settings { .. })
                ));
            })
            .unwrap();
        click_settings_control(window, cx, "settings-theme-mode-trigger");
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        for selector in [
            "settings-theme-mode-item-0",
            "settings-theme-mode-item-1",
            "settings-theme-mode-item-2",
        ] {
            assert!(visual.debug_bounds(selector).is_some());
        }
        cx.simulate_keystrokes(window.into(), "escape");
        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        assert!(visual.debug_bounds("settings-theme-mode-item-0").is_none());
        assert!(visual.debug_bounds("settings-dialog-save").is_some());
        cx.simulate_keystrokes(window.into(), "escape");
        cx.run_until_parked();
        window
            .update(cx, |view, _, _| assert!(view.application_dialog.is_none()))
            .unwrap();
    }

    #[gpui_kit::test]
    fn settings_selection_is_a_draft_and_cancel_discards_it(cx: &mut TestAppContext) {
        init_settings_test(cx);
        let window = settings_window(cx);
        show_settings(window, cx);
        click_settings_control(window, cx, "settings-theme-mode-trigger");
        click_settings_control(window, cx, "settings-theme-mode-item-2");
        window
            .update(cx, |view, _, _| {
                assert_eq!(view.user_config.theme, ThemeMode::System);
                assert!(!view.saving_user_config);
                assert!(matches!(
                    view.application_dialog,
                    Some(ApplicationDialog::Settings {
                        theme: ThemeMode::Dark
                    })
                ));
            })
            .unwrap();
        cx.simulate_keystrokes(window.into(), "escape");
        cx.run_until_parked();
        window
            .update(cx, |view, _, _| {
                assert!(view.application_dialog.is_none());
                assert_eq!(view.user_config.theme, ThemeMode::System);
            })
            .unwrap();
    }

    #[gpui_kit::test]
    fn settings_save_keeps_draft_on_failure_and_applies_only_after_success(
        cx: &mut TestAppContext,
    ) {
        init_settings_test(cx);
        let window = settings_window(cx);
        window
            .update(cx, |view, window, cx| {
                view.show_application_dialog(
                    ApplicationDialog::Settings {
                        theme: ThemeMode::Dark,
                    },
                    window,
                    cx,
                );
                view.saving_user_config = true;
                view.handle_application_dialog_action(ApplicationDialogAction::Cancel, window, cx);
                assert!(view.application_dialog.is_some());
                assert_eq!(view.user_config.theme, ThemeMode::System);
                view.complete_settings_save(
                    ThemeMode::Dark,
                    Err("Could not save settings".into()),
                    window,
                    cx,
                );
                assert_eq!(view.user_config.theme, ThemeMode::System);
                assert!(!view.saving_user_config);
                assert!(matches!(
                    view.application_dialog,
                    Some(ApplicationDialog::Settings {
                        theme: ThemeMode::Dark
                    })
                ));
                view.complete_settings_save(ThemeMode::Dark, Ok(()), window, cx);
                assert_eq!(view.user_config.theme, ThemeMode::Dark);
                assert_eq!(
                    view.theme(window).appearance,
                    crate::theme::ThemeAppearance::Dark
                );
                assert_eq!(
                    cx.global::<LoadedUserConfig>().config.theme,
                    ThemeMode::Dark
                );
                assert!(view.application_dialog.is_none());
            })
            .unwrap();
    }
    #[gpui_kit::test]
    fn settings_save_shortcut_resolves_from_dialog_and_dropdown(cx: &mut TestAppContext) {
        use std::{cell::Cell, rc::Rc};
        let submissions = Rc::new(Cell::new(0));
        let observed = submissions.clone();
        let _subscription = cx.update(|cx| {
            cx.observe_keystrokes(move |event, _, _| {
                if event
                    .action
                    .as_ref()
                    .is_some_and(|action| action.as_any().is::<SubmitApplicationDialog>())
                {
                    observed.set(observed.get() + 1);
                }
            })
        });
        init_settings_test(cx);
        let window = settings_window(cx);
        show_settings(window, cx);
        let shortcut = crate::app::tests::send_shortcut();
        cx.simulate_keystrokes(window.into(), shortcut);
        cx.run_until_parked();
        assert_eq!(submissions.get(), 1);
        click_settings_control(window, cx, "settings-theme-mode-trigger");
        cx.simulate_keystrokes(window.into(), "escape");
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), shortcut);
        cx.run_until_parked();
        assert_eq!(submissions.get(), 2);
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        assert!(visual.debug_bounds("settings-theme-mode-item-0").is_none());
        cx.simulate_keystrokes(window.into(), "enter");
        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        assert!(visual.debug_bounds("settings-theme-mode-item-0").is_some());
        assert_eq!(submissions.get(), 2);
        cx.simulate_keystrokes(window.into(), shortcut);
        cx.run_until_parked();
        assert_eq!(submissions.get(), 3);
    }

    #[gpui_kit::test]
    fn settings_shortcut_preserves_structure_environment_and_application_dialogs(
        cx: &mut TestAppContext,
    ) {
        init_settings_test(cx);
        let shortcut = crate::app::tests::settings_shortcut();
        for modal in ["structure", "environment", "application"] {
            let window = settings_window(cx);
            let mut structure = StructureDialog::create_folder(None);
            structure.name = "Unsaved folder".into();
            window
                .update(cx, |view, window, cx| {
                    match modal {
                        "structure" => {
                            view.structure_dialog = Some(structure.clone());
                            view.structure_dialog_focus.focus(window, cx);
                        }
                        "environment" => {
                            view.create_environment_dialog = Some("Unsaved environment".into());
                            view.create_environment_dialog_focus.focus(window, cx);
                        }
                        _ => view.show_application_dialog(
                            ApplicationDialog::Settings {
                                theme: ThemeMode::Dark,
                            },
                            window,
                            cx,
                        ),
                    }
                    cx.notify();
                })
                .unwrap();
            cx.run_until_parked();
            cx.simulate_keystrokes(window.into(), shortcut);
            cx.run_until_parked();
            window
                .update(cx, |view, _, _| {
                    match modal {
                        "structure" => {
                            assert_eq!(view.structure_dialog.as_ref(), Some(&structure));
                            assert!(view.application_dialog.is_none());
                        }
                        "environment" => {
                            assert_eq!(
                                view.create_environment_dialog.as_deref(),
                                Some("Unsaved environment")
                            );
                            assert!(view.application_dialog.is_none());
                        }
                        _ => assert!(matches!(
                            view.application_dialog,
                            Some(ApplicationDialog::Settings {
                                theme: ThemeMode::Dark
                            })
                        )),
                    }
                    assert!(view.pending_application_dialogs.is_empty());
                })
                .unwrap();
        }
    }
}
