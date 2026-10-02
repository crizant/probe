use super::*;
use crate::app::SecretUiStatus;
use crate::credentials::{CredentialId, CredentialStore, CredentialStoreError};
use gpui::{Context, Window, WindowHandle};
use std::collections::HashMap;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc,
};

mod manager;
mod secret_execution;
mod secrets;
mod variables;

const SECRET_SENTINEL: &str = "SUPER_SECRET_VALUE_THAT_MUST_NEVER_APPEAR";

/// In-memory credential store. `get` panics unless execution reads were allowed,
/// so every test also proves that presentation never reads a secret value.
#[derive(Default)]
struct FakeCredentials {
    values: Mutex<HashMap<CredentialId, String>>,
    get_calls: AtomicUsize,
    allow_get: AtomicBool,
    fail_set: AtomicBool,
    fail_delete: AtomicBool,
    read_gate: Mutex<Option<mpsc::Receiver<()>>>,
}

impl FakeCredentials {
    fn value(&self, id: &CredentialId) -> Option<String> {
        self.values.lock().unwrap().get(id).cloned()
    }

    fn get_calls(&self) -> usize {
        self.get_calls.load(Ordering::SeqCst)
    }

    fn allow_execution_reads(&self) {
        self.allow_get.store(true, Ordering::SeqCst);
    }

    /// Blocks reads after they capture a value until the returned sender fires.
    fn hold_reads(&self) -> mpsc::Sender<()> {
        let (release, gate) = mpsc::channel();
        *self.read_gate.lock().unwrap() = Some(gate);
        release
    }
}

impl CredentialStore for FakeCredentials {
    fn set(&self, id: &CredentialId, value: &str) -> Result<(), CredentialStoreError> {
        if self.fail_set.load(Ordering::SeqCst) {
            return Err(CredentialStoreError::BackendFailure);
        }
        self.values
            .lock()
            .unwrap()
            .insert(id.clone(), value.to_owned());
        Ok(())
    }

    fn delete(&self, id: &CredentialId) -> Result<(), CredentialStoreError> {
        if self.fail_delete.load(Ordering::SeqCst) {
            return Err(CredentialStoreError::BackendFailure);
        }
        self.values
            .lock()
            .unwrap()
            .remove(id)
            .map(|_| ())
            .ok_or(CredentialStoreError::NotFound)
    }

    fn get(
        &self,
        id: &CredentialId,
    ) -> Result<Option<probe_core::SecretValue>, CredentialStoreError> {
        assert!(
            self.allow_get.load(Ordering::SeqCst),
            "presentation must not read credential values"
        );
        let value = self.value(id);
        self.get_calls.fetch_add(1, Ordering::SeqCst);
        if let Some(gate) = self.read_gate.lock().unwrap().as_ref() {
            let _ = gate.recv_timeout(Duration::from_secs(10));
        }
        Ok(value.map(probe_core::SecretValue::new))
    }
}

/// A desktop window with the environment fixture loaded and a fake credential store.
struct EnvironmentWorkspace {
    window: WindowHandle<ProbeApp>,
    path: PathBuf,
    store: Arc<FakeCredentials>,
    temporary: bool,
}

impl EnvironmentWorkspace {
    fn open(cx: &mut TestAppContext) -> Self {
        Self::load(cx, environment_fixture().canonicalize().unwrap(), false)
    }

    fn writable(cx: &mut TestAppContext, suffix: &str) -> Self {
        let path = writable_environment_fixture(suffix).canonicalize().unwrap();
        Self::load(cx, path, true)
    }

    fn load(cx: &mut TestAppContext, path: PathBuf, temporary: bool) -> Self {
        cx.update(Theme::init);
        let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
            ProbeApp::new(window, cx)
        });
        let store = Arc::new(FakeCredentials::default());
        let workspace = probe_opencollection::load_workspace(&path).unwrap();
        window
            .update(cx, |view, _, _| {
                view.session_store = None;
                view.credential_store = store.clone();
                view.set_workspace(path.clone(), workspace);
            })
            .unwrap();
        Self {
            window,
            path,
            store,
            temporary,
        }
    }

    fn update<R>(
        &self,
        cx: &mut TestAppContext,
        update: impl FnOnce(&mut ProbeApp, &mut Window, &mut Context<ProbeApp>) -> R,
    ) -> R {
        self.window
            .update(cx, update)
            .expect("test window should remain open")
    }

    fn visual(&self, cx: &TestAppContext) -> VisualTestContext {
        VisualTestContext::from_window(self.window.into(), cx)
    }

    fn select_environment(&self, cx: &mut TestAppContext, name: &str) {
        self.update(cx, |view, _, cx| {
            view.select_environment(Some(name.to_owned()), cx)
        });
    }

    fn open_manager(&self, cx: &mut TestAppContext, environment: &str) {
        self.update(cx, |view, window, cx| {
            view.select_environment(Some(environment.to_owned()), cx);
            view.open_environment_manager_dialog(window, cx);
        });
        cx.run_until_parked();
    }

    fn add_manager_variables(&self, cx: &mut TestAppContext, prefix: &str, count: usize) {
        self.update(cx, |view, _, cx| {
            view.apply_environment_manager_draft(cx, |dialog| {
                for index in 0..count {
                    dialog.add_variable(EnvironmentVariable::Plain(Variable {
                        name: Some(format!("{prefix}-{index}")),
                        value: Some(VariableValueSet::Single(VariableValue::String(format!(
                            "value-{index}"
                        )))),
                        disabled: false,
                    }));
                }
            });
        });
    }

    fn scroll_manager_to(
        &self,
        cx: &mut TestAppContext,
        index: usize,
        strategy: gpui::ScrollStrategy,
    ) {
        self.update(cx, |view, _, cx| {
            view.environment_variables_scroll
                .scroll_to_item_strict(index, strategy);
            cx.notify();
        });
    }

    fn assert_manager_field_focus(
        &self,
        cx: &mut TestAppContext,
        focus: &gpui::FocusHandle,
        controller: gpui::EntityId,
    ) {
        self.update(cx, |view, window, cx| {
            assert_eq!(window.focused(cx).as_ref(), Some(focus));
            let active = view
                .environment_manager_dialog
                .as_ref()
                .unwrap()
                .active_field
                .as_ref()
                .expect("focused field should retain its controller");
            assert_eq!(active.2.entity_id(), controller);
        });
    }

    fn credential(&self, environment: &str, name: &str) -> CredentialId {
        CredentialId::for_workspace(&self.path, environment, name).unwrap()
    }

    fn development_secret(&self) -> CredentialId {
        self.credential("development", "secretToken")
    }

    /// Records presence without a native write, as a restored session would.
    fn mark_stored(&self, cx: &mut TestAppContext, id: &CredentialId) {
        self.update(cx, |view, _, _| {
            view.session
                .presence
                .stored_credentials
                .insert(id.persistence_key().to_owned());
        });
    }

    /// A credential Probe stored earlier: present natively and in presence metadata.
    fn store_secret(&self, cx: &mut TestAppContext, id: &CredentialId, value: &str) {
        self.store.set(id, value).unwrap();
        self.mark_stored(cx, id);
    }

    fn yaml(&self) -> String {
        fs::read_to_string(&self.path).unwrap()
    }
}

impl Drop for EnvironmentWorkspace {
    fn drop(&mut self) {
        if self.temporary {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// `Some(true)` when known stored, `Some(false)` when known missing, `None` when unknown.
fn presence(view: &ProbeApp, id: &CredentialId) -> Option<bool> {
    let key = id.persistence_key();
    if view.session.presence.is_stored(key) {
        Some(true)
    } else if view.session.presence.is_missing(key) {
        Some(false)
    } else {
        None
    }
}

fn manager_status(view: &ProbeApp, name: &str) -> Option<SecretUiStatus> {
    view.environment_manager_dialog
        .as_ref()?
        .secret_statuses
        .get(name)
        .copied()
}

fn type_secret(view: &mut ProbeApp, window: &mut Window, cx: &mut Context<ProbeApp>, value: &str) {
    let input = view
        .secret_value_dialog
        .as_ref()
        .expect("secret value dialog should be open")
        .input
        .clone();
    input.update(cx, |input, cx| input.set_value(value, window, cx));
}

fn secret_input_is_empty(view: &ProbeApp, cx: &gpui::App) -> bool {
    view.secret_value_dialog
        .as_ref()
        .expect("secret value dialog should be open")
        .input
        .read(cx)
        .value()
        .is_empty()
}

fn assert_unknown_secret(context: &crate::components::VariableContext, name: &str) {
    assert!(
        context.unknown_secrets.contains(name),
        "{name} should stay unverified until Probe learns its presence"
    );
    assert!(!context.secrets.contains(name));
    assert!(!context.resolved_secrets.contains(name));
    assert_ne!(
        context.status(name),
        probe_core::VariableStatus::SecretWithoutValue
    );
    assert_ne!(context.status(name), probe_core::VariableStatus::Resolved);
}

fn assert_secret_stays_out_of_editor_context(context: &crate::components::VariableContext) {
    assert!(!context.values.contains_key("secretToken"));
    assert!(
        !format!("{context:?}").contains(SECRET_SENTINEL),
        "editor variable context must not contain the secret value: {context:?}"
    );
}
