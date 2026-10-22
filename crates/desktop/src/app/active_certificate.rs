//! Read-only next-request certificate preview. Never uses the native secret provider.
use super::*;
use crate::components::IconButtonHint;
use probe_application::CertificatePreview;

#[derive(Clone, PartialEq)]
struct Inputs {
    workspace: Option<PathBuf>,
    key: RequestKey,
    request: Request,
    environments: Vec<probe_core::Environment>,
    certificates: Vec<probe_core::ClientCertificate>,
    selected: Option<String>,
}

impl Inputs {
    /// Compare borrowed live values before allocating another background snapshot.
    fn matches(
        &self,
        workspace: Option<&std::path::Path>,
        key: RequestKey,
        request: &Request,
        environments: &[probe_core::Environment],
        certificates: &[probe_core::ClientCertificate],
        selected: Option<&str>,
    ) -> bool {
        self.workspace.as_deref() == workspace
            && self.key == key
            && !request.kind.is_websocket()
            && self.selected.as_deref() == selected
            && self.request.url == request.url
            && self.request.path_parameters == request.path_parameters
            && self.environments == environments
            && self.certificates == certificates
    }
}

#[derive(Default)]
pub(super) struct ActiveCertificate {
    inputs: Option<Arc<Inputs>>,
    generation: Rc<()>,
    result: Option<CertificatePreview>,
    inspection: Option<probe_http::CertificateInspection>,
    pending: bool,
    visible_while_refreshing: bool,
    open: bool,
}

impl ActiveCertificate {
    fn set_inputs(&mut self, inputs: Option<Arc<Inputs>>) {
        if self.inputs == inputs {
            return;
        }
        // A configuration save must not remove and reinsert the URL-bar control.
        // Keep its footprint, but clear all old selection and metadata immediately.
        let same_target =
            self.inputs
                .as_ref()
                .zip(inputs.as_ref())
                .is_some_and(|(before, after)| {
                    before.workspace == after.workspace
                        && before.key == after.key
                        && before.request == after.request
                        && before.selected == after.selected
                });
        let visible = same_target
            && (self.visible_while_refreshing
                || self
                    .result
                    .as_ref()
                    .is_some_and(|result| !matches!(result, CertificatePreview::Hidden)));
        *self = Self {
            inputs,
            visible_while_refreshing: visible,
            open: visible && self.open,
            ..Default::default()
        };
    }

    fn accept(
        &mut self,
        generation: &Rc<()>,
        current_matches: bool,
        result: CertificatePreview,
        inspection: Option<probe_http::CertificateInspection>,
    ) -> bool {
        if !Rc::ptr_eq(generation, &self.generation) || !current_matches {
            return false;
        }
        self.visible_while_refreshing = false;
        self.result = Some(result);
        self.inspection = inspection;
        true
    }
}

fn certificate_detail(
    theme: Theme,
    label: &'static str,
    value: &str,
    warning: Option<&'static str>,
) -> gpui_kit::Div {
    div()
        .w_full()
        .min_w(px(0.0))
        .flex()
        .items_start()
        .gap(px(theme.metrics.spacing_2))
        .child(
            div()
                .w(px(56.0))
                .flex_none()
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.colors.text.secondary)
                .child(label.to_owned()),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .flex()
                .flex_col()
                .gap(px(theme.metrics.spacing_1))
                .text_size(px(theme.typography.body_size))
                .text_color(theme.colors.text.primary)
                .child(
                    components::truncated_label(value.to_owned())
                        .id(label)
                        .debug_selector(move || format!("active-certificate-value-{label}"))
                        .w_full(),
                )
                .when_some(warning, |value, warning| {
                    value.child(
                        div()
                            .whitespace_normal()
                            .text_size(px(theme.typography.caption_size))
                            .text_color(theme.colors.status.warning)
                            .child(warning),
                    )
                }),
        )
}

impl ProbeApp {
    fn certificate_preview_inputs(&self) -> Option<Inputs> {
        let loaded = self.loaded_workspace.as_ref()?;
        let request = self.active_request()?;
        if request.kind.is_websocket() {
            return None;
        }
        let workspace = loaded.workspace();
        let selected = self.shell.selected_environment();
        if workspace.client_certificates().is_empty()
            && selected
                .and_then(|name| {
                    workspace
                        .environments()
                        .iter()
                        .find(|environment| environment.name == name)
                })
                .is_none_or(|environment| environment.client_certificates.is_empty())
        {
            return None;
        }
        Some(Inputs {
            workspace: self.workspace_path.clone(),
            key: self.shell.active_tab()?,
            // Body, headers, and GraphQL documents do not affect host selection.
            request: Request {
                url: request.url.clone(),
                path_parameters: request.path_parameters.clone(),
                ..Default::default()
            },
            environments: loaded.workspace().environments().to_vec(),
            certificates: loaded.workspace().client_certificates().to_vec(),
            selected: self.shell.selected_environment().map(str::to_owned),
        })
    }

    fn certificate_preview_matches(&self, inputs: &Inputs) -> bool {
        let (Some(loaded), Some(key), Some(request)) = (
            self.loaded_workspace.as_ref(),
            self.shell.active_tab(),
            self.active_request(),
        ) else {
            return false;
        };
        inputs.matches(
            self.workspace_path.as_deref(),
            key,
            request,
            loaded.workspace().environments(),
            loaded.workspace().client_certificates(),
            self.shell.selected_environment(),
        )
    }

    pub(super) fn refresh_active_certificate(&mut self, cx: &mut Context<Self>) {
        if self
            .active_certificate
            .inputs
            .as_deref()
            .is_none_or(|inputs| !self.certificate_preview_matches(inputs))
        {
            let inputs = self.certificate_preview_inputs().map(Arc::new);
            self.active_certificate.set_inputs(inputs);
        }
        if self.active_certificate.pending {
            return;
        }
        let Some(inputs) = self.active_certificate.inputs.clone() else {
            return;
        };
        self.active_certificate.pending = true;
        let generation = self.active_certificate.generation.clone();
        cx.spawn(async move |view, cx| {
            let worker_inputs = inputs.clone();
            let (result, inspection) = cx
                .background_spawn(async move {
                    let inputs = worker_inputs;
                    let result = probe_application::preview_client_certificate(
                        &inputs.request,
                        &probe_application::RequestResolution {
                            environments: &inputs.environments,
                            environment: inputs.selected.as_deref(),
                            client_certificates: &inputs.certificates,
                            ..Default::default()
                        },
                    );
                    let inspection =
                        if let CertificatePreview::Selected { certificate, .. } = &result {
                            Some(probe_application::inspect_certificate(
                                certificate,
                                &inputs.environments,
                                inputs.selected.as_deref(),
                                &probe_application::NoSecrets,
                                &probe_http::ExecutionOptions {
                                    base_directory: inputs
                                        .workspace
                                        .as_deref()
                                        .and_then(workspace_base_directory),
                                    ..Default::default()
                                },
                                true,
                            ))
                        } else {
                            None
                        };
                    (result, inspection)
                })
                .await;
            let refresh = matches!(result, CertificatePreview::Selected { .. });
            let accepted = view
                .update(cx, |view, cx| {
                    let current_matches = view.certificate_preview_matches(&inputs);
                    if !view.active_certificate.accept(
                        &generation,
                        current_matches,
                        result,
                        inspection,
                    ) {
                        return false;
                    }
                    cx.notify();
                    true
                })
                .unwrap_or(false);
            if accepted && refresh {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(3))
                    .await;
                let _ = view.update(cx, |view, cx| {
                    if Rc::ptr_eq(&generation, &view.active_certificate.generation) {
                        view.active_certificate.pending = false;
                        cx.notify();
                    }
                });
            }
        })
        .detach();
    }

    pub(super) fn render_active_certificate(
        &self,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let state = &self.active_certificate;
        let loading = state.result.is_none();
        if loading && !state.visible_while_refreshing {
            return div().into_any_element();
        }
        let result = state
            .result
            .as_ref()
            .unwrap_or(&CertificatePreview::Unknown);
        if matches!(result, CertificatePreview::Hidden) {
            return div().into_any_element();
        }
        let metadata = state
            .inspection
            .as_ref()
            .and_then(|inspection| inspection.metadata.as_ref());
        let warning =
            metadata.and_then(|metadata| metadata.validity_warning(std::time::SystemTime::now()));
        let mut popup = components::popup_surface(theme, "active-certificate-details", 340.0)
            .debug_selector(|| "active-certificate-details".into())
            .p(px(theme.metrics.spacing_3))
            .flex()
            .flex_col()
            .gap(px(theme.metrics.spacing_2))
            .text_size(px(theme.typography.caption_size))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(theme.metrics.spacing_2))
                    .child(components::certificate_details_icon(theme).flex_none())
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(theme.typography.body_size))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Client Certificate"),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_color(theme.colors.text.secondary)
                            .child("Automatic"),
                    ),
            );
        match result {
            CertificatePreview::Selected {
                certificate,
                environment,
                match_rule,
            } => {
                let source = environment.as_ref().map_or_else(
                    || "Collection".to_owned(),
                    |name| format!("Environment: {name}"),
                );
                let path = match certificate {
                    probe_core::ClientCertificate::Pem {
                        certificate_file_path,
                        ..
                    } => certificate_file_path,
                    probe_core::ClientCertificate::Pkcs12 {
                        pkcs12_file_path, ..
                    } => pkcs12_file_path,
                };
                let filename = path.rsplit(['/', '\\']).next().unwrap_or(path);
                popup = popup.child(
                    div()
                        .text_color(theme.colors.text.secondary)
                        .child("Selected for next request. TLS handshake not yet performed."),
                );
                if let Some(metadata) = metadata {
                    for (label, value, warning) in [
                        ("Subject", metadata.subject.as_str(), None),
                        ("Issuer", metadata.issuer.as_str(), None),
                        ("Expiry", metadata.expires.as_str(), warning),
                    ] {
                        popup = popup.child(certificate_detail(theme, label, value, warning));
                    }
                } else {
                    popup = popup.child(
                        div()
                            .id("active-certificate-metadata-unavailable")
                            .debug_selector(|| "active-certificate-metadata-unavailable".into())
                            .text_color(theme.colors.text.muted)
                            .child("Metadata unavailable"),
                    );
                }
                for (label, value) in [
                    ("File", filename),
                    ("Source", source.as_str()),
                    ("Match", match_rule.as_str()),
                ] {
                    popup = popup.child(certificate_detail(theme, label, value, None));
                }
                if environment.is_some() {
                    let manage = cx.weak_entity();
                    popup = popup.child(
                        components::text_button(
                            theme,
                            "active-certificate-manage",
                            "Manage Certificates",
                            move |_, window, cx| {
                                let _ = manage.update(cx, |view, cx| {
                                    view.active_certificate.open = false;
                                    view.open_environment_manager_dialog(window, cx);
                                    if let Some(dialog) = view.environment_manager_dialog.as_mut() {
                                        dialog.tab =
                                            certificates::EnvironmentTab::ClientCertificates;
                                    }
                                    cx.notify();
                                });
                            },
                        )
                        .p(px(0.0))
                        .h_auto()
                        .font_weight(FontWeight::MEDIUM)
                        .gap(px(theme.metrics.spacing_1))
                        .child(components::open_dialog_icon(theme)),
                    );
                }
            }
            CertificatePreview::Unmatched => {
                popup = popup.child("No enabled certificate matches the request host and port.");
            }
            _ if loading => {
                popup = popup.child("Updating certificate selection and public metadata…");
            }
            _ => {
                popup = popup.child("Selection unavailable. Check the request URL and certificate domain variables.");
            }
        };
        let color = if warning.is_some() {
            theme.colors.status.warning
        } else if loading || matches!(result, CertificatePreview::Selected { .. }) {
            theme.colors.text.secondary
        } else {
            theme.colors.text.muted
        };
        let change = cx.weak_entity();
        Popover::new("active-certificate-popover")
            .open(state.open)
            .on_open_change(move |open, _, cx| {
                let _ = change.update(cx, |view, cx| {
                    view.active_certificate.open = *open;
                    cx.notify();
                });
            })
            .trigger(
                Button::new("active-certificate-indicator")
                    .debug_selector(|| "active-certificate-indicator".into())
                    .accessibility_label("Client certificate details")
                    .ml(px(theme.metrics.spacing_1))
                    .flex_none()
                    .w(px(theme.metrics.control_height))
                    .h(px(theme.metrics.control_height))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(theme.metrics.radius_small))
                    .border_1()
                    .border_color(theme.colors.borders.standard)
                    .bg(theme.colors.surfaces.raised)
                    .hover(move |button| button.bg(theme.colors.selection.inactive_background))
                    .focus_visible(move |button| {
                        button.border_1().border_color(theme.colors.borders.focused)
                    })
                    .hint(theme, "Client certificate details")
                    .child(components::certificate_icon(theme).text_color(color))
                    .when(
                        !loading && matches!(result, CertificatePreview::Unknown),
                        |button| button.child("?"),
                    ),
            )
            .content(move |_, _, _| popup)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_preview_rejects_tab_workspace_url_and_environment_changes() {
        let loaded = probe_opencollection::load_workspace_from_str(include_str!(
            "../../../../tests/fixtures/opencollection/mtls-execution.yml"
        ))
        .unwrap();
        let key = loaded.requests()[0].key();
        let inputs = Inputs {
            workspace: Some("collection.yml".into()),
            key,
            request: loaded.workspace().request(key).unwrap().clone(),
            certificates: loaded.workspace().client_certificates().to_vec(),
            environments: vec![],
            selected: None,
        };
        let mut state = ActiveCertificate {
            inputs: Some(Arc::new(inputs.clone())),
            pending: true,
            ..Default::default()
        };
        let generation = state.generation.clone();
        let mut changes = vec![inputs.clone(); 4];
        changes[0].key = loaded.requests()[1].key();
        changes[1].workspace = Some("other.yml".into());
        changes[2].request.url = Some("https://other.test".into());
        changes[3].selected = Some("other".into());
        for current in &changes {
            assert!(!state.accept(
                &generation,
                inputs.matches(
                    current.workspace.as_deref(),
                    current.key,
                    &current.request,
                    &current.environments,
                    &current.certificates,
                    current.selected.as_deref()
                ),
                CertificatePreview::Unmatched,
                None
            ));
            assert!(state.result.is_none());
        }
        // Even returning to identical inputs cannot resurrect a previous generation.
        let mut replacement = ActiveCertificate::default();
        assert!(!replacement.accept(&generation, true, CertificatePreview::Unmatched, None));
        assert!(!state.accept(&generation, false, CertificatePreview::Unmatched, None));
        assert!(state.accept(&generation, true, CertificatePreview::Unmatched, None));
        state.result = Some(CertificatePreview::Selected {
            certificate: inputs.certificates[0].clone(),
            environment: None,
            match_rule: "localhost:8443".into(),
        });
        let mut saved = inputs.clone();
        if let probe_core::ClientCertificate::Pem { passphrase, .. } = &mut saved.certificates[0] {
            *passphrase = Some("updated".into());
        }
        state.set_inputs(Some(Arc::new(saved.clone())));
        assert!(
            state.visible_while_refreshing,
            "saving certificates keeps the button mounted"
        );
        assert!(
            state.result.is_none(),
            "old selection is not displayed while refreshing"
        );
        assert!(state.inspection.is_none());
        assert!(!state.accept(&generation, false, CertificatePreview::Unmatched, None));
        state.set_inputs(Some(Arc::new(changes[0].clone())));
        assert!(
            !state.visible_while_refreshing,
            "tab changes never retain another tab's preview"
        );
    }

    #[gpui_kit::test]
    fn preview_reuses_snapshots_and_observes_unsaved_variables(cx: &mut gpui_kit::TestAppContext) {
        cx.update(Theme::init);
        let window = cx.open_window(gpui_kit::size(px(1180.0), px(780.0)), |window, cx| {
            ProbeApp::new(window, cx)
        });
        window
            .update(cx, |view, _, cx| {
                view.session_store = None;
                let loaded = probe_opencollection::load_workspace_from_str(include_str!(
                    "../../../../tests/fixtures/opencollection/mtls-variables.yml"
                ))
                .unwrap();
                let key = loaded.requests()[0].key();
                view.set_workspace(PathBuf::from("certificate-preview.yml"), loaded);
                view.select_request(key, cx);
                view.select_environment(Some("child".into()), cx);
                view.refresh_active_certificate(cx);
                let snapshot = view.active_certificate.inputs.clone().unwrap();
                let generation = view.active_certificate.generation.clone();
                view.refresh_active_certificate(cx);
                assert!(Arc::ptr_eq(
                    view.active_certificate.inputs.as_ref().unwrap(),
                    &snapshot
                ));
                view.edit_request(
                    key,
                    |request| request.metadata.name = Some("Renamed".into()),
                    cx,
                );
                view.refresh_active_certificate(cx);
                assert!(
                    Arc::ptr_eq(view.active_certificate.inputs.as_ref().unwrap(), &snapshot),
                    "unrelated editor changes retain the snapshot"
                );
                view.active_certificate.pending = false;
                view.refresh_active_certificate(cx);
                assert!(
                    Arc::ptr_eq(view.active_certificate.inputs.as_ref().unwrap(), &snapshot),
                    "periodic inspection shares the existing snapshot"
                );

                view.loaded_workspace
                    .as_mut()
                    .unwrap()
                    .set_environment_variable("parent", "mtlsDomain", "elsewhere.test".into())
                    .unwrap();
                let current_matches = view.certificate_preview_matches(&snapshot);
                assert!(
                    !current_matches,
                    "unsaved inherited variables invalidate pending work"
                );
                assert!(!view.active_certificate.accept(
                    &generation,
                    current_matches,
                    CertificatePreview::Unmatched,
                    None
                ));
                view.refresh_active_certificate(cx);
                assert!(!Arc::ptr_eq(
                    view.active_certificate.inputs.as_ref().unwrap(),
                    &snapshot
                ));

                let loaded = probe_opencollection::load_workspace_from_str(include_str!(
                    "../../../../tests/fixtures/opencollection/phase4-environments.yml"
                ))
                .unwrap();
                let key = loaded.requests()[0].key();
                view.set_workspace(PathBuf::from("without-certificates.yml"), loaded);
                view.select_request(key, cx);
                view.select_environment(Some("base".into()), cx);
                view.refresh_active_certificate(cx);
                assert!(view.active_certificate.inputs.is_none());
                assert!(
                    !view.active_certificate.pending,
                    "unconfigured scopes schedule no preview"
                );
            })
            .unwrap();
    }

    #[test]
    fn public_inspection_retains_selection_with_unavailable_metadata_and_expired_dates() {
        let loaded = probe_opencollection::load_workspace_from_str(include_str!(
            "../../../../tests/fixtures/opencollection/mtls-execution.yml"
        ))
        .unwrap();
        let certificate = &loaded.workspace().client_certificates()[0];
        let inspect = |certificate| {
            probe_application::inspect_certificate(
                certificate,
                &[],
                None,
                &probe_application::NoSecrets,
                &probe_http::ExecutionOptions {
                    base_directory: Some(
                        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                            .join("../../tests/fixtures/mtls"),
                    ),
                    ..Default::default()
                },
                true,
            )
        };
        let missing = inspect(certificate);
        assert!(missing.metadata.is_none());
        assert!(missing.validation.is_err());
        let mut certificate = certificate.clone();
        if let probe_core::ClientCertificate::Pem {
            certificate_file_path,
            private_key_file_path,
            passphrase,
            ..
        } = &mut certificate
        {
            *certificate_file_path = "client.pem".into();
            *private_key_file_path = "must-not-be-read.pem".into();
            *passphrase = Some("{{unavailable}}".into());
        }
        let public = inspect(&certificate);
        let metadata = public.metadata.unwrap();
        assert!(!metadata.subject.is_empty());
        assert_eq!(
            metadata.validity_warning(metadata.not_after + std::time::Duration::from_secs(1)),
            Some("Certificate has expired.")
        );
    }
}
