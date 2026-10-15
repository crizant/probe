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
fn certificate_tabs_keep_variables_and_secrets_and_mask_passphrases(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::writable(cx, "certificate-templates");
    workspace.open_manager(cx, "base");
    assert!(
        workspace
            .visual(cx)
            .debug_bounds("environment-variable-value-host")
            .is_some()
    );
    assert!(
        workspace
            .visual(cx)
            .debug_bounds("environment-secret-set-secretToken")
            .is_none()
    );
    assert!(
        workspace
            .visual(cx)
            .debug_bounds("environment-manager-add-variable")
            .is_some()
    );
    assert!(
        workspace
            .visual(cx)
            .debug_bounds("environment-manager-add-secret")
            .is_none()
    );
    click(&workspace, cx, "environment-tab-1");
    assert!(
        workspace
            .visual(cx)
            .debug_bounds("environment-secret-set-secretToken")
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
            .debug_bounds("environment-manager-add-variable")
            .is_none()
    );
    assert!(
        workspace
            .visual(cx)
            .debug_bounds("environment-manager-add-secret")
            .is_some()
    );
    click(&workspace, cx, "environment-tab-2");
    assert!(
        workspace
            .visual(cx)
            .debug_bounds("certificate-list-footer")
            .is_some()
    );
    click(&workspace, cx, "certificate-add");
    assert!(
        workspace
            .visual(cx)
            .debug_bounds("certificate-dialog")
            .is_some()
    );
    assert!(
        workspace
            .visual(cx)
            .debug_bounds("environment-certificates")
            .is_some(),
        "the list stays behind the nested dialog"
    );
    fill(&workspace, cx, 0, "api.example.com");
    fill(&workspace, cx, 1, "{{certPath}}");
    // Required private key is still absent; Save must not commit an invalid declaration.
    workspace.update(cx, |view, window, cx| {
        view.save_certificate_editor(window, cx)
    });
    assert!(certificates(&workspace, cx).is_empty());
    fill(&workspace, cx, 2, "certs/client-key.pem");
    workspace.update(cx, |view, _, cx| {
        let editor = view
            .environment_manager_dialog
            .as_ref()
            .unwrap()
            .certificate_editor
            .as_ref()
            .unwrap();
        assert!(matches!(
            editor.certificate(cx),
            Some(ClientCertificate::Pem {
                passphrase: None,
                ..
            })
        ));
    });
    fill(&workspace, cx, 4, "{{certPassword}}");
    workspace.update(cx, |view, _, cx| {
        let editor = view
            .environment_manager_dialog
            .as_ref()
            .unwrap()
            .certificate_editor
            .as_ref()
            .unwrap();
        assert!(editor.inputs[4].read(cx).presentation().is_masked());
        assert!(view.environment_manager_is_dirty());
        assert!(view.environment_manager_save_disabled());
    });
    click(&workspace, cx, "certificate-reveal");
    workspace.update(cx, |view, _, cx| {
        assert!(
            !view
                .environment_manager_dialog
                .as_ref()
                .unwrap()
                .certificate_editor
                .as_ref()
                .unwrap()
                .inputs[4]
                .read(cx)
                .presentation()
                .is_masked()
        )
    });
    click(&workspace, cx, "certificate-reveal");
    workspace.update(cx, |view, _, cx| {
        assert!(
            view.environment_manager_dialog
                .as_ref()
                .unwrap()
                .certificate_editor
                .as_ref()
                .unwrap()
                .inputs[4]
                .read(cx)
                .presentation()
                .is_masked()
        );
    });
    click(&workspace, cx, "certificate-save");
    assert!(
        matches!(&certificates(&workspace, cx)[0], ClientCertificate::Pem { certificate_file_path, passphrase, .. } if certificate_file_path == "{{certPath}}" && passphrase.as_deref() == Some("{{certPassword}}"))
    );
    click(&workspace, cx, "certificate-edit-0");
    fill(&workspace, cx, 4, "literal password");
    click(&workspace, cx, "certificate-cancel");
    assert!(
        matches!(&certificates(&workspace, cx)[0], ClientCertificate::Pem { passphrase, .. } if passphrase.as_deref() == Some("{{certPassword}}"))
    );
    click(&workspace, cx, "environment-tab-0");
    assert!(
        workspace
            .visual(cx)
            .debug_bounds("environment-variable-value-host")
            .is_some()
    );
    workspace.update(cx, |view, window, cx| {
        view.save_environment_manager_dialog(window, cx)
    });
    cx.run_until_parked();
    let saved = probe_opencollection::load_workspace(&workspace.path).unwrap();
    assert_eq!(
        saved
            .workspace()
            .environments()
            .iter()
            .find(|environment| environment.name == "base")
            .unwrap()
            .client_certificates,
        certificates(&workspace, cx)
    );
    assert_eq!(workspace.store.get_calls(), 0);
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
    assert!(
        certificates(&workspace, cx).is_empty(),
        "parent certificates must not appear as editable entries"
    );
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
    use crate::app::chrome::certificates::{
        CERTIFICATE_MOVE_DOWN, CERTIFICATE_MOVE_UP, CERTIFICATE_ORDER_DESCRIPTION,
    };
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
    assert!(CERTIFICATE_ORDER_DESCRIPTION.contains("top to bottom"));
    assert!(CERTIFICATE_ORDER_DESCRIPTION.contains("first matching enabled"));
    assert!(CERTIFICATE_MOVE_UP.contains("higher matching priority"));
    assert!(CERTIFICATE_MOVE_DOWN.contains("lower matching priority"));
    click(&workspace, cx, "certificate-add");
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
