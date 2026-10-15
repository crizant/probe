use super::*;
use probe_core::ClientCertificate;

impl ProbeApp {
    pub(in crate::app::chrome) fn render_certificate_tab(
        &self,
        theme: Theme,
        busy: bool,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let Some(dialog) = self.environment_manager_dialog.as_ref() else {
            return div().into_any_element();
        };
        if let Some(editor) = dialog.certificate_editor.as_ref() {
            return self.render_certificate_form(theme, editor, cx);
        }
        let mut list = div()
            .id("environment-certificates")
            .debug_selector(|| "environment-certificates".into())
            .flex_1()
            .min_h(px(0.0))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(theme.metrics.spacing_2));
        for (index, certificate) in dialog.draft().client_certificates.iter().enumerate() {
            let (domain, kind, paths, passphrase, disabled) = match certificate {
                ClientCertificate::Pem {
                    domain,
                    certificate_file_path,
                    private_key_file_path,
                    passphrase,
                    disabled,
                } => (
                    domain,
                    "PEM",
                    vec![
                        format!("Certificate: {certificate_file_path}"),
                        format!("Private Key: {private_key_file_path}"),
                    ],
                    passphrase,
                    *disabled,
                ),
                ClientCertificate::Pkcs12 {
                    domain,
                    pkcs12_file_path,
                    passphrase,
                    disabled,
                } => (
                    domain,
                    "PKCS#12",
                    vec![format!("PKCS#12 File: {pkcs12_file_path}")],
                    passphrase,
                    *disabled,
                ),
            };
            let edit = cx.weak_entity();
            let delete = cx.weak_entity();
            let toggle = cx.weak_entity();
            let up = cx.weak_entity();
            let down = cx.weak_entity();
            let mut card = div()
                .id(("environment-certificate", index))
                .debug_selector(move || format!("environment-certificate-{index}"))
                .p(px(theme.metrics.spacing_2))
                .border_1()
                .border_color(theme.colors.borders.standard)
                .rounded(px(theme.metrics.radius_small))
                .flex()
                .flex_col()
                .gap(px(theme.metrics.spacing_1))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(theme.metrics.spacing_2))
                        .child(components::truncated_label(domain.clone()).flex_1())
                        .child(kind)
                        .child(components::editor_action_button(
                            theme,
                            format!("certificate-toggle-{index}"),
                            if disabled { "Disabled" } else { "Enabled" },
                            busy,
                            move |_, _, cx| {
                                let _ = toggle.update(cx, |view, cx| {
                                    view.apply_environment_manager_draft(cx, |dialog| {
                                        match &mut dialog.draft_mut().client_certificates[index] {
                                            ClientCertificate::Pem { disabled, .. }
                                            | ClientCertificate::Pkcs12 { disabled, .. } => {
                                                *disabled = !*disabled
                                            }
                                        }
                                    })
                                });
                            },
                        )),
                );
            for path in paths {
                card = card.child(
                    components::truncated_label(path).text_color(theme.colors.text.secondary),
                );
            }
            if passphrase.is_some() {
                card = card.child("Passphrase: ••••••••");
            }
            card = card.child(
                div()
                    .flex()
                    .gap(px(theme.metrics.spacing_2))
                    .child(components::editor_action_button(
                        theme,
                        format!("certificate-edit-{index}"),
                        "Edit",
                        busy,
                        move |_, window, cx| {
                            let _ = edit.update(cx, |view, cx| {
                                view.open_certificate_editor(Some(index), window, cx)
                            });
                        },
                    ))
                    .child(components::editor_action_button(
                        theme,
                        format!("certificate-delete-{index}"),
                        "Delete",
                        busy,
                        move |_, _, cx| {
                            let _ = delete.update(cx, |view, cx| {
                                view.apply_environment_manager_draft(cx, |dialog| {
                                    dialog.draft_mut().client_certificates.remove(index);
                                })
                            });
                        },
                    ))
                    .child(components::editor_action_button(
                        theme,
                        format!("certificate-up-{index}"),
                        "Move up",
                        busy || index == 0,
                        move |_, _, cx| {
                            let _ = up.update(cx, |view, cx| {
                                view.apply_environment_manager_draft(cx, |dialog| {
                                    if index > 0 {
                                        dialog
                                            .draft_mut()
                                            .client_certificates
                                            .swap(index, index - 1);
                                    }
                                })
                            });
                        },
                    ))
                    .child(components::editor_action_button(
                        theme,
                        format!("certificate-down-{index}"),
                        "Move down",
                        busy || index + 1 == dialog.draft().client_certificates.len(),
                        move |_, _, cx| {
                            let _ = down.update(cx, |view, cx| {
                                view.apply_environment_manager_draft(cx, |dialog| {
                                    let certificates = &mut dialog.draft_mut().client_certificates;
                                    if index + 1 < certificates.len() {
                                        certificates.swap(index, index + 1);
                                    }
                                })
                            });
                        },
                    )),
            );
            list = list.child(card);
        }
        if dialog.draft().client_certificates.is_empty() {
            list = list.child("No client certificates in this environment.");
        }
        let add = cx.weak_entity();
        div()
            .mt(px(theme.metrics.spacing_2))
            .flex_1()
            .min_h(px(0.0))
            .flex()
            .flex_col()
            .gap(px(theme.metrics.spacing_2))
            .child(list)
            .child(
                components::editor_add_button(
                    theme,
                    "certificate-add",
                    "Add Certificate",
                    move |_, window, cx| {
                        let _ = add.update(cx, |view, cx| {
                            view.open_certificate_editor(None, window, cx)
                        });
                    },
                )
                .disabled(busy),
            )
            .into_any_element()
    }

    fn render_certificate_form(
        &self,
        theme: Theme,
        editor: &CertificateEditor,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let mut form = div()
            .id("certificate-form")
            .debug_selector(|| "certificate-form".into())
            .mt(px(theme.metrics.spacing_2))
            .flex_1()
            .min_h(px(0.0))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(theme.metrics.spacing_2));
        form = form.child(components::dialog_title(
            theme,
            if editor.index.is_some() {
                "Edit Certificate"
            } else {
                "Add Certificate"
            },
        ));
        let mut types = div()
            .flex()
            .items_center()
            .gap(px(theme.metrics.spacing_2))
            .child(components::dialog_field_label(theme, "Type"));
        for (index, (label, pkcs12)) in [("PEM", false), ("PKCS#12", true)].into_iter().enumerate()
        {
            let view = cx.weak_entity();
            types = types.child(
                components::text_tab(
                    theme,
                    format!("certificate-type-{index}"),
                    label,
                    editor.pkcs12 == pkcs12,
                    index + 1,
                    2,
                    move |_, _, cx| {
                        let _ = view.update(cx, |view, cx| {
                            if let Some(editor) = view
                                .environment_manager_dialog
                                .as_mut()
                                .and_then(|dialog| dialog.certificate_editor.as_mut())
                            {
                                editor.pkcs12 = pkcs12;
                            }
                            cx.notify();
                        });
                    },
                )
                .debug_selector(move || format!("certificate-type-{index}")),
            );
        }
        let mut types = Some(types);
        let fields = if editor.pkcs12 {
            vec![
                (0, "Domain"),
                (3, "PKCS#12 File"),
                (4, "Passphrase (optional)"),
            ]
        } else {
            vec![
                (0, "Domain"),
                (1, "Certificate"),
                (2, "Private Key"),
                (4, "Passphrase (optional)"),
            ]
        };
        for (field, label) in fields {
            let input = editor.inputs[field].clone();
            let focus = input.clone();
            let selector = format!("certificate-field-{field}");
            let mut row = div()
                .flex()
                .items_center()
                .gap(px(theme.metrics.spacing_2))
                .child(
                    div()
                        .w(px(140.0))
                        .child(components::dialog_field_label(theme, label)),
                )
                .child(
                    div()
                        .id(selector.clone())
                        .debug_selector(move || selector.clone())
                        .h(px(theme.metrics.control_height))
                        .flex_1()
                        .min_w(px(0.0))
                        .px(px(theme.metrics.spacing_2))
                        .flex()
                        .items_center()
                        .border_1()
                        .border_color(theme.colors.borders.standard)
                        .rounded(px(theme.metrics.radius_small))
                        .bg(theme.colors.surfaces.raised)
                        .cursor_text()
                        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                            focus.update(cx, |input, cx| input.focus(window, cx));
                        })
                        .child(Input::new(&input)),
                );
            if (1..=3).contains(&field) {
                let view = cx.weak_entity();
                row = row.child(components::editor_action_button(
                    theme,
                    format!("certificate-browse-{field}"),
                    "Browse…",
                    false,
                    move |_, window, cx| {
                        let _ = view.update(cx, |view, cx| {
                            view.browse_certificate_path(field, window, cx)
                        });
                    },
                ));
            } else if field == 4 {
                let view = cx.weak_entity();
                row = row.child(components::editor_action_button(
                    theme,
                    "certificate-reveal",
                    if editor.revealed { "Hide" } else { "Reveal" },
                    false,
                    move |_, window, cx| {
                        let _ = view.update(cx, |view, cx| {
                            if let Some(editor) = view
                                .environment_manager_dialog
                                .as_mut()
                                .and_then(|dialog| dialog.certificate_editor.as_mut())
                            {
                                editor.revealed = !editor.revealed;
                                editor.inputs[4]
                                    .update(cx, |input, cx| input.toggle_masked(window, cx));
                            }
                            cx.notify();
                        });
                    },
                ));
            }
            form = form.child(row);
            if field == 0 {
                form = form.child(types.take().unwrap());
            }
        }
        form = form.child(components::dialog_description(theme, "Domain and file paths are required. Paths and passphrases accept {{variables}}. Prefer {{secretVariable}} for passphrases; literal values are saved in the collection."));
        let missing_field = editor.missing_required_field(cx);
        if let Some(field) = missing_field {
            form = form.child(
                components::dialog_description(theme, format!("{field} is required."))
                    .id("certificate-required-field")
                    .debug_selector(|| "certificate-required-field".into())
                    .text_color(theme.colors.status.error),
            );
        }
        let cancel = cx.weak_entity();
        let save = cx.weak_entity();
        form.child(
            components::dialog_actions(theme)
                .child(components::dialog_action_button(
                    theme,
                    "certificate-cancel",
                    "Cancel",
                    components::DialogActionStyle::Secondary,
                    None,
                    false,
                    move |_, window, cx| {
                        let _ = cancel
                            .update(cx, |view, cx| view.cancel_certificate_editor(window, cx));
                    },
                ))
                .child(components::dialog_action_button(
                    theme,
                    "certificate-save",
                    "Save",
                    components::DialogActionStyle::Primary,
                    None,
                    missing_field.is_some(),
                    move |_, window, cx| {
                        let _ =
                            save.update(cx, |view, cx| view.save_certificate_editor(window, cx));
                    },
                )),
        )
        .into_any_element()
    }
}
