//! Transient next-request certificate selection and preview. Never uses the native secret provider.
use super::*;
use crate::components::IconButtonHint;
use probe_application::{CertificateOverride, CertificatePreview, CertificateRef};

#[derive(Clone, PartialEq)]
struct Inputs {
    workspace: Option<PathBuf>,
    key: RequestKey,
    request: Request,
    environments: Vec<probe_core::Environment>,
    certificates: Vec<probe_core::ClientCertificate>,
    selected: Option<String>,
    selection: CertificateOverride,
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

#[derive(Clone)]
struct Candidate {
    reference: CertificateRef,
    inspection: probe_http::CertificateInspection,
    source: Option<PublicFileRevision>,
}

/// Public files only. Stat on the worker instead of reopening/parsing unchanged
/// certificates. Unix change time/inode also detect replacement or restored mtime.
#[derive(Clone, PartialEq)]
struct PublicFileRevision {
    path: PathBuf,
    modified: std::time::SystemTime,
    created: Option<std::time::SystemTime>,
    length: u64,
    #[cfg(unix)]
    identity: (u64, u64, i64, i64),
}

impl PublicFileRevision {
    fn read(path: PathBuf) -> Option<Self> {
        let metadata = std::fs::metadata(&path).ok()?;
        if !metadata.is_file() {
            return None;
        }
        Some(Self {
            path,
            modified: metadata.modified().ok()?,
            created: metadata.created().ok(),
            length: metadata.len(),
            #[cfg(unix)]
            identity: {
                use std::os::unix::fs::MetadataExt;
                (
                    metadata.dev(),
                    metadata.ino(),
                    metadata.ctime(),
                    metadata.ctime_nsec(),
                )
            },
        })
    }
}

fn certificate_file(certificate: &probe_core::ClientCertificate) -> (&str, &'static str) {
    match certificate {
        probe_core::ClientCertificate::Pem {
            certificate_file_path,
            ..
        } => (certificate_file_path, "PEM"),
        probe_core::ClientCertificate::Pkcs12 {
            pkcs12_file_path, ..
        } => (pkcs12_file_path, "PKCS#12"),
    }
}

fn public_source_revision(
    inputs: &Inputs,
    certificate: &probe_core::ClientCertificate,
) -> Option<PublicFileRevision> {
    let (path, _) = certificate_file(certificate);
    let environment = probe_core::resolve_environment_for_templates_with_provider(
        [path],
        &inputs.environments,
        inputs.selected.as_deref(),
        &[],
        &probe_application::NoSecrets,
        None,
    )
    .ok()?;
    let path = PathBuf::from(environment.interpolate_strict(path).ok()?);
    let path = if path.is_absolute() {
        path
    } else {
        inputs
            .workspace
            .as_deref()
            .and_then(workspace_base_directory)?
            .join(path)
    };
    PublicFileRevision::read(path)
}

fn inspect_candidates(
    inputs: &Inputs,
    references: Vec<CertificateRef>,
    previous: &[Candidate],
    mut inspect: impl FnMut(&probe_core::ClientCertificate) -> probe_http::CertificateInspection,
) -> Vec<Candidate> {
    references
        .into_iter()
        .enumerate()
        .map(|(index, reference)| {
            let source = public_source_revision(inputs, &reference.certificate);
            if let Some(cached) = previous.get(index)
                && cached.reference == reference
                && source.is_some()
                && cached.source == source
            {
                return cached.clone();
            }
            let mut inspection = inspect(&reference.certificate);
            // A concurrent write must not label partially read/old bytes as current.
            // Unstatable sources stay uncached and are retried on the next refresh.
            let stable = source
                .as_ref()
                .filter(|before| {
                    PublicFileRevision::read(before.path.clone()).as_ref() == Some(before)
                })
                .cloned();
            if source.is_some() && stable.is_none() {
                inspection = probe_http::CertificateInspection {
                    metadata: None,
                    validation: Err(probe_http::HttpError::ClientConfiguration(
                        "Certificate changed during inspection; metadata will be refreshed.".into(),
                    )),
                };
            }
            Candidate {
                reference,
                inspection,
                source: stable,
            }
        })
        .collect()
}

#[derive(Default)]
pub(super) struct ActiveCertificate {
    inputs: Option<Arc<Inputs>>,
    generation: Rc<()>,
    result: Option<CertificatePreview>,
    inspection: Option<probe_http::CertificateInspection>,
    candidates: Vec<Candidate>,
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
            && (inputs
                .as_ref()
                .is_some_and(|inputs| inputs.selection != CertificateOverride::Automatic)
                || self.visible_while_refreshing
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

fn candidate_filename(candidate: &Candidate) -> &str {
    let (path, _) = certificate_file(&candidate.reference.certificate);
    candidate
        .source
        .as_ref()
        .and_then(|source| source.path.file_name())
        .and_then(|name| name.to_str())
        .unwrap_or_else(|| path.rsplit(['/', '\\']).next().unwrap_or(path))
}

fn certificate_options(
    candidates: &[Candidate],
    now: std::time::SystemTime,
) -> Vec<(CertificateOverride, String)> {
    let labels: Vec<_> = candidates
        .iter()
        .map(|candidate| {
            let (_, kind) = certificate_file(&candidate.reference.certificate);
            let filename = candidate_filename(candidate);
            let subject = candidate.inspection.metadata.as_ref().map(|metadata| {
                let mut text: String = metadata.subject.chars().take(24).collect();
                if metadata.subject.chars().count() > 24 {
                    text.push('…');
                }
                text
            });
            let name = subject.filter(|subject| subject != filename).map_or_else(
                || filename.to_owned(),
                |subject| format!("{filename} · {subject}"),
            );
            let source = candidate
                .reference
                .environment
                .as_deref()
                .unwrap_or("Collection");
            format!("{name} · {kind} · {source}")
        })
        .collect();
    let mut counts = std::collections::HashMap::new();
    for label in &labels {
        *counts.entry(label.as_str()).or_insert(0) += 1;
    }
    let mut seen = std::collections::HashSet::new();
    candidates
        .iter()
        .zip(&labels)
        .enumerate()
        .map(|(index, (candidate, label))| {
            let mut label = label.clone();
            let filename = candidate_filename(candidate);
            if counts[label.as_str()] > 1 {
                let suffix = candidate
                    .inspection
                    .metadata
                    .as_ref()
                    .map(|metadata| {
                        let hex: String = metadata
                            .fingerprint
                            .chars()
                            .filter(|ch| ch.is_ascii_hexdigit())
                            .collect();
                        hex[hex.len().saturating_sub(8)..].to_owned()
                    })
                    .unwrap_or_else(|| {
                        certificate_file(&candidate.reference.certificate)
                            .0
                            .to_owned()
                    });
                label.insert_str(filename.len(), &format!(" [{suffix}]"));
            }
            if !seen.insert(label.clone()) {
                // Presentation-only ordinal for identical public identities. Selection
                // still retains the complete declaration and scope, never this index.
                label.insert_str(filename.len(), &format!(" #{}", index + 1));
            }
            if let Some(warning) = candidate
                .inspection
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.validity_warning(now))
            {
                label.push_str(&format!(" · {warning}"));
            }
            (
                CertificateOverride::Selected(candidate.reference.clone()),
                label,
            )
        })
        .collect()
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
                            .flex()
                            .items_start()
                            .gap(px(theme.metrics.spacing_1))
                            .text_size(px(theme.typography.caption_size))
                            .text_color(theme.colors.status.warning)
                            .font_weight(FontWeight::MEDIUM)
                            .line_height(relative(1.25))
                            .child(
                                components::warning_icon(theme)
                                    .h(px(theme.typography.caption_size * 1.25)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .whitespace_normal()
                                    .child(warning),
                            ),
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
        if self.shell.certificate_override(self.shell.active_tab()?)
            == &CertificateOverride::Automatic
            && workspace.client_certificates().is_empty()
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
            selection: self
                .shell
                .certificate_override(self.shell.active_tab()?)
                .clone(),
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
        inputs.selection == *self.shell.certificate_override(key)
            && inputs.matches(
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
        let previous = self.active_certificate.candidates.clone();
        cx.spawn(async move |view, cx| {
            let worker_inputs = inputs.clone();
            let (result, inspection, candidates) = cx
                .background_spawn(async move {
                    let inputs = worker_inputs;
                    let resolution = probe_application::RequestResolution {
                        environments: &inputs.environments,
                        environment: inputs.selected.as_deref(),
                        client_certificates: &inputs.certificates,
                        certificate_override: Some(&inputs.selection),
                        ..Default::default()
                    };
                    let result =
                        probe_application::preview_client_certificate(&inputs.request, &resolution);
                    let inspect = |certificate: &probe_core::ClientCertificate| {
                        probe_application::inspect_certificate(
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
                        )
                    };
                    let candidates = inspect_candidates(
                        &inputs,
                        probe_application::certificate_candidates(&inputs.request, &resolution),
                        &previous,
                        inspect,
                    );
                    let inspection = if let CertificatePreview::Selected {
                        certificate,
                        environment,
                        ..
                    } = &result
                    {
                        candidates
                            .iter()
                            .find(|candidate| {
                                &candidate.reference.certificate == certificate
                                    && &candidate.reference.environment == environment
                            })
                            .map(|candidate| candidate.inspection.clone())
                    } else {
                        None
                    };
                    (result, inspection, candidates)
                })
                .await;
            let refresh = !matches!(result, CertificatePreview::Hidden);
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
                    view.active_certificate.candidates = candidates;
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
        let selection = self
            .shell
            .active_tab()
            .map(|key| self.shell.certificate_override(key))
            .unwrap_or(&CertificateOverride::Automatic);
        let overridden = selection != &CertificateOverride::Automatic;
        let loading = state.result.is_none();
        if loading && !state.visible_while_refreshing && !overridden {
            return div().into_any_element();
        }
        let result = state.result.as_ref().unwrap_or(
            if matches!(selection, CertificateOverride::NoCertificate) {
                &CertificatePreview::NoCertificate
            } else {
                &CertificatePreview::Unknown
            },
        );
        if matches!(result, CertificatePreview::Hidden) && !overridden {
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
                            .child(match selection {
                                CertificateOverride::Automatic => "Automatic",
                                CertificateOverride::NoCertificate => "No Certificate",
                                CertificateOverride::Selected(_) => "Manual",
                            }),
                    ),
            );
        let mut options = vec![
            (
                CertificateOverride::Automatic,
                "Automatic (default)".to_owned(),
            ),
            (
                CertificateOverride::NoCertificate,
                "No Certificate".to_owned(),
            ),
        ];
        options.extend(certificate_options(
            &state.candidates,
            std::time::SystemTime::now(),
        ));
        // Keep an invalid choice visible without making it another selectable identity.
        let value = options
            .iter()
            .any(|(value, _)| value == selection)
            .then(|| selection.clone());
        if let Some(key) = self.shell.active_tab() {
            let change = cx.weak_entity();
            popup = popup
                .child(
                    div()
                        .text_color(theme.colors.text.secondary)
                        .child("Select Certificate"),
                )
                .child(
                    components::dropdown(
                        theme,
                        "active-certificate-selector",
                        "Select Certificate",
                        value,
                        options,
                        314.0,
                        move |selection, _, cx| {
                            let Some(selection) = selection.cloned() else {
                                return;
                            };
                            let _ = change.update(cx, |view, cx| {
                                view.shell.set_certificate_override(key, selection);
                                view.refresh_active_certificate(cx);
                                view.active_certificate.open = true;
                                cx.notify();
                            });
                        },
                    )
                    .option_hints()
                    .placeholder(if loading {
                        "Updating selection…"
                    } else if matches!(result, CertificatePreview::Unknown) {
                        "Selection unknown"
                    } else {
                        "Invalid selection"
                    }),
                );
            if overridden {
                popup = popup.child(components::text_button(
                    theme,
                    "reset-active-certificate",
                    "Reset to Automatic",
                    cx.listener(move |view, _, _, cx| {
                        view.shell
                            .set_certificate_override(key, CertificateOverride::Automatic);
                        view.refresh_active_certificate(cx);
                        view.active_certificate.open = true;
                        cx.notify();
                    }),
                ));
            }
        }
        match result {
            CertificatePreview::NoCertificate => {
                popup = popup.child("No client certificate will be sent for this request.");
            }
            CertificatePreview::Invalid => {
                popup = popup.child(div().text_color(theme.colors.status.warning).child("Selected certificate is unavailable or no longer matches this HTTPS target. Reset to Automatic or select another certificate."));
            }
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
                    for (label, value) in [
                        ("Subject", metadata.subject.as_str()),
                        ("Issuer", metadata.issuer.as_str()),
                    ] {
                        popup = popup.child(certificate_detail(theme, label, value, None));
                    }
                    let expires: chrono::DateTime<chrono::Utc> = metadata.not_after.into();
                    popup = popup.child(
                        certificate_detail(
                            theme,
                            "Expiry",
                            &expires.format("%-d %b %Y").to_string(),
                            warning,
                        )
                        .id("active-certificate-expiry")
                        .hint(theme, format!("Expires {}", metadata.expires)),
                    );
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
            CertificatePreview::Unknown => {
                popup = popup.child(div()
                    .id("active-certificate-selection-unknown")
                    .debug_selector(|| "active-certificate-selection-unknown".into())
                    .child("Certificate selection cannot be confirmed without resolving URL or domain variables. It will be validated when this request is sent."));
            }
            _ => {
                popup = popup.child("Selection unavailable. Check the request URL and certificate domain variables.");
            }
        };
        let color = if warning.is_some() || matches!(result, CertificatePreview::Invalid) {
            theme.colors.status.warning
        } else if matches!(selection, CertificateOverride::NoCertificate) {
            theme.colors.text.muted
        } else if loading || matches!(result, CertificatePreview::Selected { .. }) {
            theme.colors.text.secondary
        } else {
            theme.colors.text.muted
        };
        let indicator_label = if matches!(result, CertificatePreview::Invalid) {
            "Invalid client certificate override"
        } else if matches!(result, CertificatePreview::Unknown) && !loading {
            "Client certificate selection unknown; validated when sent"
        } else {
            match selection {
                CertificateOverride::Automatic => "Client certificate details",
                CertificateOverride::NoCertificate => "Client certificate: No Certificate override",
                CertificateOverride::Selected(_) => "Client certificate: manual override",
            }
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
                    .accessibility_label(indicator_label)
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
                    .hint(theme, indicator_label)
                    .child(components::certificate_icon(theme).text_color(color))
                    .when(overridden, |button| {
                        button.child(match selection {
                            CertificateOverride::NoCertificate => "−",
                            _ if matches!(result, CertificatePreview::Invalid) => "!",
                            _ if matches!(result, CertificatePreview::Unknown) => "?",
                            _ => "•",
                        })
                    })
                    .when(
                        !loading && !overridden && matches!(result, CertificatePreview::Unknown),
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
            selection: CertificateOverride::Automatic,
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
        let mut overridden = saved;
        overridden.selection = CertificateOverride::NoCertificate;
        let previous_generation = state.generation.clone();
        state.set_inputs(Some(Arc::new(overridden)));
        assert!(state.inspection.is_none());
        assert!(state.candidates.is_empty());
        assert!(!state.accept(
            &previous_generation,
            true,
            CertificatePreview::Unmatched,
            None
        ));
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
    fn public_pem(path: &str) -> probe_core::ClientCertificate {
        probe_core::ClientCertificate::Pem {
            domain: "localhost:8443".into(),
            certificate_file_path: path.into(),
            private_key_file_path: "must-not-be-read.pem".into(),
            passphrase: Some("{{unavailable}}".into()),
            disabled: false,
        }
    }

    fn inspect_public(
        certificate: &probe_core::ClientCertificate,
        inputs: &Inputs,
    ) -> probe_http::CertificateInspection {
        probe_application::inspect_certificate(
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
        )
    }

    #[test]
    fn periodic_metadata_reuses_unchanged_public_files_and_refreshes_external_and_config_changes() {
        use std::cell::Cell;
        let directory = tempfile::tempdir().unwrap();
        let fixtures =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/mtls");
        let first = directory.path().join("first.pem");
        let second = directory.path().join("second.pem");
        std::fs::copy(fixtures.join("client.pem"), &first).unwrap();
        std::fs::copy(&first, &second).unwrap();
        let mut loaded = probe_opencollection::load_workspace_from_str(include_str!(
            "../../../../tests/fixtures/opencollection/mtls-variables.yml"
        ))
        .unwrap();
        loaded
            .set_environment_variable("parent", "certPath", "first.pem".into())
            .unwrap();
        let mut inputs = Inputs {
            workspace: Some(directory.path().join("collection.yml")),
            key: loaded.requests()[0].key(),
            request: Request {
                url: Some("https://localhost:8443".into()),
                ..Default::default()
            },
            environments: loaded.workspace().environments().to_vec(),
            certificates: vec![public_pem("{{certPath}}"), public_pem("second.pem")],
            selected: Some("child".into()),
            selection: CertificateOverride::Automatic,
        };
        let references = inputs
            .certificates
            .iter()
            .map(|certificate| CertificateRef {
                certificate: certificate.clone(),
                environment: None,
            })
            .collect::<Vec<_>>();
        let reads = Cell::new(0);
        let refresh = |inputs: &Inputs, previous: &[Candidate]| {
            inspect_candidates(inputs, references.clone(), previous, |certificate| {
                reads.set(reads.get() + 1);
                inspect_public(certificate, inputs)
            })
        };
        let initial = refresh(&inputs, &[]);
        assert_eq!(reads.get(), 2);
        assert!(
            certificate_options(&initial, std::time::SystemTime::now())[0]
                .1
                .starts_with("first.pem ·"),
            "labels use the safely resolved filename, not its variable template"
        );
        let original = initial[0]
            .inspection
            .metadata
            .as_ref()
            .unwrap()
            .fingerprint
            .clone();
        let unchanged = refresh(&inputs, &initial);
        assert_eq!(
            reads.get(),
            2,
            "unchanged files must not be reopened or parsed"
        );
        std::fs::copy(fixtures.join("wrong-client.pem"), &first).unwrap();
        let changed = refresh(&inputs, &unchanged);
        assert_eq!(reads.get(), 3, "only the changed public file is inspected");
        assert_ne!(
            changed[0].inspection.metadata.as_ref().unwrap().fingerprint,
            original
        );
        assert_eq!(
            changed[1].inspection.metadata.as_ref().unwrap().fingerprint,
            original
        );
        // Replacement with identical bytes and restored mtime is still a new file.
        let replacement = directory.path().join("replacement.pem");
        std::fs::copy(&first, &replacement).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&replacement)
            .unwrap()
            .set_times(
                std::fs::FileTimes::new()
                    .set_modified(changed[0].source.as_ref().unwrap().modified),
            )
            .unwrap();
        std::fs::rename(replacement, &first).unwrap();
        let replaced = refresh(&inputs, &changed);
        assert_eq!(reads.get(), 4);
        // The declaration is unchanged, but its public path variable now points elsewhere.
        loaded
            .set_environment_variable("parent", "certPath", "second.pem".into())
            .unwrap();
        inputs.environments = loaded.workspace().environments().to_vec();
        let configured = refresh(&inputs, &replaced);
        assert_eq!(reads.get(), 5);
        assert_eq!(
            configured[0]
                .inspection
                .metadata
                .as_ref()
                .unwrap()
                .fingerprint,
            original
        );
        std::fs::remove_file(&second).unwrap();
        let removed = refresh(&inputs, &configured);
        assert_eq!(reads.get(), 7);
        assert!(
            removed
                .iter()
                .all(|candidate| candidate.inspection.metadata.is_none())
        );
        std::fs::copy(fixtures.join("client.pem"), &second).unwrap();
        let restored = refresh(&inputs, &removed);
        assert_eq!(reads.get(), 9);
        assert!(
            restored
                .iter()
                .all(|candidate| candidate.inspection.metadata.is_some())
        );
        let raced = inspect_candidates(&inputs, references[..1].to_vec(), &[], |certificate| {
            let inspection = inspect_public(certificate, &inputs);
            std::fs::copy(fixtures.join("wrong-client.pem"), &second).unwrap();
            inspection
        });
        assert!(raced[0].source.is_none());
        assert!(
            raced[0].inspection.metadata.is_none(),
            "a concurrent replacement cannot retain stale metadata"
        );
        let retried = refresh(&inputs, &raced);
        assert!(retried[0].inspection.metadata.is_some());
        assert_ne!(
            retried[0].inspection.metadata.as_ref().unwrap().fingerprint,
            original
        );
    }

    #[test]
    fn option_labels_include_files_and_disambiguate_duplicate_subjects_without_losing_warnings() {
        let loaded = probe_opencollection::load_workspace_from_str(include_str!(
            "../../../../tests/fixtures/opencollection/mtls-execution.yml"
        ))
        .unwrap();
        let inputs = Inputs {
            workspace: Some(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../tests/fixtures/mtls/collection.yml"),
            ),
            key: loaded.requests()[0].key(),
            request: Request::default(),
            environments: vec![],
            certificates: vec![],
            selected: None,
            selection: CertificateOverride::Automatic,
        };
        let mut first = Candidate {
            reference: CertificateRef {
                certificate: public_pem("one/client.pem"),
                environment: Some("Development".into()),
            },
            inspection: inspect_public(&public_pem("client.pem"), &inputs),
            source: None,
        };
        let mut second = Candidate {
            reference: CertificateRef {
                certificate: public_pem("two/client.pem"),
                environment: Some("Development".into()),
            },
            inspection: inspect_public(&public_pem("wrong-client.pem"), &inputs),
            source: None,
        };
        for candidate in [&mut first, &mut second] {
            let metadata = candidate.inspection.metadata.as_mut().unwrap();
            metadata.subject = "Identical Subject".into();
            metadata.not_before = std::time::SystemTime::UNIX_EPOCH;
            metadata.not_after = std::time::SystemTime::UNIX_EPOCH;
        }
        second.inspection.metadata.as_mut().unwrap().not_after =
            std::time::SystemTime::now() + std::time::Duration::from_secs(365 * 86400);
        let labels = certificate_options(
            &[first.clone(), second.clone()],
            std::time::SystemTime::now(),
        );
        assert_ne!(labels[0].1, labels[1].1);
        for (selection, label) in &labels {
            assert!(matches!(selection, CertificateOverride::Selected(_)));
            for value in ["client.pem", "Identical Subject", "PEM", "Development"] {
                assert!(label.contains(value), "missing {value}: {label}");
            }
            assert!(!label.contains("must-not-be-read"));
        }
        assert!(labels[0].1.contains("expired"));
        assert!(!labels[1].1.contains("expired"));
        assert!(
            labels
                .iter()
                .all(|(_, label)| label.starts_with("client.pem [")),
            "disambiguation must stay near the filename, independent of expiry warnings"
        );
        // Different filenames already identify certificates sharing one Subject.
        second.reference.certificate = public_pem("other.pem");
        let filenames = certificate_options(
            &[first.clone(), second.clone()],
            std::time::SystemTime::now(),
        );
        assert!(filenames[1].1.starts_with("other.pem"));
        assert!(
            !filenames[0].1.contains('['),
            "fingerprints are only needed for ambiguous labels"
        );
        let duplicates = certificate_options(&[first.clone(), first], std::time::SystemTime::now());
        assert_ne!(duplicates[0].1, duplicates[1].1);
        assert_eq!(
            duplicates[0].0, duplicates[1].0,
            "display ordinals never replace declaration identity"
        );
        second.inspection.metadata = None;
        let unavailable = certificate_options(&[second], std::time::SystemTime::now());
        assert!(unavailable[0].1.contains("other.pem"));
        assert!(!unavailable[0].1.contains("Identical Subject"));
    }
}
