//! Environment-only certificate editing. Runtime resolution stays in shared execution.
use super::*;
use gpui_kit::base::input::{InputEvent, InputState};
use probe_core::ClientCertificate;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum EnvironmentTab {
    #[default]
    Variables,
    Secrets,
    ClientCertificates,
}

/// Input entities live only as long as the form, including its masked passphrase.
#[derive(Clone)]
pub(crate) struct CertificateEditor {
    pub(crate) inspection: Option<probe_http::CertificateInspection>,
    pub(crate) inspection_watcher: Option<Rc<notify::RecommendedWatcher>>,
    /// The local result is still shown. File changes will not refresh it.
    pub(crate) monitoring_unavailable: bool,
    pub(crate) inspecting: bool,
    pub(crate) inspection_generation: Rc<()>,
    pub(crate) index: Option<usize>,
    pub(crate) focus: FocusHandle,
    pub(crate) restore_focus: FocusHandle,
    pub(crate) window: gpui_kit::AnyWindowHandle,
    pub(crate) pkcs12: bool,
    pub(crate) disabled: bool,
    pub(crate) revealed: bool,
    pub(crate) show_validation_feedback: bool,
    // An untouched explicit empty YAML value differs from an absent passphrase.
    preserve_empty_passphrase: bool,
    // Domain, PEM certificate, PEM private key, PKCS#12 archive, passphrase.
    pub(crate) inputs: Vec<gpui_kit::Entity<InputState>>,
    pub(crate) _subscriptions: Rc<Vec<gpui_kit::Subscription>>,
}

impl std::fmt::Debug for CertificateEditor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CertificateEditor")
            .field("index", &self.index)
            .finish_non_exhaustive()
    }
}

impl CertificateEditor {
    pub(crate) fn invalidate_inspection(&mut self) {
        self.inspection = None;
        self.inspection_watcher = None;
        self.monitoring_unavailable = false;
        self.inspecting = false;
        self.inspection_generation = Rc::new(());
    }

    pub(crate) fn accept_inspection(
        &mut self,
        generation: &Rc<()>,
        result: probe_http::CertificateInspection,
        watcher: Option<notify::RecommendedWatcher>,
        monitoring_unavailable: bool,
    ) -> bool {
        if !Rc::ptr_eq(&self.inspection_generation, generation) {
            return false;
        }
        self.inspecting = false;
        self.inspection = Some(result);
        self.inspection_watcher = watcher.map(Rc::new);
        self.monitoring_unavailable = monitoring_unavailable;
        true
    }

    pub(crate) fn missing_required_field(&self, cx: &App) -> Option<&'static str> {
        self.missing_field(cx, true)
    }

    /// Local inspection reads certificate material only. Domain stays mandatory for Save.
    pub(crate) fn missing_inspection_field(&self, cx: &App) -> Option<&'static str> {
        self.missing_field(cx, false)
    }

    fn missing_field(&self, cx: &App, require_domain: bool) -> Option<&'static str> {
        let required: &[(usize, &str)] = match (require_domain, self.pkcs12) {
            (true, true) => &[(0, "Domain"), (3, "PKCS#12 File")],
            (false, true) => &[(3, "PKCS#12 File")],
            (true, false) => &[(0, "Domain"), (1, "Certificate"), (2, "Private Key")],
            (false, false) => &[(1, "Certificate"), (2, "Private Key")],
        };
        required.iter().find_map(|(index, label)| {
            self.inputs[*index]
                .read(cx)
                .value()
                .trim()
                .is_empty()
                .then_some(*label)
        })
    }

    pub(crate) fn certificate(&self, cx: &App) -> Option<ClientCertificate> {
        self.certificate_for(cx, true)
    }

    fn inspection_certificate(&self, cx: &App) -> Option<ClientCertificate> {
        self.certificate_for(cx, false)
    }

    fn certificate_for(&self, cx: &App, require_domain: bool) -> Option<ClientCertificate> {
        if self.missing_field(cx, require_domain).is_some() {
            return None;
        }
        let values: Vec<_> = self
            .inputs
            .iter()
            .enumerate()
            .map(|(index, input)| {
                let value = input.read(cx).value();
                if index == 4 {
                    value.to_string()
                } else {
                    value.trim().to_owned()
                }
            })
            .collect();
        let passphrase =
            (self.preserve_empty_passphrase || !values[4].is_empty()).then(|| values[4].clone());
        Some(if self.pkcs12 {
            ClientCertificate::Pkcs12 {
                domain: values[0].clone(),
                pkcs12_file_path: values[3].clone(),
                passphrase,
                disabled: self.disabled,
            }
        } else {
            ClientCertificate::Pem {
                domain: values[0].clone(),
                certificate_file_path: values[1].clone(),
                private_key_file_path: values[2].clone(),
                passphrase,
                disabled: self.disabled,
            }
        })
    }
}

impl ProbeApp {
    pub(super) fn open_certificate_editor(
        &mut self,
        index: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.environment_save_task.is_some() {
            return;
        }
        let Some(dialog) = self.environment_manager_dialog.as_ref() else {
            return;
        };
        if dialog.certificate_editor.is_some() {
            return;
        }
        let restore_focus = if dialog.tab == EnvironmentTab::ClientCertificates {
            window
                .focused(cx)
                .unwrap_or_else(|| self.environment_manager_dialog_focus.clone())
        } else {
            self.environment_manager_dialog_focus.clone()
        };
        let mut values = vec![String::new(); 5];
        let mut pkcs12 = false;
        let mut disabled = false;
        let mut preserve_empty_passphrase = false;
        if let Some(index) = index {
            let Some(certificate) = dialog.draft().client_certificates.get(index) else {
                return;
            };
            match certificate {
                ClientCertificate::Pem {
                    domain,
                    certificate_file_path,
                    private_key_file_path,
                    passphrase,
                    disabled: off,
                } => {
                    values = vec![
                        domain.clone(),
                        certificate_file_path.clone(),
                        private_key_file_path.clone(),
                        String::new(),
                        passphrase.clone().unwrap_or_default(),
                    ];
                    preserve_empty_passphrase = passphrase.as_deref() == Some("");
                    disabled = *off;
                }
                ClientCertificate::Pkcs12 {
                    domain,
                    pkcs12_file_path,
                    passphrase,
                    disabled: off,
                } => {
                    values[0] = domain.clone();
                    values[3] = pkcs12_file_path.clone();
                    values[4] = passphrase.clone().unwrap_or_default();
                    preserve_empty_passphrase = passphrase.as_deref() == Some("");
                    disabled = *off;
                    pkcs12 = true;
                }
            }
        }
        let theme = self.theme(window);
        let mut subscriptions = Vec::new();
        let inputs = values
            .into_iter()
            .enumerate()
            .map(|(index, value)| {
                let input = cx.new(|cx| {
                    let mut input = InputState::new(window, cx).masked(index == 4);
                    input.set_editor_style(components::editor_paint_style(theme));
                    input.set_value(value, window, cx);
                    input
                });
                subscriptions.push(cx.subscribe_in(
                    &input,
                    window,
                    move |view, input, event, _, cx| {
                        if matches!(event, InputEvent::Change | InputEvent::Blur) {
                            if let Some(editor) = view
                                .environment_manager_dialog
                                .as_mut()
                                .and_then(|dialog| dialog.certificate_editor.as_mut())
                                && editor.inputs.get(index) == Some(input)
                            {
                                if matches!(event, InputEvent::Change) {
                                    editor.invalidate_inspection();
                                }
                                editor.show_validation_feedback = true;
                                if index == 4 && !input.read(cx).value().is_empty() {
                                    editor.preserve_empty_passphrase = false;
                                }
                            }
                            cx.notify();
                        }
                    },
                ));
                input
            })
            .collect();
        self.transient.environment_manager_context_menu = None;
        let dialog = self.environment_manager_dialog.as_mut().unwrap();
        dialog.active_field = None;
        dialog.tab = EnvironmentTab::ClientCertificates;
        dialog.certificate_editor = Some(CertificateEditor {
            inspection: None,
            inspection_watcher: None,
            monitoring_unavailable: false,
            inspecting: false,
            inspection_generation: Rc::new(()),
            index,
            focus: cx.focus_handle(),
            restore_focus,
            window: window.window_handle(),
            pkcs12,
            disabled,
            revealed: false,
            show_validation_feedback: false,
            preserve_empty_passphrase,
            inputs,
            _subscriptions: Rc::new(subscriptions),
        });
        dialog.certificate_editor.as_ref().unwrap().inputs[0]
            .update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    pub(super) fn cancel_certificate_editor(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(editor) = self
            .environment_manager_dialog
            .as_mut()
            .and_then(|dialog| dialog.certificate_editor.take())
        {
            for input in &editor.inputs {
                input.update(cx, |input, cx| input.set_value("", window, cx));
            }
            editor.restore_focus.focus(window, cx);
        }
        cx.notify();
    }

    pub(super) fn save_certificate_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.environment_save_task.is_some() {
            return;
        }
        let Some(dialog) = self.environment_manager_dialog.as_mut() else {
            return;
        };
        let Some(editor) = dialog.certificate_editor.as_mut() else {
            return;
        };
        let Some(certificate) = editor.certificate(cx) else {
            editor.show_validation_feedback = true;
            cx.notify();
            return;
        };
        let index = editor.index;
        if index.and_then(|index| dialog.draft().client_certificates.get(index))
            == Some(&certificate)
        {
            self.cancel_certificate_editor(window, cx);
            return;
        }
        // Domain, key and passphrase edits do not change the public certificate.
        let metadata = index
            .and_then(|index| dialog.draft().client_certificates.get(index))
            .filter(|current| same_public_certificate_source(current, &certificate))
            .and_then(|_| dialog.certificate_metadata.clone());
        let certificates = &mut dialog.draft_mut().client_certificates;
        if let Some(index) = index {
            let Some(current) = certificates.get_mut(index) else {
                return;
            };
            *current = certificate;
        } else {
            certificates.push(certificate);
        }
        dialog.certificate_metadata = metadata;
        self.cancel_certificate_editor(window, cx);
    }

    pub(super) fn browse_certificate_path(
        &mut self,
        field: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self
            .environment_manager_dialog
            .as_ref()
            .and_then(|dialog| dialog.certificate_editor.as_ref())
        else {
            return;
        };
        let Some(input) = editor.inputs.get(field).cloned() else {
            return;
        };
        let workspace_path = self.workspace_path.clone();
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Select certificate file".into()),
        });
        let view = cx.weak_entity();
        window
            .spawn(cx, async move |cx| {
                match receiver.await {
                    Ok(Ok(Some(paths))) => {
                        let Some(path) = paths.first() else {
                            return;
                        };
                        let value = body_file_path_for_storage(path, workspace_path.as_deref());
                        let _ = view.update_in(cx, |view, window, cx| {
                            // Ignore a picker result if the form was cancelled or replaced during reload.
                            let current = view
                                .environment_manager_dialog
                                .as_ref()
                                .and_then(|dialog| dialog.certificate_editor.as_ref())
                                .is_some_and(|editor| editor.inputs.get(field) == Some(&input));
                            if current {
                                if let Some(editor) = view
                                    .environment_manager_dialog
                                    .as_mut()
                                    .and_then(|dialog| dialog.certificate_editor.as_mut())
                                {
                                    editor.invalidate_inspection();
                                }
                                input.update(cx, |input, cx| input.set_value(value, window, cx));
                                cx.notify();
                            }
                        });
                    }
                    Ok(Err(error)) => {
                        let _ = view.update_in(cx, |view, _, cx| {
                            view.show_toast(
                                ToastIntent::Error,
                                format!("Could not open the file picker: {error}"),
                                cx,
                            );
                        });
                    }
                    _ => {}
                }
            })
            .detach();
    }
}

impl ProbeApp {
    /// Environments for inspection, with the unsaved draft standing in for its saved copy.
    fn certificate_inspection_context(
        &self,
        draft: &probe_core::Environment,
    ) -> (Vec<probe_core::Environment>, probe_http::ExecutionOptions) {
        let mut environments = self
            .loaded_workspace
            .as_ref()
            .map(|loaded| loaded.workspace().environments().to_vec())
            .unwrap_or_default();
        environments.retain(|environment| environment.name != draft.name);
        environments.push(draft.clone());
        let options = probe_http::ExecutionOptions {
            base_directory: self
                .workspace_path
                .as_deref()
                .and_then(workspace_base_directory),
            ..Default::default()
        };
        (environments, options)
    }

    pub(super) fn load_certificate_metadata(&mut self, cx: &mut Context<Self>) {
        let Some(dialog) = self.environment_manager_dialog.as_mut() else {
            return;
        };
        if dialog.tab != EnvironmentTab::ClientCertificates
            || dialog.certificate_metadata_refresh_pending
        {
            return;
        }
        let draft = dialog.draft().clone();
        let generation = dialog.certificate_generation.clone();
        dialog.certificate_metadata_refresh_pending = true;
        // Keep the last safe metadata visible until its replacement is ready.
        dialog.certificate_metadata.get_or_insert_with(Vec::new);
        let (environments, options) = self.certificate_inspection_context(&draft);
        cx.spawn(async move |view, cx| {
            let results = cx
                .background_spawn(async move {
                    draft
                        .client_certificates
                        .iter()
                        .map(|cert| {
                            probe_application::inspect_certificate(
                                cert,
                                &environments,
                                Some(&draft.name),
                                &probe_application::NoSecrets,
                                &options,
                                true,
                            )
                        })
                        .collect()
                })
                .await;
            let accepted = view
                .update(cx, |view, cx| {
                    if let Some(dialog) = view.environment_manager_dialog.as_mut()
                        && Rc::ptr_eq(&dialog.certificate_generation, &generation)
                    {
                        dialog.certificate_metadata = Some(results);
                        cx.notify();
                        true
                    } else {
                        false
                    }
                })
                .unwrap_or(false);
            if accepted {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(3))
                    .await;
                let _ = view.update(cx, |view, cx| {
                    if let Some(dialog) = view.environment_manager_dialog.as_mut()
                        && Rc::ptr_eq(&dialog.certificate_generation, &generation)
                    {
                        dialog.certificate_metadata_refresh_pending = false;
                        cx.notify();
                    }
                });
            }
        })
        .detach();
    }

    pub(super) fn inspect_certificate_editor(&mut self, cx: &mut Context<Self>) {
        let Some(dialog) = self.environment_manager_dialog.as_mut() else {
            return;
        };
        let draft = dialog.draft().clone();
        let Some(editor) = dialog.certificate_editor.as_mut() else {
            return;
        };
        let Some(certificate) = editor.inspection_certificate(cx) else {
            return;
        };
        editor.inspection = None;
        editor.inspection_watcher = None;
        editor.monitoring_unavailable = false;
        editor.inspecting = true;
        editor.inspection_generation = Rc::new(());
        let generation = editor.inspection_generation.clone();
        let (environments, options) = self.certificate_inspection_context(&draft);
        let workspace = self.workspace_path.clone().unwrap_or_default();
        let store = self.credential_store.clone();
        let changed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let worker_changed = changed.clone();
        cx.spawn(async move |view, cx| {
            let (result, watcher, monitoring_unavailable) = cx
                .background_spawn(async move {
                    let provider = crate::credentials::NativeSecretProvider {
                        store: store.as_ref(),
                        workspace: &workspace,
                    };
                    let mut watcher = None;
                    let mut monitoring_unavailable = false;
                    let result = probe_application::inspect_certificate_observing(
                        &certificate,
                        &environments,
                        Some(&draft.name),
                        &provider,
                        &options,
                        false,
                        |identity| {
                            let paths = identity.source_paths(&options);
                            let sources = paths.as_ref().ok().cloned().unwrap_or_default();
                            let flag = worker_changed.clone();
                            // A failed registration drops this watcher. A partial watch is not kept.
                            match start_source_monitor(
                                paths.map_err(|_| ()),
                                || {
                                    notify::recommended_watcher(
                                        move |event: notify::Result<notify::Event>| {
                                            if inspection_source_changed(&event, &sources) {
                                                flag.store(
                                                    true,
                                                    std::sync::atomic::Ordering::Relaxed,
                                                );
                                            }
                                        },
                                    )
                                    .map_err(|_| ())
                                },
                                |observer, parent| {
                                    use notify::Watcher;
                                    observer
                                        .watch(parent, notify::RecursiveMode::NonRecursive)
                                        .map_err(|_| ())
                                },
                            ) {
                                Ok(observer) => watcher = Some(observer),
                                Err(()) => monitoring_unavailable = true,
                            }
                        },
                    );
                    (result, watcher, monitoring_unavailable)
                })
                .await;
            let _ = view.update(cx, |view, cx| {
                if let Some(editor) = view
                    .environment_manager_dialog
                    .as_mut()
                    .and_then(|dialog| dialog.certificate_editor.as_mut())
                    && editor.accept_inspection(
                        &generation,
                        result,
                        watcher,
                        monitoring_unavailable,
                    )
                {
                    cx.notify();
                }
            });
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(500))
                    .await;
                let keep_watching = view
                    .update(cx, |view, cx| {
                        let Some(editor) = view
                            .environment_manager_dialog
                            .as_mut()
                            .and_then(|dialog| dialog.certificate_editor.as_mut())
                        else {
                            return false;
                        };
                        if !Rc::ptr_eq(&editor.inspection_generation, &generation) {
                            return false;
                        }
                        if changed.load(std::sync::atomic::Ordering::Relaxed) {
                            editor.invalidate_inspection();
                            cx.notify();
                            return false;
                        }
                        true
                    })
                    .unwrap_or(false);
                if !keep_watching {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }
}

/// Parent directories stay watched so an atomic replace is visible. A named
/// certificate or private-key path invalidates inspection. A watcher error or
/// rescan does too, because a missed change would leave a stale success on
/// screen. Access events that do not request a rescan do not.
fn inspection_source_changed(
    event: &notify::Result<notify::Event>,
    sources: &[std::path::PathBuf],
) -> bool {
    let Ok(event) = event else {
        return true;
    };
    if event.need_rescan() {
        return true;
    }
    if matches!(event.kind, notify::EventKind::Access(_)) {
        return false;
    }
    event
        .paths
        .iter()
        .any(|path| sources.iter().any(|source| source == path))
}

/// Deduplicate parent directories, then register every one. Creation, path
/// resolution, or any registration failure drops the watcher.
fn start_source_monitor<W>(
    paths: Result<Vec<std::path::PathBuf>, ()>,
    create: impl FnOnce() -> Result<W, ()>,
    mut watch: impl FnMut(&mut W, &std::path::Path) -> Result<(), ()>,
) -> Result<W, ()> {
    let parents = distinct_certificate_parents(&paths?)?;
    let mut watcher = create()?;
    for parent in &parents {
        watch(&mut watcher, parent)?;
    }
    Ok(watcher)
}

fn distinct_certificate_parents(
    paths: &[std::path::PathBuf],
) -> Result<Vec<std::path::PathBuf>, ()> {
    if paths.is_empty() {
        return Err(());
    }
    let mut parents = Vec::new();
    for path in paths {
        let Some(parent) = path.parent() else {
            return Err(());
        };
        if parent.as_os_str().is_empty() {
            return Err(());
        }
        if parents.iter().any(|seen| seen == parent) {
            continue;
        }
        parents.push(parent.to_path_buf());
    }
    Ok(parents)
}

fn same_public_certificate_source(before: &ClientCertificate, after: &ClientCertificate) -> bool {
    match (before, after) {
        (
            ClientCertificate::Pem {
                certificate_file_path: before,
                ..
            },
            ClientCertificate::Pem {
                certificate_file_path: after,
                ..
            },
        ) => before == after,
        (
            ClientCertificate::Pkcs12 {
                pkcs12_file_path: before,
                ..
            },
            ClientCertificate::Pkcs12 {
                pkcs12_file_path: after,
                ..
            },
        ) => before == after,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use notify::{
        Event, EventKind,
        event::{AccessKind, AccessMode, DataChange, ModifyKind, RenameMode},
    };

    use super::{inspection_source_changed, start_source_monitor};

    fn sources() -> Vec<PathBuf> {
        vec![
            PathBuf::from("/certs/client.pem"),
            PathBuf::from("/certs/client-key.pem"),
        ]
    }

    #[test]
    fn only_certificate_and_key_paths_invalidate_inspection() {
        let sources = sources();
        let changed = |event: Event| inspection_source_changed(&Ok(event), &sources);
        assert!(changed(
            Event::new(EventKind::Modify(ModifyKind::Data(DataChange::Content)))
                .add_path(PathBuf::from("/certs/client.pem"))
        ));
        assert!(changed(
            Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
                .add_path(PathBuf::from("/certs/.client-key.pem.A2K5AL"))
                .add_path(PathBuf::from("/certs/client-key.pem"))
        ));
        assert!(!changed(
            Event::new(EventKind::Modify(ModifyKind::Data(DataChange::Content)))
                .add_path(PathBuf::from("/certs/other.pem"))
        ));
        assert!(!changed(
            Event::new(EventKind::Access(AccessKind::Close(AccessMode::Read)))
                .add_path(PathBuf::from("/certs/client.pem"))
        ));
        assert!(!changed(Event::new(EventKind::Modify(ModifyKind::Any))));
        assert!(inspection_source_changed(
            &Err(notify::Error::new(notify::ErrorKind::Generic(
                "watch failed".into(),
            ))),
            &sources
        ));
        assert!(changed(
            Event::new(EventKind::Access(AccessKind::Close(AccessMode::Read)))
                .add_path(PathBuf::from("/certs/other.pem"))
                .set_flag(notify::event::Flag::Rescan)
        ));
        assert!(changed(
            Event::new(EventKind::Modify(ModifyKind::Any)).set_flag(notify::event::Flag::Rescan)
        ));
    }

    #[test]
    fn watcher_setup_failure_does_not_start_monitoring() {
        let created = std::cell::Cell::new(false);
        assert!(
            start_source_monitor(
                Err(()),
                || {
                    created.set(true);
                    Err(())
                },
                |_: &mut (), _| Ok(()),
            )
            .is_err()
        );
        assert!(!created.get());

        assert!(
            start_source_monitor(
                Ok(Vec::new()),
                || {
                    created.set(true);
                    Err(())
                },
                |_: &mut (), _| Ok(()),
            )
            .is_err()
        );
        assert!(!created.get());

        assert!(
            start_source_monitor(
                Ok(vec![PathBuf::from("client.pem")]),
                || {
                    created.set(true);
                    Err(())
                },
                |_: &mut (), _| Ok(()),
            )
            .is_err()
        );
        assert!(!created.get());

        assert!(
            start_source_monitor(
                Ok(vec![PathBuf::from("/certs/client.pem")]),
                || Err(()),
                |_: &mut (), _| Ok(()),
            )
            .is_err()
        );
    }

    #[test]
    fn partial_directory_registration_is_rejected() {
        let mut watched = Vec::new();
        let result = start_source_monitor(
            Ok(vec![
                PathBuf::from("/certs/client.pem"),
                PathBuf::from("/certs/client-key.pem"),
                PathBuf::from("/keys/client-key.pem"),
            ]),
            || Ok(()),
            |_, parent| {
                watched.push(parent.to_path_buf());
                if parent.ends_with("keys") {
                    Err(())
                } else {
                    Ok(())
                }
            },
        );
        assert!(result.is_err());
        assert_eq!(
            watched,
            vec![PathBuf::from("/certs"), PathBuf::from("/keys")]
        );
    }
}
