use super::*;
use probe_core::VariableStatus::{Resolved, SecretWithoutValue};

fn park_until(
    cx: &mut TestAppContext,
    mut ready: impl FnMut(&mut TestAppContext) -> bool,
    waiting_for: &str,
) {
    // Secret resolution finishes on the execution runtime and wakes the GPUI
    // task from another thread. One `run_until_parked` can return before that
    // wake is visible, which is common on slower Windows CI hosts.
    cx.executor().allow_parking();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        cx.run_until_parked();
        if ready(cx) {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for {waiting_for}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Sends the first request with `{{secretToken}}` in its URL. The address refuses
/// connections, so the send fails quickly after secret resolution.
fn send_secret_request(
    workspace: &EnvironmentWorkspace,
    cx: &mut TestAppContext,
) -> probe_core::RequestKey {
    workspace.store.allow_execution_reads();
    workspace.update(cx, |view, _, cx| {
        let key = view.loaded_workspace.as_ref().unwrap().requests()[0].key();
        view.select_environment(Some("development".into()), cx);
        view.select_request(key, cx);
        view.edit_request(
            key,
            |request| request.url = Some("http://127.0.0.1:1/{{secretToken}}".to_owned()),
            cx,
        );
        view.send_request(key, cx);
        key
    })
}

fn wait_for_response(
    workspace: &EnvironmentWorkspace,
    cx: &mut TestAppContext,
    key: probe_core::RequestKey,
) {
    // `begin` records `Running` before secret resolution or the socket call
    // finishes. Presence can update while that state is still current.
    park_until(
        cx,
        |cx| {
            workspace.update(cx, |view, _, _| {
                view.execution
                    .response(key)
                    .is_some_and(|state| !state.is_running())
            })
        },
        "request execution",
    );
}

fn assert_failed_without_secret(view: &ProbeApp, key: probe_core::RequestKey) {
    match view.execution.response(key) {
        Some(crate::execution::ResponseState::Failed(message)) => {
            assert!(!message.contains(SECRET_SENTINEL));
        }
        other => panic!("expected a failed send after resolution, got {other:?}"),
    }
}

#[gpui::test]
fn execution_marks_stale_presence_missing_when_the_native_secret_is_absent(
    cx: &mut TestAppContext,
) {
    let workspace = EnvironmentWorkspace::open(cx);
    let id = workspace.development_secret();
    workspace.mark_stored(cx, &id);
    let key = send_secret_request(&workspace, cx);
    wait_for_response(&workspace, cx, key);
    workspace.update(cx, |view, _, cx| {
        assert_eq!(presence(view, &id), Some(false));
        assert_eq!(
            view.variable_context(cx).status("secretToken"),
            SecretWithoutValue
        );
        assert_failed_without_secret(view, key);
    });
    assert_eq!(workspace.store.get_calls(), 1);
}

#[gpui::test]
fn execution_reads_the_native_value_and_relearns_presence(cx: &mut TestAppContext) {
    let workspace = EnvironmentWorkspace::open(cx);
    let id = workspace.development_secret();
    workspace.store.set(&id, SECRET_SENTINEL).unwrap();
    workspace.update(cx, |view, _, _| assert_eq!(presence(view, &id), None));
    let key = send_secret_request(&workspace, cx);
    wait_for_response(&workspace, cx, key);
    workspace.update(cx, |view, _, cx| {
        assert_eq!(presence(view, &id), Some(true));
        let context = view.variable_context(cx);
        assert_eq!(context.status("secretToken"), Resolved);
        assert_secret_stays_out_of_editor_context(&context);
        assert!(
            !serde_json::to_string(&view.session)
                .unwrap()
                .contains(SECRET_SENTINEL)
        );
        assert_failed_without_secret(view, key);
    });
    assert_eq!(workspace.store.get_calls(), 1);
}

enum CredentialWrite {
    Set,
    Delete,
}

/// Holds execution after it reads the credential, applies a user write, then lets
/// the older observation arrive. The write must win.
fn assert_stale_execution_cannot_override(cx: &mut TestAppContext, write: CredentialWrite) {
    let workspace = EnvironmentWorkspace::open(cx);
    let id = workspace.development_secret();
    if matches!(write, CredentialWrite::Delete) {
        workspace.store_secret(cx, &id, SECRET_SENTINEL);
    }
    let release = workspace.store.hold_reads();
    let key = send_secret_request(&workspace, cx);
    park_until(
        cx,
        |_| workspace.store.get_calls() >= 1,
        "execution to read the credential",
    );

    workspace.open_manager(cx, "development");
    workspace.update(cx, |view, window, cx| {
        view.open_secret_value_dialog("secretToken".into(), window, cx);
        match write {
            CredentialWrite::Set => {
                type_secret(view, window, cx, "newer-value");
                view.save_secret_value(window, cx);
            }
            CredentialWrite::Delete => {
                view.delete_stored_secret("secretToken".into(), "development".into(), window, cx)
            }
        }
    });
    cx.run_until_parked();
    let expected = matches!(write, CredentialWrite::Set);
    workspace.update(cx, |view, _, _| {
        assert_eq!(presence(view, &id), Some(expected))
    });

    release.send(()).unwrap();
    drop(release);
    wait_for_response(&workspace, cx, key);
    workspace.update(cx, |view, _, cx| {
        assert_eq!(
            presence(view, &id),
            Some(expected),
            "an execution observation from before the write must not replace it"
        );
        let status = view.variable_context(cx).status("secretToken");
        assert_eq!(
            status,
            if expected {
                Resolved
            } else {
                SecretWithoutValue
            }
        );
    });
}

#[gpui::test]
fn stale_execution_cannot_restore_a_secret_deleted_while_it_resolved(cx: &mut TestAppContext) {
    assert_stale_execution_cannot_override(cx, CredentialWrite::Delete);
}

#[gpui::test]
fn stale_execution_cannot_forget_a_secret_set_while_it_resolved(cx: &mut TestAppContext) {
    assert_stale_execution_cannot_override(cx, CredentialWrite::Set);
}
