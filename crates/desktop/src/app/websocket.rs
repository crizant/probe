use super::*;
use crate::websocket::ConnectionState;

impl ProbeApp {
    pub(super) fn websocket_control(&mut self, key: RequestKey, cx: &mut Context<Self>) {
        if let Some(session) = self.websocket.sessions.get_mut(&key)
            && matches!(
                session.state,
                ConnectionState::Connecting | ConnectionState::Connected | ConnectionState::Closing
            )
        {
            session.disconnect();
            cx.notify();
            return;
        }
        self.connect_websocket(key, cx);
    }
    pub(super) fn connect_websocket(&mut self, key: RequestKey, cx: &mut Context<Self>) {
        if self.websocket.sessions.get(&key).is_some_and(|s| {
            matches!(
                s.state,
                ConnectionState::Connecting | ConnectionState::Connected | ConnectionState::Closing
            )
        }) {
            return;
        }
        let Some(loaded) = &self.loaded_workspace else {
            return;
        };
        let Some(request) = loaded.workspace().request(key).cloned() else {
            return;
        };
        let input = ExecutionInput {
            request,
            environments: loaded.workspace().environments().to_vec(),
            environment: self.shell.selected_environment().map(str::to_owned),
            credential_workspace: loaded.source_path().map(Path::to_owned),
        };
        if self.execution_service.is_none() {
            match ExecutionService::with_credentials(Arc::clone(&self.credential_store)) {
                Ok(service) => self.execution_service = Some(service),
                Err(_) => {
                    self.show_toast(
                        ToastIntent::Error,
                        "Could not start WebSocket execution.",
                        cx,
                    );
                    return;
                }
            }
        }
        let (cancel, cancellation) = tokio::sync::oneshot::channel();
        let generation = self.websocket.begin(key, cancel);
        let (mut events, presence) = self
            .execution_service
            .as_ref()
            .unwrap()
            .connect_websocket(input, cancellation);
        let revision = self.session.presence.revision();
        cx.spawn(async move |view, cx| {
            if let Ok(presence) = presence.await {
                let _ = view.update(cx, |view, cx| {
                    view.apply_secret_presence_reconciliation(presence, revision, cx)
                });
            }
        })
        .detach();
        cx.spawn(async move |view, cx| {
            while let Some(event) = events.recv().await {
                if view
                    .update(cx, |view, cx| {
                        view.websocket.apply(generation, event);
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }
    pub(super) fn send_websocket_message(&mut self, key: RequestKey, cx: &mut Context<Self>) {
        let Some(state) = self.websocket.sessions.get_mut(&key) else {
            return;
        };
        if state.state != ConnectionState::Connected || state.sending {
            return;
        }
        let Some(sender) = state.sender.clone() else {
            return;
        };
        let text = state.draft.clone();
        let generation = state.generation;
        state.sending = true;
        let result = self
            .execution_service
            .as_ref()
            .unwrap()
            .send_websocket(sender, text.clone());
        cx.spawn(async move |view, cx| {
            let result = result
                .await
                .unwrap_or(Err(probe_application::SessionError::Closed));
            let _ = view.update(cx, |view, cx| {
                if let Some(state) = view
                    .websocket
                    .sessions
                    .values_mut()
                    .find(|s| s.generation == generation)
                {
                    state.sending = false;
                    if result.is_ok() && state.draft == text {
                        state.draft.clear();
                    }
                    if let Err(error) = result {
                        view.show_toast(ToastIntent::Error, error.to_string(), cx);
                    }
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }
}
