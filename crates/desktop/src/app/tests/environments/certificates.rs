use super::*;
use crate::app::EnvironmentTab;
use probe_core::ClientCertificate;

fn click(workspace: &EnvironmentWorkspace, cx: &mut TestAppContext, selector: &'static str) {
    let mut visual = workspace.visual(cx);
    let bounds = visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    visual.simulate_click(bounds.center(), Modifiers::default());
    visual.run_until_parked();
}

fn fill(workspace: &EnvironmentWorkspace, cx: &mut TestAppContext, field: usize, value: &str) {
    workspace.update(cx, |view, window, cx| {
        view.environment_manager_dialog
            .as_ref()
            .unwrap()
            .certificate_editor
            .as_ref()
            .unwrap()
            .inputs[field]
            .update(cx, |input, cx| {
                input.set_value(value.to_owned(), window, cx)
            });
        cx.notify();
    });
    cx.run_until_parked();
}

fn certificates(
    workspace: &EnvironmentWorkspace,
    cx: &mut TestAppContext,
) -> Vec<ClientCertificate> {
    workspace.update(cx, |view, _, _| {
        view.environment_manager_dialog
            .as_ref()
            .unwrap()
            .draft()
            .client_certificates
            .clone()
    })
}

#[gpui_kit::test]
fn certificate_tabs_filter_rows_mask_passphrases_and_leave_credential_store_untouched(
    cx: &mut TestAppContext,
) {
    let workspace = EnvironmentWorkspace::writable(cx, "certificate-templates");
    workspace.open_manager(cx, "base");
    for (tab, plain) in [("environment-tab-0", true), ("environment-tab-1", false)] {
        click(&workspace, cx, tab);
        for (selector, visible) in [
            ("environment-variable-value-host", plain),
            ("environment-secret-set-secretToken", !plain),
            ("environment-manager-add-variable", plain),
            ("environment-manager-add-secret", !plain),
        ] {
            assert_eq!(
                workspace.visual(cx).debug_bounds(selector).is_some(),
                visible,
                "{selector}"
            );
        }
    }
    click(&workspace, cx, "environment-tab-2");
    assert!(
        workspace
            .visual(cx)
            .debug_bounds("environment-certificates")
            .is_some()
    );
    assert!(
        workspace
            .visual(cx)
            .debug_bounds("environment-variable-value-host")
            .is_none()
    );
    assert!(
        workspace
            .visual(cx)
            .debug_bounds("environment-secret-set-secretToken")
            .is_none()
    );
    click(&workspace, cx, "certificate-add");
    fill(&workspace, cx, 0, "api.example.com");
    fill(&workspace, cx, 1, "certs/client.pem");
    fill(&workspace, cx, 2, "certs/client-key.pem");
    fill(&workspace, cx, 4, "{{secretVariable}}");
    for (toggle, masked) in [(false, true), (true, false), (true, true)] {
        if toggle {
            click(&workspace, cx, "certificate-reveal");
        }
        workspace.update(cx, |view, _, cx| {
            let editor = view
                .environment_manager_dialog
                .as_ref()
                .unwrap()
                .certificate_editor
                .as_ref()
                .unwrap();
            assert_eq!(editor.inputs[4].read(cx).presentation().is_masked(), masked);
        });
    }
    click(&workspace, cx, "certificate-save");
    workspace.update(cx, |view, window, cx| {
        view.save_environment_manager_dialog(window, cx)
    });
    cx.run_until_parked();
    let saved = probe_opencollection::load_workspace(&workspace.path).unwrap();
    let certificate = &saved
        .workspace()
        .environments()
        .iter()
        .find(|environment| environment.name == "base")
        .unwrap()
        .client_certificates[0];
    assert!(
        matches!(certificate, ClientCertificate::Pem { passphrase, .. } if passphrase.as_deref() == Some("{{secretVariable}}"))
    );
    assert_eq!(workspace.store.get_calls(), 0);
    assert!(
        workspace.store.values.lock().unwrap().is_empty(),
        "templated passphrases must not be written to the credential store"
    );
}

#[gpui_kit::test]
fn certificate_crud_order_disable_save_and_dirty_switching(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::writable(cx, "certificate-ui");
    workspace.open_manager(cx, "base");
    let original_variables = workspace.update(cx, |view, _, _| {
        view.environment_manager_dialog
            .as_ref()
            .unwrap()
            .draft()
            .variables
            .clone()
    });
    click(&workspace, cx, "environment-tab-2");
    for pkcs12 in [false, true] {
        click(&workspace, cx, "certificate-add");
        // An unfinished form protects environment switching even before saving the row.
        workspace.update(cx, |view, _, cx| {
            view.select_environment_manager_environment("development", cx);
            assert_eq!(
                view.environment_manager_dialog
                    .as_ref()
                    .unwrap()
                    .original_name,
                "base"
            );
        });
        fill(&workspace, cx, 0, "{{domain}}");
        if pkcs12 {
            click(&workspace, cx, "certificate-type-1");
            workspace.update(cx, |view, window, cx| {
                view.save_certificate_editor(window, cx)
            });
            assert_eq!(certificates(&workspace, cx).len(), 1);
            fill(&workspace, cx, 3, "{{certDirectory}}/client.pfx");
        } else {
            fill(&workspace, cx, 1, "certs/client.pem");
            fill(&workspace, cx, 2, "{{privateKeyPath}}");
        }
        fill(&workspace, cx, 4, "literal password");
        click(&workspace, cx, "certificate-save");
    }
    let original_order = certificates(&workspace, cx);
    click(&workspace, cx, "certificate-up-0");
    click(&workspace, cx, "certificate-down-1");
    assert_eq!(
        certificates(&workspace, cx),
        original_order,
        "boundary chevrons must be disabled"
    );
    // These clicks now exercise the same switches used by Variables.
    click(&workspace, cx, "certificate-toggle-0");
    click(&workspace, cx, "certificate-toggle-1");
    click(&workspace, cx, "certificate-up-1");
    let reordered = certificates(&workspace, cx);
    click(&workspace, cx, "certificate-down-0");
    assert_eq!(
        certificates(&workspace, cx),
        reordered.iter().rev().cloned().collect::<Vec<_>>()
    );
    click(&workspace, cx, "certificate-up-1");
    assert_eq!(certificates(&workspace, cx), reordered);
    click(&workspace, cx, "certificate-edit-0");
    fill(&workspace, cx, 3, "certs/edited.p12");
    click(&workspace, cx, "certificate-save");
    click(&workspace, cx, "certificate-edit-1");
    fill(&workspace, cx, 1, "certs/edited.pem");
    click(&workspace, cx, "certificate-save");
    let expected = certificates(&workspace, cx);
    assert!(
        matches!(&expected[0], ClientCertificate::Pkcs12 { disabled: true, pkcs12_file_path, .. } if pkcs12_file_path == "certs/edited.p12")
    );
    assert!(
        matches!(&expected[1], ClientCertificate::Pem { disabled: true, certificate_file_path, .. } if certificate_file_path == "certs/edited.pem")
    );
    workspace.update(cx, |view, window, cx| {
        view.select_environment_manager_environment("development", cx);
        assert_eq!(
            view.environment_manager_dialog
                .as_ref()
                .unwrap()
                .original_name,
            "base"
        );
        view.save_environment_manager_dialog(window, cx);
    });
    cx.run_until_parked();
    let loaded = probe_opencollection::load_workspace(&workspace.path).unwrap();
    let base = loaded
        .workspace()
        .environments()
        .iter()
        .find(|environment| environment.name == "base")
        .unwrap();
    assert_eq!(base.client_certificates, expected);
    assert_eq!(base.variables, original_variables);
    workspace.update(cx, |view, _, cx| {
        assert!(!view.environment_manager_is_dirty());
        view.select_environment_manager_environment("development", cx);
        assert_eq!(
            view.environment_manager_dialog
                .as_ref()
                .unwrap()
                .original_name,
            "development"
        );
        assert_eq!(
            view.environment_manager_dialog.as_ref().unwrap().tab,
            EnvironmentTab::ClientCertificates
        );
    });
    cx.run_until_parked();
    workspace.update(cx, |view, _, cx| {
        view.select_environment_manager_environment("base", cx)
    });
    cx.run_until_parked();
    click(&workspace, cx, "certificate-delete-0");
    click(&workspace, cx, "certificate-delete-0");
    workspace.update(cx, |view, window, cx| {
        view.save_environment_manager_dialog(window, cx)
    });
    cx.run_until_parked();
    let loaded = probe_opencollection::load_workspace(&workspace.path).unwrap();
    assert!(
        loaded
            .workspace()
            .environments()
            .iter()
            .find(|environment| environment.name == "base")
            .unwrap()
            .client_certificates
            .is_empty()
    );
    assert_eq!(workspace.store.get_calls(), 0);
    assert!(
        workspace.store.values.lock().unwrap().is_empty(),
        "literal passphrases never enter the credential store"
    );
}

#[gpui_kit::test]
fn certificate_browse_stores_relative_and_external_paths_and_ignores_stale_picker(
    cx: &mut TestAppContext,
) {
    let workspace = EnvironmentWorkspace::writable(cx, "certificate-picker");
    workspace.open_manager(cx, "base");
    workspace.update(cx, |view, window, cx| {
        view.open_certificate_editor(None, window, cx)
    });
    cx.run_until_parked();
    for (field, filename) in [(1, "client.pem"), (3, "client.p12")] {
        let relative = format!("certs/{filename}");
        let inside = workspace
            .path
            .parent()
            .unwrap()
            .join("certs")
            .join(filename);
        let outside = workspace
            .path
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join(format!("external-{filename}"));
        if field == 3 {
            click(&workspace, cx, "certificate-type-1");
        }
        for (selected, expected) in [
            (inside, relative),
            (outside.clone(), outside.to_string_lossy().into_owned()),
        ] {
            click(
                &workspace,
                cx,
                if field == 1 {
                    "certificate-browse-1"
                } else {
                    "certificate-browse-3"
                },
            );
            assert!(cx.did_prompt_for_paths());
            cx.simulate_path_prompt_response(|options| {
                assert!(options.files && !options.directories && !options.multiple);
                Some(vec![selected])
            });
            cx.run_until_parked();
            workspace.update(cx, |view, _, cx| {
                assert_eq!(
                    view.environment_manager_dialog
                        .as_ref()
                        .unwrap()
                        .certificate_editor
                        .as_ref()
                        .unwrap()
                        .inputs[field]
                        .read(cx)
                        .value()
                        .as_str(),
                    expected
                )
            });
        }
    }
    click(&workspace, cx, "certificate-type-0");
    click(&workspace, cx, "certificate-browse-2");
    workspace.update(cx, |view, window, cx| {
        view.cancel_certificate_editor(window, cx);
        view.open_certificate_editor(None, window, cx)
    });
    cx.simulate_path_prompt_response(|_| {
        Some(vec![workspace.path.parent().unwrap().join("stale.pem")])
    });
    cx.run_until_parked();
    workspace.update(cx, |view, _, cx| {
        assert!(
            view.environment_manager_dialog
                .as_ref()
                .unwrap()
                .certificate_editor
                .as_ref()
                .unwrap()
                .inputs[2]
                .read(cx)
                .value()
                .is_empty()
        )
    });
}

#[gpui_kit::test]
fn certificate_forms_follow_existing_external_reload_conflict_policy(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::writable(cx, "certificate-reload");
    workspace.open_manager(cx, "base");
    workspace.update(cx, |view, window, cx| {
        view.open_certificate_editor(None, window, cx)
    });
    fill(&workspace, cx, 0, "unsaved.example.com");
    workspace.update(cx, |view, _, cx| {
        view.apply_reconciled_workspace(
            reconciled_workspace(probe_opencollection::load_workspace(&workspace.path).unwrap()),
            cx,
        );
        assert!(
            view.environment_manager_dialog
                .as_ref()
                .unwrap()
                .certificate_editor
                .is_some(),
            "unrelated reload retains the form"
        );
    });
    let source = workspace.yaml().replacen("name: base", "name: base\n      clientCertificates:\n        - domain: disk.example.com\n          type: pkcs12\n          pkcs12FilePath: disk.p12", 1);
    fs::write(&workspace.path, source).unwrap();
    workspace.update(cx, |view, _, cx| {
        view.apply_reconciled_workspace(
            reconciled_workspace(probe_opencollection::load_workspace(&workspace.path).unwrap()),
            cx,
        );
        let dialog = view.environment_manager_dialog.as_ref().unwrap();
        assert!(dialog.certificate_editor.is_none());
        assert_eq!(dialog.draft().client_certificates.len(), 1);
        assert!(!view.environment_manager_is_dirty());
        assert!(view.environment_dialog_error.is_some());
    });
    cx.run_until_parked();
    workspace.update(cx, |view, window, cx| {
        assert_eq!(
            window.focused(cx),
            Some(view.environment_manager_dialog_focus.clone())
        );
    });
}

fn certificate_fixture_source() -> String {
    fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/opencollection/client-certificates.yml"),
    )
    .unwrap()
}

#[gpui_kit::test]
fn parent_environment_certificates_have_no_editable_rows_or_actions(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::writable_source(
        cx,
        "certificate-inheritance",
        &certificate_fixture_source(),
    );
    let before = probe_opencollection::load_workspace(&workspace.path).unwrap();
    workspace.open_manager(cx, "production");
    click(&workspace, cx, "environment-tab-2");
    assert!(
        workspace
            .visual(cx)
            .debug_bounds("environment-certificate-0")
            .is_some()
    );
    workspace.update(cx, |view, _, cx| {
        view.select_environment_manager_environment("inherited-variables", cx)
    });
    cx.run_until_parked();
    assert!(
        workspace
            .visual(cx)
            .debug_bounds("environment-certificate-0")
            .is_none()
    );
    assert!(
        workspace
            .visual(cx)
            .debug_bounds("environment-certificates")
            .is_some()
    );
    for action in [
        "certificate-edit-0",
        "certificate-delete-0",
        "certificate-toggle-0",
        "certificate-up-0",
        "certificate-down-0",
    ] {
        assert!(
            workspace.visual(cx).debug_bounds(action).is_none(),
            "parent action must not render: {action}"
        );
    }
    workspace.update(cx, |view, window, cx| {
        view.open_certificate_editor(Some(0), window, cx);
        assert!(
            view.environment_manager_dialog
                .as_ref()
                .unwrap()
                .certificate_editor
                .is_none()
        );
        assert!(!view.environment_manager_is_dirty());
    });
    click(&workspace, cx, "certificate-add");
    fill(&workspace, cx, 0, "child.example.com");
    fill(&workspace, cx, 1, "child.pem");
    fill(&workspace, cx, 2, "child-key.pem");
    click(&workspace, cx, "certificate-save");
    workspace.update(cx, |view, window, cx| {
        view.save_environment_manager_dialog(window, cx)
    });
    cx.run_until_parked();
    let after = probe_opencollection::load_workspace(&workspace.path).unwrap();
    for original in before
        .workspace()
        .environments()
        .iter()
        .filter(|environment| environment.name != "inherited-variables")
    {
        assert_eq!(
            after
                .workspace()
                .environments()
                .iter()
                .find(|environment| environment.name == original.name),
            Some(original)
        );
    }
    assert_eq!(certificates(&workspace, cx).len(), 1);
}

#[gpui_kit::test]
fn certificate_save_trims_paths_and_domain_and_preserves_or_clears_passphrases(
    cx: &mut TestAppContext,
) {
    let source = certificate_fixture_source().replacen(
        "privateKeyFilePath: ./certs/dev-key.pem",
        "privateKeyFilePath: ./certs/dev-key.pem\n          passphrase: ''",
        1,
    );
    let workspace = EnvironmentWorkspace::writable_source(cx, "certificate-normalization", &source);
    for (environment, pkcs12) in [("development", false), ("production", true)] {
        workspace.open_manager(cx, environment);
        click(&workspace, cx, "environment-tab-2");
        click(&workspace, cx, "certificate-edit-0");
        fill(&workspace, cx, 0, "  {{domain}} \t");
        if pkcs12 {
            fill(&workspace, cx, 3, "  {{pkcs12Path}}  ");
        } else {
            fill(&workspace, cx, 1, "  {{certPath}}  ");
            fill(&workspace, cx, 2, "  certs/client key.pem  ");
        }
        click(&workspace, cx, "certificate-save");
        workspace.update(cx, |view, window, cx| {
            view.save_environment_manager_dialog(window, cx)
        });
        cx.run_until_parked();
        let loaded = probe_opencollection::load_workspace(&workspace.path).unwrap();
        let certificate = &loaded
            .workspace()
            .environments()
            .iter()
            .find(|entry| entry.name == environment)
            .unwrap()
            .client_certificates[0];
        match certificate {
            ClientCertificate::Pem {
                domain,
                certificate_file_path,
                private_key_file_path,
                passphrase,
                ..
            } => {
                assert_eq!(domain, "{{domain}}");
                assert_eq!(certificate_file_path, "{{certPath}}");
                assert_eq!(private_key_file_path, "certs/client key.pem");
                assert_eq!(passphrase.as_deref(), Some(""));
            }
            ClientCertificate::Pkcs12 {
                domain,
                pkcs12_file_path,
                passphrase,
                ..
            } => {
                assert_eq!(domain, "{{domain}}");
                assert_eq!(pkcs12_file_path, "{{pkcs12Path}}");
                assert_eq!(passphrase.as_deref(), Some(""));
            }
        }
        click(&workspace, cx, "certificate-edit-0");
        click(&workspace, cx, "certificate-field-4");
        cx.simulate_input(workspace.window.into(), "  literal passphrase  ");
        cx.run_until_parked();
        workspace.update(cx, |view, _, cx| {
            let certificate = view
                .environment_manager_dialog
                .as_ref()
                .unwrap()
                .certificate_editor
                .as_ref()
                .unwrap()
                .certificate(cx)
                .unwrap();
            let (ClientCertificate::Pem { passphrase, .. }
            | ClientCertificate::Pkcs12 { passphrase, .. }) = certificate;
            assert_eq!(passphrase.as_deref(), Some("  literal passphrase  "));
        });
        // Explicitly clearing a previously empty passphrase after typing removes the field.
        cx.simulate_keystrokes(
            workspace.window.into(),
            if cfg!(target_os = "macos") {
                "cmd-a"
            } else {
                "ctrl-a"
            },
        );
        cx.simulate_keystrokes(workspace.window.into(), "backspace");
        cx.run_until_parked();
        click(&workspace, cx, "certificate-save");
        workspace.update(cx, |view, window, cx| {
            view.save_environment_manager_dialog(window, cx)
        });
        cx.run_until_parked();
        let loaded = probe_opencollection::load_workspace(&workspace.path).unwrap();
        let certificate = &loaded
            .workspace()
            .environments()
            .iter()
            .find(|entry| entry.name == environment)
            .unwrap()
            .client_certificates[0];
        let (ClientCertificate::Pem { passphrase, .. }
        | ClientCertificate::Pkcs12 { passphrase, .. }) = certificate;
        assert_eq!(passphrase, &None);
    }
}

#[gpui_kit::test]
fn certificate_required_field_feedback_tracks_type_and_whitespace(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::open(cx);
    workspace.open_manager(cx, "base");
    click(&workspace, cx, "environment-tab-2");
    click(&workspace, cx, "certificate-add");
    assert!(
        workspace
            .visual(cx)
            .debug_bounds("certificate-required-field")
            .is_none(),
        "a pristine form must not show validation feedback"
    );
    click(&workspace, cx, "certificate-field-0");
    cx.simulate_input(workspace.window.into(), " ");
    cx.run_until_parked();
    for (expected, field, value) in [
        ("Domain", 0, " \t "),
        ("Domain", 0, "api.example.com"),
        ("Certificate", 1, "cert.pem"),
        ("Private Key", 2, "key.pem"),
    ] {
        workspace.update(cx, |view, _, cx| {
            let editor = view
                .environment_manager_dialog
                .as_ref()
                .unwrap()
                .certificate_editor
                .as_ref()
                .unwrap();
            assert_eq!(editor.missing_required_field(cx), Some(expected));
            assert!(editor.certificate(cx).is_none());
        });
        assert!(
            workspace
                .visual(cx)
                .debug_bounds("certificate-required-field")
                .is_some()
        );
        fill(&workspace, cx, field, value);
    }
    assert!(
        workspace
            .visual(cx)
            .debug_bounds("certificate-required-field")
            .is_none()
    );
    click(&workspace, cx, "certificate-type-1");
    workspace.update(cx, |view, _, cx| {
        assert_eq!(
            view.environment_manager_dialog
                .as_ref()
                .unwrap()
                .certificate_editor
                .as_ref()
                .unwrap()
                .missing_required_field(cx),
            Some("PKCS#12 File")
        )
    });
    assert!(
        workspace
            .visual(cx)
            .debug_bounds("certificate-required-field")
            .is_some()
    );
    fill(&workspace, cx, 3, " {{archive}} ");
    assert!(
        workspace
            .visual(cx)
            .debug_bounds("certificate-required-field")
            .is_none()
    );
}

#[gpui_kit::test]
fn certificate_dialog_escape_cancel_and_save_preserve_outer_draft(cx: &mut TestAppContext) {
    cx.update(bind_platform_hotkeys);
    let workspace = EnvironmentWorkspace::writable(cx, "certificate-modal");
    let original_yaml = workspace.yaml();
    workspace.open_manager(cx, "base");
    click(&workspace, cx, "environment-tab-2");
    assert!(
        workspace
            .visual(cx)
            .debug_bounds("certificate-order-description")
            .is_some()
    );
    click(&workspace, cx, "certificate-add");
    assert!(
        workspace
            .visual(cx)
            .debug_bounds("environment-certificates")
            .is_some(),
        "the list stays behind the nested dialog"
    );
    let restore_focus = workspace.update(cx, |view, _, _| {
        Some(
            view.environment_manager_dialog
                .as_ref()
                .unwrap()
                .certificate_editor
                .as_ref()
                .unwrap()
                .restore_focus
                .clone(),
        )
    });
    fill(&workspace, cx, 0, "discard.example.com");
    fill(&workspace, cx, 1, "client.pem");
    fill(&workspace, cx, 2, "key.pem");
    // A click behind the modal must not switch tabs.
    click(&workspace, cx, "environment-tab-0");
    workspace.update(cx, |view, window, cx| {
        view.select_environment_manager_environment("development", cx);
        view.request_close_environment_manager_dialog(window, cx);
        view.save_environment_manager_dialog(window, cx);
        let dialog = view.environment_manager_dialog.as_ref().unwrap();
        assert_eq!(dialog.tab, EnvironmentTab::ClientCertificates);
        assert_eq!(dialog.draft().name, "base");
        assert!(dialog.certificate_editor.is_some());
        assert!(view.application_dialog.is_none());
        assert!(view.environment_save_task.is_none());
        assert!(view.environment_manager_is_dirty());
        assert!(view.environment_manager_save_disabled());
    });
    cx.simulate_keystrokes(workspace.window.into(), "escape");
    cx.run_until_parked();
    workspace.update(cx, |view, window, cx| {
        let dialog = view.environment_manager_dialog.as_ref().unwrap();
        assert!(dialog.certificate_editor.is_none());
        assert!(dialog.draft().client_certificates.is_empty());
        assert!(!view.environment_manager_is_dirty());
        assert_eq!(window.focused(cx), restore_focus);
    });
    assert!(
        workspace
            .visual(cx)
            .debug_bounds("certificate-dialog")
            .is_none()
    );
    click(&workspace, cx, "certificate-add");
    fill(&workspace, cx, 0, "saved.example.com");
    fill(&workspace, cx, 1, "{{certPath}}");
    fill(&workspace, cx, 2, "key.pem");
    click(&workspace, cx, "certificate-save");
    assert_eq!(certificates(&workspace, cx).len(), 1);
    assert_eq!(
        workspace.yaml(),
        original_yaml,
        "inner Save changes only the draft"
    );
    assert!(
        workspace
            .visual(cx)
            .debug_bounds("certificate-dialog")
            .is_none()
    );
    click(&workspace, cx, "certificate-edit-0");
    assert!(
        workspace
            .visual(cx)
            .debug_bounds("certificate-dialog")
            .is_some()
    );
    fill(&workspace, cx, 0, "cancelled.example.com");
    click(&workspace, cx, "certificate-cancel");
    assert!(
        matches!(&certificates(&workspace, cx)[0], ClientCertificate::Pem { domain, .. } if domain == "saved.example.com")
    );
    click(&workspace, cx, "certificate-edit-0");
    fill(&workspace, cx, 0, "edited.example.com");
    cx.simulate_keystrokes(workspace.window.into(), "enter");
    cx.run_until_parked();
    assert!(
        matches!(&certificates(&workspace, cx)[0], ClientCertificate::Pem { domain, .. } if domain == "edited.example.com")
    );
    assert_eq!(workspace.yaml(), original_yaml);
}

#[gpui_kit::test]
fn certificate_list_scrollbar_reaches_overflow_rows(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::open(cx);
    workspace.open_manager(cx, "base");
    click(&workspace, cx, "environment-tab-2");
    workspace.update(cx, |view, _, _| {
        assert_eq!(
            view.environment_manager_dialog
                .as_ref()
                .unwrap()
                .certificates_scroll
                .max_offset()
                .y,
            px(0.0)
        );
    });
    workspace.update(cx, |view, _, cx| {
        view.apply_environment_manager_draft(cx, |dialog| {
            dialog
                .draft_mut()
                .client_certificates
                .extend((0..40).map(|index| ClientCertificate::Pkcs12 {
                    domain: format!("api-{index}.example.com"),
                    pkcs12_file_path: "{{certPath}}/client.p12".into(),
                    passphrase: None,
                    disabled: false,
                }));
        });
    });
    let mut visual = workspace.visual(cx);
    visual.run_until_parked();
    let list = visual.debug_bounds("environment-certificates").unwrap();
    let assert_footer_visible = |visual: &mut VisualTestContext| {
        let dialog = visual.debug_bounds("environment-manager-dialog").unwrap();
        let list = visual.debug_bounds("environment-certificates").unwrap();
        let footer = visual.debug_bounds("certificate-list-footer").unwrap();
        let add = visual.debug_bounds("certificate-add").unwrap();
        assert!(footer.is_contained_within(&dialog));
        assert!(add.is_contained_within(&footer));
        assert!(
            !footer.intersects(&list),
            "the footer stays outside the scrolling viewport"
        );
    };
    assert_footer_visible(&mut visual);
    let last_row = visual.debug_bounds("environment-certificate-39").unwrap();
    assert!(
        !list.intersects(&last_row),
        "the last row starts below the fold"
    );
    let max_offset = workspace.update(cx, |view, _, _| {
        view.environment_manager_dialog
            .as_ref()
            .unwrap()
            .certificates_scroll
            .max_offset()
            .y
    });
    assert!(max_offset > px(0.0));
    visual.simulate_event(gpui_kit::ScrollWheelEvent {
        position: list.center(),
        delta: gpui_kit::ScrollDelta::Pixels(point(px(0.0), -max_offset)),
        modifiers: Modifiers::default(),
        touch_phase: gpui_kit::TouchPhase::Started,
    });
    visual.run_until_parked();
    let last_row = visual.debug_bounds("environment-certificate-39").unwrap();
    assert!(
        list.contains(&last_row.center()),
        "the wheel reaches overflow rows"
    );
    assert_footer_visible(&mut visual);
    let wheel_offset = workspace.update(cx, |view, _, _| {
        view.environment_manager_dialog
            .as_ref()
            .unwrap()
            .certificates_scroll
            .offset()
            .y
    });
    assert!(wheel_offset < px(0.0));
    // Use the scrollbar's handle API instead of depending on track geometry.
    workspace.update(cx, |view, _, cx| {
        let scroll = &view
            .environment_manager_dialog
            .as_ref()
            .unwrap()
            .certificates_scroll;
        gpui_kit::base::ScrollbarHandle::set_offset(scroll, point(px(0.0), px(0.0)));
        cx.notify();
    });
    visual.run_until_parked();
    workspace.update(cx, |view, _, _| {
        assert!(
            view.environment_manager_dialog
                .as_ref()
                .unwrap()
                .certificates_scroll
                .offset()
                .y
                > wheel_offset
        );
    });
    let first_row = visual.debug_bounds("environment-certificate-0").unwrap();
    let last_row = visual.debug_bounds("environment-certificate-39").unwrap();
    assert!(
        list.contains(&first_row.center()),
        "the scrollbar handle controls the rendered list"
    );
    assert!(!list.intersects(&last_row));
    assert_footer_visible(&mut visual);
    click(&workspace, cx, "certificate-add");
    workspace.update(cx, |view, _, _| {
        assert!(
            view.environment_manager_dialog
                .as_ref()
                .unwrap()
                .certificate_editor
                .is_some(),
            "Add Certificate remains clickable after scrolling"
        );
    });
}

fn editor(view: &mut ProbeApp) -> &mut crate::app::CertificateEditor {
    view.environment_manager_dialog
        .as_mut()
        .unwrap()
        .certificate_editor
        .as_mut()
        .unwrap()
}

fn fixture_path(filename: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/mtls")
        .join(filename)
        .to_str()
        .unwrap()
        .into()
}

fn assert_visible(
    workspace: &EnvironmentWorkspace,
    cx: &mut TestAppContext,
    selector: &'static str,
    visible: bool,
) {
    assert_eq!(
        workspace.visual(cx).debug_bounds(selector).is_some(),
        visible,
        "{selector}"
    );
}

#[gpui_kit::test]
fn inspection_generations_reject_stale_results_and_save_ignores_failure(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::writable(cx, "certificate-inspection-state");
    workspace.open_manager(cx, "base");
    click(&workspace, cx, "environment-tab-2");
    click(&workspace, cx, "certificate-add");
    fill(&workspace, cx, 0, "localhost");
    fill(&workspace, cx, 1, "missing.pem");
    fill(&workspace, cx, 2, "missing-key.pem");
    let failure = || probe_http::CertificateInspection {
        metadata: None,
        validation: Err(probe_http::HttpError::ClientConfiguration(
            "cannot read client certificate file".into(),
        )),
    };
    let generation = workspace.update(cx, |view, _, cx| {
        let editor = editor(view);
        let generation = editor.inspection_generation.clone();
        editor.inspecting = true;
        assert!(editor.accept_inspection(&generation, failure(), None, true));
        assert!(editor.inspection.is_some());
        assert!(editor.monitoring_unavailable);
        cx.notify();
        generation
    });
    assert_visible(&workspace, cx, "certificate-validation-failure", true);
    assert_visible(&workspace, cx, "certificate-validation-success", false);
    assert_visible(&workspace, cx, "certificate-monitoring-warning", true);
    workspace.update(cx, |view, window, cx| {
        editor(view).inputs[1].update(cx, |input, cx| {
            input.replace_all("edited-unsaved.pem", window, cx)
        });
    });
    cx.run_until_parked();
    workspace.update(cx, |view, _, cx| {
        let dialog = view.environment_manager_dialog.as_mut().unwrap();
        let editor = dialog.certificate_editor.as_mut().unwrap();
        assert!(editor.inspection.is_none());
        assert!(!editor.monitoring_unavailable);
        assert!(!editor.accept_inspection(&generation, failure(), None, false));
        assert!(!editor.monitoring_unavailable);
        let current = editor.inspection_generation.clone();
        assert!(editor.accept_inspection(&current, failure(), None, false));
        assert!(!editor.monitoring_unavailable);
        let draft = editor.certificate(cx).unwrap();
        assert!(matches!(draft, ClientCertificate::Pem { certificate_file_path, .. } if certificate_file_path == "edited-unsaved.pem"));
        let old_cache = dialog.certificate_generation.clone();
        dialog.draft_mut().color = Some("blue".into());
        assert!(!std::rc::Rc::ptr_eq(&old_cache, &dialog.certificate_generation));
        assert!(dialog.certificate_metadata.is_none());
        assert!(!dialog.certificate_editor.as_mut().unwrap().accept_inspection(&current, failure(), None, true));
        assert!(!dialog.certificate_editor.as_ref().unwrap().monitoring_unavailable);
        let editor = dialog.certificate_editor.as_mut().unwrap();
        let current = editor.inspection_generation.clone();
        editor.accept_inspection(&current, failure(), None, false);
    });
    click(&workspace, cx, "certificate-save");
    assert!(certificates(&workspace, cx).iter().any(|cert| matches!(cert, ClientCertificate::Pem { certificate_file_path, .. } if certificate_file_path == "edited-unsaved.pem")));
    click(&workspace, cx, "certificate-add");
    workspace.update(cx, |view, _, _| {
        assert!(!editor(view).accept_inspection(&generation, failure(), None, false));
    });
    click(&workspace, cx, "certificate-cancel");
}

#[gpui_kit::test]
fn inspect_reads_unsaved_fields_and_copies_public_fingerprint(cx: &mut TestAppContext) {
    // Watcher notifications and teardown wake GPUI from an OS thread.
    cx.executor().allow_parking();
    let workspace = EnvironmentWorkspace::writable(cx, "certificate-inspect-unsaved");
    workspace.open_manager(cx, "base");
    click(&workspace, cx, "environment-tab-2");
    click(&workspace, cx, "certificate-add");
    fill(&workspace, cx, 0, "localhost");
    let directory = tempfile::tempdir().unwrap();
    let certificate = directory.path().canonicalize().unwrap().join("client.pem");
    fs::copy(fixture_path("client.pem"), &certificate).unwrap();
    fill(&workspace, cx, 1, certificate.to_str().unwrap());
    fill(&workspace, cx, 2, &fixture_path("client-encrypted-key.pem"));
    fill(&workspace, cx, 4, "probe-test-only-password");
    let before = certificates(&workspace, cx);
    click(&workspace, cx, "certificate-inspect");
    workspace.update(cx, |view, _, _| {
        let editor = editor(view);
        let result = editor
            .inspection
            .as_ref()
            .expect("background inspection completed");
        assert!(result.validation.is_ok());
        assert_eq!(
            result.metadata.as_ref().unwrap().subject,
            "Probe test-only client"
        );
    });
    assert_visible(&workspace, cx, "certificate-validation-success", true);
    assert_visible(&workspace, cx, "certificate-validation-failure", false);
    assert_visible(&workspace, cx, "certificate-monitoring-warning", false);
    assert_eq!(
        certificates(&workspace, cx),
        before,
        "inspection leaves the draft unchanged"
    );
    click(&workspace, cx, "certificate-copy-fingerprint");
    workspace.update(cx, |view, _, cx| {
        let editor = editor(view);
        let expected = &editor
            .inspection
            .as_ref()
            .unwrap()
            .metadata
            .as_ref()
            .unwrap()
            .fingerprint;
        assert_eq!(
            cx.read_from_clipboard().unwrap().text().as_deref(),
            Some(expected.as_str())
        );
    });
    // A real source event must wake the inspection task and clear its result.
    fs::copy(fixture_path("client.pem"), &certificate).unwrap();
    super::secret_execution::park_until(
        cx,
        |cx| workspace.update(cx, |view, _, _| editor(view).inspection.is_none()),
        "certificate source invalidation",
    );
    assert_visible(&workspace, cx, "certificate-validation-success", false);
    click(&workspace, cx, "certificate-inspect");
    workspace.update(cx, |view, _, cx| {
        let editor = editor(view);
        let result = editor.inspection.clone().unwrap();
        let generation = editor.inspection_generation.clone();
        assert!(editor.accept_inspection(&generation, result, None, true));
        cx.notify();
    });
    cx.run_until_parked();
    assert_visible(&workspace, cx, "certificate-validation-success", true);
    assert_visible(&workspace, cx, "certificate-monitoring-warning", true);
    click(&workspace, cx, "certificate-cancel");
    assert_eq!(certificates(&workspace, cx), before);
}

#[gpui_kit::test]
fn inspect_allows_missing_domain_while_save_requires_it(cx: &mut TestAppContext) {
    // Watcher notifications and teardown wake GPUI from an OS thread.
    cx.executor().allow_parking();
    let workspace = EnvironmentWorkspace::writable(cx, "certificate-inspect-without-domain");
    workspace.open_manager(cx, "base");
    click(&workspace, cx, "environment-tab-2");
    click(&workspace, cx, "certificate-add");
    fill(&workspace, cx, 1, &fixture_path("client.pem"));
    fill(&workspace, cx, 2, &fixture_path("client-key.pem"));
    workspace.update(cx, |view, _, cx| {
        let editor = editor(view);
        assert_eq!(editor.missing_required_field(cx), Some("Domain"));
        assert!(editor.missing_inspection_field(cx).is_none());
        assert!(editor.certificate(cx).is_none());
    });
    click(&workspace, cx, "certificate-inspect");
    workspace.update(cx, |view, window, cx| {
        let result = editor(view)
            .inspection
            .as_ref()
            .expect("inspect runs without a domain");
        assert!(result.validation.is_ok());
        assert_eq!(
            result.metadata.as_ref().unwrap().subject,
            "Probe test-only client"
        );
        view.save_certificate_editor(window, cx);
        let dialog = view.environment_manager_dialog.as_ref().unwrap();
        assert!(dialog.draft().client_certificates.is_empty());
        assert_eq!(
            dialog
                .certificate_editor
                .as_ref()
                .unwrap()
                .missing_required_field(cx),
            Some("Domain")
        );
    });
    click(&workspace, cx, "certificate-type-1");
    workspace.update(cx, |view, _, cx| {
        let editor = editor(view);
        assert_eq!(editor.missing_required_field(cx), Some("Domain"));
        assert_eq!(editor.missing_inspection_field(cx), Some("PKCS#12 File"));
    });
    fill(&workspace, cx, 3, &fixture_path("client-unencrypted.p12"));
    workspace.update(cx, |view, _, cx| {
        let editor = editor(view);
        assert_eq!(editor.missing_required_field(cx), Some("Domain"));
        assert!(editor.missing_inspection_field(cx).is_none());
    });
    click(&workspace, cx, "certificate-inspect");
    workspace.update(cx, |view, _, cx| {
        let editor = editor(view);
        let result = editor
            .inspection
            .as_ref()
            .expect("pkcs12 inspect runs without a domain");
        assert!(result.validation.is_ok());
        assert!(editor.certificate(cx).is_none());
    });
}

#[gpui_kit::test]
fn certificate_metadata_and_expiry_warning_survive_refresh_saves_and_reload(
    cx: &mut TestAppContext,
) {
    let workspace = EnvironmentWorkspace::writable(cx, "certificate-refresh");
    workspace.open_manager(cx, "base");
    workspace.update(cx, |view, _, _| {
        view.environment_manager_dialog
            .as_mut()
            .unwrap()
            .draft_mut()
            .client_certificates
            .push(ClientCertificate::Pem {
                domain: "localhost".into(),
                certificate_file_path: fixture_path("expired-client.pem"),
                private_key_file_path: "not-read-for-public-metadata.pem".into(),
                passphrase: None,
                disabled: false,
            });
    });
    click(&workspace, cx, "environment-tab-2");
    assert_visible(&workspace, cx, "certificate-validity-warning-0", true);
    let fingerprint = workspace.update(cx, |view, _, cx| {
        let dialog = view.environment_manager_dialog.as_mut().unwrap();
        let fingerprint = dialog
            .certificate_metadata
            .as_ref()
            .unwrap()
            .last()
            .unwrap()
            .metadata
            .as_ref()
            .unwrap()
            .fingerprint
            .clone();
        // Start the next refresh, checking before its background work can finish.
        dialog.certificate_metadata_refresh_pending = false;
        view.load_certificate_metadata(cx);
        let dialog = view.environment_manager_dialog.as_ref().unwrap();
        assert!(dialog.certificate_metadata_refresh_pending);
        assert_eq!(
            dialog
                .certificate_metadata
                .as_ref()
                .unwrap()
                .last()
                .unwrap()
                .metadata
                .as_ref()
                .unwrap()
                .fingerprint,
            fingerprint
        );
        fingerprint
    });
    cx.run_until_parked();
    workspace.update(cx, |view, window, cx| {
        let index = view
            .environment_manager_dialog
            .as_ref()
            .unwrap()
            .draft()
            .client_certificates
            .len()
            - 1;
        view.open_certificate_editor(Some(index), window, cx);
        view.save_certificate_editor(window, cx);
        assert_eq!(
            cached_fingerprint(view.environment_manager_dialog.as_ref().unwrap()),
            Some(fingerprint.as_str())
        );

        view.open_certificate_editor(Some(index), window, cx);
        editor(view).inputs[4].update(cx, |input, cx| {
            input.set_value("unused-passphrase", window, cx)
        });
        view.save_certificate_editor(window, cx);
        let dialog = view.environment_manager_dialog.as_ref().unwrap();
        assert_eq!(
            cached_fingerprint(dialog),
            Some(fingerprint.as_str()),
            "key configuration does not invalidate public metadata"
        );
        let refreshed = dialog.refreshed(dialog.draft());
        assert_eq!(
            cached_fingerprint(&refreshed),
            Some(fingerprint.as_str()),
            "save completion must not introduce a loading frame"
        );
        assert!(!std::rc::Rc::ptr_eq(
            &dialog.certificate_generation,
            &refreshed.certificate_generation
        ));
        let mut changed = dialog.draft().clone();
        changed.variables.clear();
        changed.extends = Some("different-parent".into());
        assert!(
            dialog.refreshed(&changed).certificate_metadata.is_none(),
            "changed resolution inputs invalidate metadata"
        );
        view.save_environment_manager_dialog(window, cx);
    });
    cx.run_until_parked();
    let fresh = probe_opencollection::load_workspace(&workspace.path).unwrap();
    workspace.update(cx, |view, window, cx| {
        assert_eq!(
            cached_fingerprint(view.environment_manager_dialog.as_ref().unwrap()),
            Some(fingerprint.as_str())
        );
        view.reconcile_filesystem_workspace(fresh, Default::default(), window, cx);
        assert_eq!(
            cached_fingerprint(view.environment_manager_dialog.as_ref().unwrap()),
            Some(fingerprint.as_str()),
            "an unchanged watcher reload must not clear metadata"
        );
    });
    cx.run_until_parked();
    assert_visible(&workspace, cx, "certificate-validity-warning-0", true);
}

fn cached_fingerprint(dialog: &crate::app::EnvironmentManagerDialog) -> Option<&str> {
    dialog
        .certificate_metadata
        .as_ref()?
        .last()?
        .metadata
        .as_ref()
        .map(|metadata| metadata.fingerprint.as_str())
}

#[gpui_kit::test]
fn certificate_list_mutations_keep_metadata_with_surviving_rows(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::writable(cx, "certificate-delete-metadata");
    workspace.open_manager(cx, "base");
    workspace.update(cx, |view, _, _| {
        view.environment_manager_dialog
            .as_mut()
            .unwrap()
            .draft_mut()
            .client_certificates = ["client.pem", "ec-client.pem", "server.pem"]
            .into_iter()
            .map(|filename| ClientCertificate::Pem {
                domain: filename.into(),
                certificate_file_path: fixture_path(filename),
                private_key_file_path: "not-read-for-preview.pem".into(),
                passphrase: None,
                disabled: false,
            })
            .collect();
    });
    click(&workspace, cx, "environment-tab-2");
    let labels = |dialog: &crate::app::EnvironmentManagerDialog| {
        dialog
            .certificate_metadata
            .as_ref()
            .expect("metadata stays cached")
            .iter()
            .map(|row| {
                let metadata = row.metadata.as_ref().expect("public fixture metadata");
                (metadata.subject.clone(), metadata.expires.clone())
            })
            .collect::<Vec<_>>()
    };
    let original = workspace.update(cx, |view, _, _| {
        labels(view.environment_manager_dialog.as_ref().unwrap())
    });
    assert_eq!(original.len(), 3);
    workspace.update(cx, |view, _, cx| {
        let generation = view
            .environment_manager_dialog
            .as_ref()
            .unwrap()
            .certificate_generation
            .clone();
        view.apply_environment_manager_draft(cx, |dialog| dialog.remove_certificate(0));
        let dialog = view.environment_manager_dialog.as_ref().unwrap();
        assert_eq!(
            labels(dialog),
            original[1..],
            "deletion must preserve labels before background work runs"
        );
        assert!(
            !std::rc::Rc::ptr_eq(&generation, &dialog.certificate_generation),
            "reject pending results for the previous row order"
        );
        view.apply_environment_manager_draft(cx, |dialog| dialog.swap_certificates(0, 1));
        assert_eq!(
            labels(view.environment_manager_dialog.as_ref().unwrap()),
            vec![original[2].clone(), original[1].clone()]
        );
        view.apply_environment_manager_draft(cx, |dialog| dialog.set_certificate_enabled(0, false));
        assert_eq!(
            labels(view.environment_manager_dialog.as_ref().unwrap()),
            vec![original[2].clone(), original[1].clone()]
        );
    });
    cx.run_until_parked();
    click(&workspace, cx, "certificate-delete-0");
    workspace.update(cx, |view, _, _| {
        let dialog = view.environment_manager_dialog.as_ref().unwrap();
        assert_eq!(labels(dialog), vec![original[1].clone()]);
        assert!(matches!(&dialog.draft().client_certificates[0], ClientCertificate::Pem { domain, .. } if domain == "ec-client.pem"));
    });
}

#[gpui_kit::test]
fn active_certificate_popover_opens_environment_management_without_secret_reads(
    cx: &mut TestAppContext,
) {
    let environment = format!("Development{}", "a".repeat(80));
    let hostname = format!("{}.example.com", "a".repeat(63));
    let source = include_str!("../../../../../../tests/fixtures/opencollection/mtls-variables.yml")
        .replace("parent", &environment)
        .replace("api.example.com", &hostname);
    let workspace = EnvironmentWorkspace::writable_source(cx, "active-certificate", &source);
    workspace.update(cx, |view, _, cx| {
        let key = view.loaded_workspace.as_ref().unwrap().requests()[0].key();
        view.select_environment(Some(environment.clone()), cx);
        view.select_request(key, cx);
    });
    secret_execution::park_until(
        cx,
        |cx| {
            workspace
                .visual(cx)
                .debug_bounds("active-certificate-indicator")
                .is_some()
        },
        "active certificate indicator",
    );
    click(&workspace, cx, "active-certificate-indicator");
    assert_visible(&workspace, cx, "active-certificate-manage", true);
    assert_visible(
        &workspace,
        cx,
        "active-certificate-metadata-unavailable",
        true,
    );
    for selector in [
        "active-certificate-value-Subject",
        "active-certificate-value-Issuer",
        "active-certificate-value-Expiry",
    ] {
        assert_visible(&workspace, cx, selector, false);
    }
    let mut visual = workspace.visual(cx);
    let popup = visual.debug_bounds("active-certificate-details").unwrap();
    for selector in [
        "active-certificate-value-File",
        "active-certificate-value-Source",
        "active-certificate-value-Match",
    ] {
        let value = visual.debug_bounds(selector).unwrap();
        assert!(
            value.left() >= popup.left() && value.right() <= popup.right(),
            "long metadata stays inside the popover: {selector}"
        );
    }

    click(&workspace, cx, "active-certificate-manage");
    workspace.update(cx, |view, _, _| {
        let dialog = view.environment_manager_dialog.as_ref().unwrap();
        assert_eq!(dialog.tab, EnvironmentTab::ClientCertificates);
        assert_eq!(dialog.draft().name, environment);
    });
    assert_eq!(workspace.store.get_calls(), 0);
}
