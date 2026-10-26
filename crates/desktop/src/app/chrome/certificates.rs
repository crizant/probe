use super::*;
use crate::components::IconButtonHint;
use probe_core::ClientCertificate;

pub(in crate::app) const CERTIFICATE_ORDER_DESCRIPTION: &str =
    "Certificates are matched from top to bottom. The first matching enabled certificate is used.";
pub(in crate::app) const CERTIFICATE_MOVE_UP: &str = "Move up - higher matching priority";
pub(in crate::app) const CERTIFICATE_MOVE_DOWN: &str = "Move down - lower matching priority";

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
        let mut list = div()
            .id("environment-certificates")
            .debug_selector(|| "environment-certificates".into())
            .size_full()
            .min_h(px(0.0))
            .overflow_y_scroll()
            .track_scroll(&dialog.certificates_scroll)
            .flex()
            .flex_col();
        for (index, certificate) in dialog.draft().client_certificates.iter().enumerate() {
            let (domain, kind, summary, disabled) = match certificate {
                ClientCertificate::Pem {
                    domain,
                    certificate_file_path,
                    private_key_file_path,
                    disabled,
                    ..
                } => (
                    domain,
                    "PEM",
                    format!(
                        "Certificate: {} · Private Key: {}",
                        certificate_filename(certificate_file_path),
                        certificate_filename(private_key_file_path)
                    ),
                    *disabled,
                ),
                ClientCertificate::Pkcs12 {
                    domain,
                    pkcs12_file_path,
                    disabled,
                    ..
                } => (
                    domain,
                    "PKCS#12",
                    format!("PKCS#12 File: {}", certificate_filename(pkcs12_file_path)),
                    *disabled,
                ),
            };
            let edit = cx.weak_entity();
            let delete = cx.weak_entity();
            let toggle = cx.weak_entity();
            let up = cx.weak_entity();
            let down = cx.weak_entity();
            list = list.child(
                div()
                    .id(("environment-certificate", index))
                    .debug_selector(move || format!("environment-certificate-{index}"))
                    .w_full()
                    .flex_none()
                    .px(px(theme.metrics.spacing_2))
                    .py(px(theme.metrics.spacing_2))
                    .flex()
                    .items_center()
                    .gap(px(theme.metrics.spacing_2))
                    .border_b_1()
                    .border_color(theme.colors.borders.subtle)
                    .child(
                        div()
                            .w(px(44.0))
                            .flex_none()
                            .debug_selector(move || format!("certificate-toggle-{index}"))
                            .child(components::switch(
                                theme,
                                format!("certificate-enabled-{index}"),
                                format!("Enable certificate for {domain}"),
                                !disabled,
                                busy,
                                move |enabled, _, cx| {
                                    let _ = toggle.update(cx, |view, cx| {
                                        view.apply_environment_manager_draft(cx, |dialog| {
                                            dialog.set_certificate_enabled(index, enabled);
                                        })
                                    });
                                },
                            )),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .flex()
                            .flex_col()
                            .gap(px(theme.metrics.spacing_1))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(theme.metrics.spacing_2))
                                    .child(
                                        div()
                                            .flex_none()
                                            .px(px(theme.metrics.spacing_1))
                                            .py(px(1.0))
                                            .rounded(px(theme.metrics.radius_small))
                                            .border_1()
                                            .border_color(theme.colors.borders.subtle)
                                            .bg(theme.colors.surfaces.raised)
                                            .text_size(px(theme.typography.caption_size))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(theme.colors.text.secondary)
                                            .child(kind),
                                    )
                                    .child(
                                        components::truncated_label(domain.clone())
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_size(px(theme.typography.body_size))
                                            .text_color(theme.colors.text.primary)
                                            .flex_1()
                                            .min_w(px(0.0)),
                                    ),
                            )
                            .child({
                                let metadata = dialog
                                    .certificate_metadata
                                    .as_ref()
                                    .and_then(|rows| rows.get(index))
                                    .and_then(|row| row.metadata.as_ref());
                                let warning = metadata.and_then(|metadata| {
                                    metadata.validity_warning(std::time::SystemTime::now())
                                });
                                let label = metadata
                                    .map(|m| {
                                        let expires: chrono::DateTime<chrono::Utc> =
                                            m.not_after.into();
                                        format!(
                                            "{} · Expires {}",
                                            m.subject,
                                            expires.format("%-d %b %Y")
                                        )
                                    })
                                    .unwrap_or_else(|| {
                                        if dialog
                                            .certificate_metadata
                                            .as_ref()
                                            .is_some_and(|rows| rows.get(index).is_some())
                                        {
                                            "Metadata unavailable · Use Inspect in Edit".into()
                                        } else {
                                            "Loading certificate metadata…".into()
                                        }
                                    });
                                div()
                                    .min_w(px(0.0))
                                    .flex()
                                    .flex_col()
                                    .gap(px(theme.metrics.spacing_1))
                                    .child(
                                        components::truncated_label(label)
                                            .id(("certificate-metadata", index))
                                            .when_some(metadata, |label, metadata| {
                                                label.hint(
                                                    theme,
                                                    format!("Expires {}", metadata.expires),
                                                )
                                            })
                                            .text_size(px(theme.typography.caption_size))
                                            .text_color(theme.colors.text.secondary),
                                    )
                                    .when_some(warning, |summary, warning| {
                                        summary.child(
                                            div()
                                                .id(("certificate-validity-warning", index))
                                                .debug_selector(move || {
                                                    format!("certificate-validity-warning-{index}")
                                                })
                                                .flex()
                                                .items_start()
                                                .gap(px(theme.metrics.spacing_1))
                                                .text_size(px(theme.typography.caption_size))
                                                .text_color(theme.colors.status.warning)
                                                .font_weight(FontWeight::MEDIUM)
                                                .line_height(relative(1.25))
                                                .child(
                                                    components::warning_icon(theme).h(px(theme
                                                        .typography
                                                        .caption_size
                                                        * 1.25)),
                                                )
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .min_w(px(0.0))
                                                        .whitespace_normal()
                                                        .child(warning),
                                                ),
                                        )
                                    })
                            })
                            .child(
                                components::truncated_label(summary)
                                    .text_size(px(theme.typography.caption_size))
                                    .text_color(theme.colors.text.muted),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .child(
                                components::icon_button(
                                    theme,
                                    format!("certificate-up-{index}"),
                                    CERTIFICATE_MOVE_UP,
                                    components::reorder_icon(theme, true),
                                    move |_, _, cx| {
                                        let _ = up.update(cx, |view, cx| {
                                            view.apply_environment_manager_draft(cx, |dialog| {
                                                if index > 0 {
                                                    dialog.swap_certificates(index, index - 1);
                                                }
                                            })
                                        });
                                    },
                                )
                                .flex_none()
                                .disabled(busy || index == 0),
                            )
                            .child(
                                components::icon_button(
                                    theme,
                                    format!("certificate-down-{index}"),
                                    CERTIFICATE_MOVE_DOWN,
                                    components::reorder_icon(theme, false),
                                    move |_, _, cx| {
                                        let _ = down.update(cx, |view, cx| {
                                            view.apply_environment_manager_draft(cx, |dialog| {
                                                dialog.swap_certificates(index, index + 1);
                                            })
                                        });
                                    },
                                )
                                .flex_none()
                                .disabled(
                                    busy || index + 1 == dialog.draft().client_certificates.len(),
                                ),
                            )
                            .child(
                                components::icon_button(
                                    theme,
                                    format!("certificate-edit-{index}"),
                                    format!("Edit certificate for {domain}"),
                                    components::edit_icon(theme),
                                    move |_, window, cx| {
                                        let _ = edit.update(cx, |view, cx| {
                                            view.open_certificate_editor(Some(index), window, cx)
                                        });
                                    },
                                )
                                .flex_none()
                                .disabled(busy),
                            )
                            .child(
                                components::remove_row_button(
                                    theme,
                                    format!("certificate-delete-{index}"),
                                    format!("Remove certificate for {domain}"),
                                    move |_, _, cx| {
                                        let _ = delete.update(cx, |view, cx| {
                                            view.apply_environment_manager_draft(cx, |dialog| {
                                                dialog.remove_certificate(index);
                                            })
                                        });
                                    },
                                )
                                .flex_none()
                                .disabled(busy),
                            ),
                    ),
            );
        }
        if dialog.draft().client_certificates.is_empty() {
            list = list.child(
                div()
                    .px(px(theme.metrics.spacing_2))
                    .py(px(theme.metrics.spacing_2))
                    .text_color(theme.colors.text.muted)
                    .child("No client certificates in this environment."),
            );
        }
        let add = cx.weak_entity();
        let table = div()
            .id("certificate-table")
            .debug_selector(|| "certificate-table".into())
            .flex_1()
            .min_h(px(0.0))
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(px(theme.metrics.radius_small))
            .border_1()
            .border_color(theme.colors.borders.standard)
            .child(
                div()
                    .h(px(30.0))
                    .flex_none()
                    .px(px(theme.metrics.spacing_2))
                    .flex()
                    .items_center()
                    .gap(px(theme.metrics.spacing_2))
                    .bg(theme.colors.surfaces.raised)
                    .border_b_1()
                    .border_color(theme.colors.borders.subtle)
                    .text_size(px(theme.typography.caption_size))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.colors.text.muted)
                    .child(div().w(px(44.0)).child("ON"))
                    .child(div().flex_1().child("CERTIFICATE")),
            )
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.0))
                    .relative()
                    .overflow_hidden()
                    .child(list)
                    .child(
                        Scrollbar::vertical(&dialog.certificates_scroll)
                            .id("environment-manager-certificates-scrollbar")
                            .mode(ScrollbarMode::Scrolling),
                    ),
            )
            .child(
                div()
                    .id("certificate-list-footer")
                    .debug_selector(|| "certificate-list-footer".into())
                    .w_full()
                    .flex_none()
                    .px(px(theme.metrics.spacing_2))
                    .py(px(theme.metrics.spacing_2))
                    .flex()
                    .items_center()
                    .bg(theme.colors.surfaces.raised)
                    .border_t_1()
                    .border_color(theme.colors.borders.subtle)
                    .child(
                        components::text_button(
                            theme,
                            "certificate-add",
                            "+ Add Certificate",
                            move |_, window, cx| {
                                let _ = add.update(cx, |view, cx| {
                                    view.open_certificate_editor(None, window, cx)
                                });
                            },
                        )
                        .p(px(0.0))
                        .h_auto()
                        .font_weight(FontWeight::MEDIUM)
                        .disabled(busy),
                    ),
            );
        div()
            .mt(px(theme.metrics.spacing_2))
            .flex_1()
            .min_h(px(0.0))
            .flex()
            .flex_col()
            .gap(px(theme.metrics.spacing_2))
            .child(
                components::dialog_description(theme, CERTIFICATE_ORDER_DESCRIPTION)
                    .id("certificate-order-description")
                    .debug_selector(|| "certificate-order-description".into())
                    .pl(px(theme.metrics.spacing_2))
                    .flex_none()
                    .text_size(px(theme.typography.caption_size))
                    .text_color(theme.colors.text.muted),
            )
            .child(table)
            .into_any_element()
    }

    pub(in crate::app) fn render_certificate_dialog(
        &self,
        theme: Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let Some(editor) = self
            .environment_manager_dialog
            .as_ref()
            .and_then(|dialog| dialog.certificate_editor.as_ref())
        else {
            return div().into_any_element();
        };
        let mut form = div()
            .id("certificate-form")
            .debug_selector(|| "certificate-form".into())
            .min_h(px(0.0))
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
            .child(
                div()
                    .w(px(140.0))
                    .flex_none()
                    .child(components::dialog_field_label(theme, "Type")),
            );
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
                                editor.invalidate_inspection();
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
                row = row.child(components::browse_file_button(
                    theme,
                    format!("certificate-browse-{field}"),
                    format!("Browse {label}"),
                    move |_, window, cx| {
                        let _ = view.update(cx, |view, cx| {
                            view.browse_certificate_path(field, window, cx)
                        });
                    },
                ));
            } else if field == 4 {
                let view = cx.weak_entity();
                row = row.child(components::icon_button(
                    theme,
                    "certificate-reveal",
                    if editor.revealed {
                        "Hide passphrase"
                    } else {
                        "Reveal passphrase"
                    },
                    components::eye_icon(theme, editor.revealed),
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
        let missing_inspection = editor.missing_inspection_field(cx);
        if let Some(field) = missing_field.filter(|_| editor.show_validation_feedback) {
            form = form.child(
                components::dialog_description(theme, format!("{field} is required."))
                    .id("certificate-required-field")
                    .debug_selector(|| "certificate-required-field".into())
                    .text_color(theme.colors.status.error),
            );
        }
        let inspect = cx.weak_entity();
        let mut details = div()
            .flex()
            .flex_col()
            .gap(px(theme.metrics.spacing_1))
            .border_t_1()
            .border_color(theme.colors.borders.subtle)
            .pt(px(theme.metrics.spacing_2))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(components::dialog_field_label(theme, "Certificate Details"))
                    .child(
                        components::editor_action_button(
                            theme,
                            "certificate-inspect",
                            "Inspect",
                            editor.inspecting || missing_inspection.is_some(),
                            move |_, _, cx| {
                                let _ = inspect
                                    .update(cx, |view, cx| view.inspect_certificate_editor(cx));
                            },
                        )
                        .debug_selector(|| "certificate-inspect".into()),
                    ),
            );
        if let Some(metadata) = editor
            .inspection
            .as_ref()
            .and_then(|result| result.metadata.as_ref())
        {
            for (label, value) in [
                ("Subject", &metadata.subject),
                ("Issuer", &metadata.issuer),
                ("Valid From", &metadata.valid_from),
                ("Expires", &metadata.expires),
                ("Serial Number", &metadata.serial),
                ("SHA-256", &metadata.fingerprint),
            ] {
                let mut row = div()
                    .flex()
                    .gap(px(theme.metrics.spacing_2))
                    .child(
                        div()
                            .w(px(140.0))
                            .flex_none()
                            .child(components::dialog_field_label(theme, label)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .text_size(px(theme.typography.caption_size))
                            .text_color(theme.colors.text.muted)
                            .child(value.clone()),
                    );
                if label == "SHA-256" {
                    let fingerprint = value.clone();
                    row = row.child(
                        components::icon_button(
                            theme,
                            "certificate-copy-fingerprint",
                            "Copy SHA-256 fingerprint",
                            components::field_copy_icon(theme),
                            move |_, _, cx| {
                                cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(
                                    fingerprint.clone(),
                                ));
                            },
                        )
                        .flex_none(),
                    );
                }
                details = details.child(row);
            }
            if let Some(warning) = metadata.validity_warning(std::time::SystemTime::now()) {
                details = details.child(
                    components::dialog_description(theme, warning)
                        .text_color(theme.colors.status.warning),
                );
            }
        }
        let (status, color, succeeded) = if editor.inspecting {
            ("Inspecting…".to_string(), theme.colors.text.muted, None)
        } else if let Some(result) = &editor.inspection {
            match &result.validation {
                Ok(()) => (
                    "Certificate and private key match.".into(),
                    theme.colors.status.success,
                    Some(true),
                ),
                Err(error) => (error.to_string(), theme.colors.status.error, Some(false)),
            }
        } else {
            (
                "Not inspected. Inspect reads current fields and may unlock configured secrets."
                    .into(),
                theme.colors.text.muted,
                None,
            )
        };
        let mut status_line = div().flex().items_start().gap(px(theme.metrics.spacing_1));
        if let Some(succeeded) = succeeded {
            let marker = if succeeded {
                "certificate-validation-success"
            } else {
                "certificate-validation-failure"
            };
            status_line = status_line.child(
                components::certificate_validation_icon(succeeded)
                    .mt(px(2.0))
                    .text_color(color)
                    .id(marker)
                    .debug_selector(move || marker.into()),
            );
        }
        details = details.child(
            status_line.child(
                components::dialog_description(theme, status)
                    .flex_1()
                    .min_w(px(0.0))
                    .text_color(color),
            ),
        );
        if editor.monitoring_unavailable {
            details = details.child(
                components::dialog_description(
                    theme,
                    "File changes cannot be monitored. Inspect again if the certificate or private key changes.",
                )
                .text_color(theme.colors.status.warning)
                .id("certificate-monitoring-warning")
                .debug_selector(|| "certificate-monitoring-warning".into()),
            );
        }
        form = form.child(details);
        let cancel = cx.weak_entity();
        let save = cx.weak_entity();
        let form = form.child(
            components::dialog_actions(theme)
                .mt(px(theme.metrics.spacing_2))
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
                    components::shortcut_label_for_action_in_context(
                        window,
                        &SubmitCertificateDialog,
                        "CertificateDialog",
                    ),
                    missing_field.is_some(),
                    move |_, window, cx| {
                        let _ =
                            save.update(cx, |view, cx| view.save_certificate_editor(window, cx));
                    },
                )),
        );
        let content = components::dialog_surface(theme, "certificate-dialog", 680.0)
            .debug_selector(|| "certificate-dialog".into())
            .max_w(relative(0.9))
            .max_h(relative(0.9))
            .overflow_y_scroll()
            .child(form);
        components::dialog_layer(theme, &editor.focus, "CertificateDialog", content)
            .into_any_element()
    }
}

fn certificate_filename(path: &str) -> &str {
    std::path::Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
}
