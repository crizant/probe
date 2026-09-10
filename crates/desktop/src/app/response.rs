use super::*;

impl ProbeApp {
    pub(super) fn send_request(&mut self, key: RequestKey, cx: &mut Context<Self>) {
        self.start_request(key, None, cx);
    }

    fn start_request(&mut self, key: RequestKey, output: Option<PathBuf>, cx: &mut Context<Self>) {
        self.transient.request_execution_menu_open = false;
        let Some(request) = self
            .loaded_workspace
            .as_ref()
            .and_then(|loaded| loaded.workspace().request(key))
            .cloned()
        else {
            return;
        };
        let selected_environment = self.shell.selected_environment().map(str::to_owned);
        let request = if let Some(environment_name) = selected_environment {
            let Some(loaded) = &self.loaded_workspace else {
                return;
            };
            match resolve_environment(loaded.workspace().environments(), &environment_name)
                .and_then(|environment| resolve_request(&request, &environment))
            {
                Ok(request) => request,
                Err(error) => {
                    self.execution.fail(key, error.to_string());
                    self.response_viewer.remove(key);
                    cx.notify();
                    return;
                }
            }
        } else {
            request
        };
        let options = ExecutionOptions {
            base_directory: self
                .workspace_path
                .as_deref()
                .and_then(workspace_base_directory),
            response_cache: Some(self.response_cache.clone()),
        };
        let (cancellation_sender, cancellation_receiver) = tokio::sync::oneshot::channel();
        let (result_sender, result_receiver) = tokio::sync::oneshot::channel();
        let (progress_sender, mut progress_receiver) = tokio::sync::mpsc::unbounded_channel();
        let generation = self.execution.begin(key, cancellation_sender);
        let spawn_result = thread::Builder::new()
            .name("probe-http-request".to_owned())
            .spawn(move || {
                let result = execute_http_request(
                    request,
                    options,
                    output,
                    cancellation_receiver,
                    move |progress| {
                        let _ = progress_sender.send(progress);
                    },
                );
                let _ = result_sender.send(result);
            });
        if let Err(error) = spawn_result {
            self.execution
                .fail(key, format!("Could not start HTTP execution: {error}"));
            self.response_viewer.remove(key);
            cx.notify();
            return;
        }

        cx.spawn(async move |view, cx| {
            while let Some(progress) = progress_receiver.recv().await {
                let _ = view.update(cx, |view, cx| {
                    view.execution.report_progress(key, generation, progress);
                    while let Ok(progress) = progress_receiver.try_recv() {
                        view.execution.report_progress(key, generation, progress);
                    }
                    cx.notify();
                });
            }
            let (result, saved_to) = match result_receiver.await {
                Ok(Ok((response, saved_to))) => (Ok(response), saved_to),
                Ok(Err(error)) => (Err(error), None),
                Err(_) => (
                    Err(HttpError::Transport(
                        "HTTP execution ended without a result".to_owned(),
                    )),
                    None,
                ),
            };
            let _ = view.update(cx, |view, cx| {
                view.complete_execution(key, generation, result, saved_to, cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn complete_execution(
        &mut self,
        key: RequestKey,
        generation: u64,
        result: Result<HttpResponse, HttpError>,
        saved_to: Option<SavedResponseBody>,
        cx: &mut Context<Self>,
    ) {
        let completed = result.is_ok();
        self.execution
            .finish(key, generation, result, saved_to.clone());
        self.refresh_response_document(key, cx);
        if completed && let Some(path) = saved_to {
            self.show_toast(
                ToastIntent::Success,
                format!("Response body saved to {}.", path.path().display()),
                cx,
            );
        }
    }

    pub(super) fn refresh_response_document(&mut self, key: RequestKey, cx: &mut Context<Self>) {
        let Some(ResponseState::Complete { response, saved_to }) = self.execution.response(key)
        else {
            self.response_viewer.remove(key);
            return;
        };
        let generation = self.response_viewer.allocate_generation();
        let (mut document, pretty_pending, inspection_pending) =
            prepare_document(response, generation);
        if document.truncated
            && let Some(path) = saved_to
        {
            document.retention_notice = Some(format!(
                "Showing the first {} of {}. The complete response was saved to {}.",
                format_size(document.page_len as u64),
                format_size(document.total_size as u64),
                path.path().display()
            ));
        }
        let pretty_job = pretty_pending.then(|| (response.body.clone(), document.syntax));
        let inspection_source = inspection_pending.then(|| {
            response.body_file.clone().map_or_else(
                || Ok(response.body.clone()),
                |file| Err((file, document.syntax)),
            )
        });
        self.response_viewer.insert(key, document);
        if self.shell.active_tab() == Some(key) {
            self.response_viewer.ensure_available_tab(key);
        }
        self.start_base64_encoding(key, cx);
        self.start_hex_encoding(key, cx);
        if let Some((body, syntax)) = pretty_job {
            cx.spawn(async move |view, cx| {
                let pretty = cx
                    .background_spawn(async move { pretty_body(&body, syntax) })
                    .await;
                let _ = view.update(cx, |view, cx| {
                    view.response_viewer.apply_pretty(key, generation, pretty);
                    cx.notify();
                });
            })
            .detach();
        }
        if let Some(source) = inspection_source {
            cx.spawn(async move |view, cx| {
                let inspection = cx
                    .background_spawn(async move {
                        match source {
                            Ok(body) => inspect_response_body(&body),
                            Err((file, ResponseBodySyntax::Json)) => inspect_json_file(file.path()),
                            Err((file, ResponseBodySyntax::Xml)) => inspect_xml_file(file.path()),
                            Err((_, ResponseBodySyntax::Plain)) => Default::default(),
                        }
                    })
                    .await;
                let _ = view.update(cx, |view, cx| {
                    view.response_viewer
                        .apply_inspection(key, generation, inspection);
                    cx.notify();
                });
            })
            .detach();
        }
    }

    pub(super) fn set_response_tab(&mut self, tab: ResponseViewerTab, cx: &mut Context<Self>) {
        self.response_viewer.set_tab(tab);
        if let Some(key) = self.shell.active_tab() {
            self.start_base64_encoding(key, cx);
        }
        cx.notify();
    }

    pub(super) fn set_raw_body_view(&mut self, view: RawBodyView, cx: &mut Context<Self>) {
        self.response_viewer.set_raw_view(view);
        if let Some(key) = self.shell.active_tab() {
            self.start_base64_encoding(key, cx);
            self.start_hex_encoding(key, cx);
        }
        cx.notify();
    }

    pub(super) fn start_base64_encoding(&mut self, key: RequestKey, cx: &mut Context<Self>) {
        let Some((generation, bytes)) = self.response_viewer.take_base64_job(key) else {
            return;
        };
        cx.spawn(async move |view, cx| {
            let encoded = cx
                .background_spawn(async move { encode_base64(&bytes) })
                .await;
            let _ = view.update(cx, |view, cx| {
                view.response_viewer.apply_base64(key, generation, encoded);
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn start_hex_encoding(&mut self, key: RequestKey, cx: &mut Context<Self>) {
        let Some((generation, bytes)) = self.response_viewer.take_hex_job(key) else {
            return;
        };
        cx.spawn(async move |view, cx| {
            let encoded = cx.background_spawn(async move { encode_hex(&bytes) }).await;
            let _ = view.update(cx, |view, cx| {
                view.response_viewer.apply_hex(key, generation, encoded);
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn load_response_page(
        &mut self,
        key: RequestKey,
        direction: PageDirection,
        cx: &mut Context<Self>,
    ) {
        let Some(ResponseState::Complete { response, .. }) = self.execution.response(key) else {
            return;
        };
        let Some(file) = response.body_file.clone() else {
            return;
        };
        let Some((generation, offset)) = self.response_viewer.begin_page(key, direction) else {
            return;
        };
        let length = response
            .size
            .saturating_sub(offset)
            .min(RESPONSE_PAGE_BYTES);
        cx.spawn(async move |view, cx| {
            let result = cx
                .background_spawn(async move { read_response_page(&file, offset, length) })
                .await;
            let _ = view.update(cx, |view, cx| {
                match result {
                    Ok(body) => {
                        view.response_viewer
                            .apply_page(key, generation, offset, body);
                        view.start_base64_encoding(key, cx);
                        view.start_hex_encoding(key, cx);
                    }
                    Err(error) => view.response_viewer.fail_page(
                        key,
                        generation,
                        format!("Could not read retained response: {error}"),
                    ),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn cancel_request(&mut self, key: RequestKey, cx: &mut Context<Self>) {
        self.execution.cancel(key);
        self.response_viewer.remove(key);
        cx.notify();
    }

    pub(super) fn choose_send_and_save(
        &mut self,
        key: RequestKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(request) = self
            .loaded_workspace
            .as_ref()
            .and_then(|loaded| loaded.workspace().request(key))
        else {
            return;
        };
        self.transient.request_execution_menu_open = false;
        cx.notify();
        let filename = suggested_request_filename(request);
        let receiver = cx.prompt_for_new_path(&download_directory(), Some(&filename));
        let view = cx.weak_entity();
        window
            .spawn(cx, async move |cx| {
                let destination = match receiver.await {
                    Ok(Ok(Some(path))) => path,
                    Ok(Ok(None)) | Err(_) => return,
                    Ok(Err(error)) => {
                        let _ = view.update_in(cx, |view, _, cx| {
                            view.show_toast(
                                ToastIntent::Error,
                                format!("Could not open the download destination picker: {error}"),
                                cx,
                            );
                        });
                        return;
                    }
                };
                let _ = view.update_in(cx, |view, _, cx| {
                    view.start_request(key, Some(destination), cx);
                });
            })
            .detach();
    }

    pub(super) fn choose_response_save(
        &mut self,
        key: RequestKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ResponseState::Complete { response, saved_to }) = self.execution.response(key)
        else {
            return;
        };
        let response = response.clone();
        let saved_to = saved_to.clone();
        let filename = suggested_response_filename(&response);
        let receiver = cx.prompt_for_new_path(&download_directory(), Some(&filename));
        let view = cx.weak_entity();
        window
            .spawn(cx, async move |cx| {
                let destination = match receiver.await {
                    Ok(Ok(Some(path))) => path,
                    Ok(Ok(None)) | Err(_) => return,
                    Ok(Err(error)) => {
                        let _ = view.update_in(cx, |view, _, cx| {
                            view.show_toast(
                                ToastIntent::Error,
                                format!("Could not open the save destination picker: {error}"),
                                cx,
                            );
                        });
                        return;
                    }
                };
                let saved_destination = destination.clone();
                let result = cx
                    .background_spawn(async move {
                        save_response_body(&response, saved_to.as_ref(), &destination)
                    })
                    .await;
                let _ = view.update_in(cx, |view, _, cx| match result {
                    Ok(()) => view.show_toast(
                        ToastIntent::Success,
                        format!("Response body saved to {}.", saved_destination.display()),
                        cx,
                    ),
                    Err(error) => view.show_toast(
                        ToastIntent::Error,
                        format!("Could not save the response body: {error}"),
                        cx,
                    ),
                });
            })
            .detach();
    }
}
