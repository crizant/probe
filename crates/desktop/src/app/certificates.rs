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
    pub(crate) index: Option<usize>,
    pub(crate) pkcs12: bool,
    pub(crate) disabled: bool,
    pub(crate) revealed: bool,
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
    pub(crate) fn missing_required_field(&self, cx: &App) -> Option<&'static str> {
        let required: &[(usize, &str)] = if self.pkcs12 {
            &[(0, "Domain"), (3, "PKCS#12 File")]
        } else {
            &[(0, "Domain"), (1, "Certificate"), (2, "Private Key")]
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
        if self.missing_required_field(cx).is_some() {
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
                        if let InputEvent::Change = event {
                            if index == 4
                                && !input.read(cx).value().is_empty()
                                && let Some(editor) = view
                                    .environment_manager_dialog
                                    .as_mut()
                                    .and_then(|dialog| dialog.certificate_editor.as_mut())
                                && editor.inputs.get(4) == Some(input)
                            {
                                editor.preserve_empty_passphrase = false;
                            }
                            cx.notify();
                        }
                    },
                ));
                input
            })
            .collect();
        let dialog = self.environment_manager_dialog.as_mut().unwrap();
        dialog.tab = EnvironmentTab::ClientCertificates;
        dialog.certificate_editor = Some(CertificateEditor {
            index,
            pkcs12,
            disabled,
            revealed: false,
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
        if let Some(dialog) = self.environment_manager_dialog.as_mut() {
            dialog.certificate_editor = None;
        }
        self.environment_manager_dialog_focus.focus(window, cx);
        cx.notify();
    }

    pub(super) fn save_certificate_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.environment_save_task.is_some() {
            return;
        }
        let Some(dialog) = self.environment_manager_dialog.as_mut() else {
            return;
        };
        let Some(editor) = dialog.certificate_editor.as_ref() else {
            return;
        };
        let Some(certificate) = editor.certificate(cx) else {
            return;
        };
        let index = editor.index;
        let certificates = &mut dialog.draft_mut().client_certificates;
        if let Some(index) = index {
            let Some(current) = certificates.get_mut(index) else {
                return;
            };
            *current = certificate;
        } else {
            certificates.push(certificate);
        }
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
