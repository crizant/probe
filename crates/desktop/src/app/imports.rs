pub(crate) use probe_application::imports::{
    CollectionImport, ImportConversion, ImportSource, inspect_import,
};

use super::*;

pub(crate) fn import_source_label(source: ImportSource) -> &'static str {
    match source {
        ImportSource::Postman => "Postman",
        ImportSource::Yaak => "Yaak",
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(super) enum CollectionPathResolution {
    Open(PathBuf),
    Choose(Vec<PathBuf>),
}

pub(super) fn resolve_collection_path(path: PathBuf) -> Result<CollectionPathResolution, String> {
    if path.is_file() {
        return Ok(CollectionPathResolution::Open(path));
    }
    if !path.is_dir() {
        return Err(format!("{} is not a file or folder", path.display()));
    }

    if load_workspace(&path).is_ok() {
        return Ok(CollectionPathResolution::Open(path));
    }

    let entries = fs::read_dir(&path)
        .map_err(|error| format!("could not inspect {}: {error}", path.display()))?;
    let mut candidates = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|candidate| candidate.is_file() && is_yaml_file(candidate))
        .filter(|candidate| {
            load_workspace(candidate).is_ok_and(|workspace| !workspace.uses_path_locators())
        })
        .collect::<Vec<_>>();
    candidates.sort();

    match candidates.len() {
        0 => Err(format!(
            "No OpenCollection workspace was found in {}. Select an unbundled collection folder or a folder containing a bundled .yml or .yaml collection.",
            path.display()
        )),
        1 => Ok(CollectionPathResolution::Open(candidates.remove(0))),
        _ => Ok(CollectionPathResolution::Choose(candidates)),
    }
}

fn is_yaml_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some(extension) if extension.eq_ignore_ascii_case("yml") || extension.eq_ignore_ascii_case("yaml")
    )
}

impl ProbeApp {
    pub(super) fn choose_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: cfg!(target_os = "macos"),
            directories: true,
            multiple: false,
            prompt: Some("Open Collection".into()),
        });
        let view = cx.weak_entity();

        window
            .spawn(cx, async move |cx| {
                let paths = match receiver.await {
                    Ok(Ok(Some(paths))) => paths,
                    Ok(Ok(None)) => return,
                    Ok(Err(error)) => {
                        let _ = view.update_in(cx, |view, _, cx| {
                            view.show_toast(
                                ToastIntent::Error,
                                format!("Could not open the file picker: {error}"),
                                cx,
                            );
                        });
                        return;
                    }
                    Err(_) => return,
                };
                let Some(path) = paths.into_iter().next() else {
                    return;
                };
                let result = cx
                    .background_spawn(async move { resolve_collection_path(path) })
                    .await;
                let _ = view.update_in(cx, |view, window, cx| match result {
                    Ok(CollectionPathResolution::Open(path)) => {
                        view.request_load_workspace(path, None, window, cx);
                    }
                    Ok(CollectionPathResolution::Choose(candidates)) => {
                        view.show_application_dialog(
                            ApplicationDialog::SelectCollectionFile { candidates },
                            window,
                            cx,
                        );
                    }
                    Err(error) => {
                        view.show_toast(ToastIntent::Error, error, cx);
                    }
                });
            })
            .detach();
    }

    pub(super) fn choose_new_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let directory = self.new_collection_directory();
        let receiver = cx.prompt_for_new_path(&directory, Some("Untitled.yml"));
        let view = cx.weak_entity();

        window
            .spawn(cx, async move |cx| {
                let path = match receiver.await {
                    Ok(Ok(Some(path))) => path,
                    Ok(Ok(None)) => return,
                    Ok(Err(error)) => {
                        let _ = view.update_in(cx, |view, _, cx| {
                            view.show_toast(
                                ToastIntent::Error,
                                format!("Could not open the file picker: {error}"),
                                cx,
                            );
                        });
                        return;
                    }
                    Err(_) => return,
                };
                let _ = view.update_in(cx, |view, window, cx| {
                    view.request_create_workspace(path, window, cx);
                });
            })
            .detach();
    }

    pub(super) fn request_import(
        &mut self,
        source: ImportSource,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request_workspace_transition(PendingClose::Import(source), window, cx);
    }

    pub(super) fn choose_import(
        &mut self,
        source: ImportSource,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let source_label = import_source_label(source);
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: matches!(source, ImportSource::Yaak),
            multiple: false,
            prompt: Some(format!("Import from {source_label}").into()),
        });
        let view = cx.weak_entity();
        window
            .spawn(cx, async move |cx| {
                let paths = match receiver.await {
                    Ok(Ok(Some(paths))) => paths,
                    Ok(Ok(None)) | Err(_) => return,
                    Ok(Err(error)) => {
                        let _ = view.update_in(cx, |view, _, cx| {
                            view.show_toast(
                                ToastIntent::Error,
                                format!("Could not open the {source_label} source picker: {error}"),
                                cx,
                            );
                        });
                        return;
                    }
                };
                let Some(path) = paths.into_iter().next() else {
                    return;
                };
                let _ = view.update_in(cx, |view, _, cx| {
                    view.loading = true;
                    cx.notify();
                });
                let inspected = cx
                    .background_spawn(async move {
                        let conversion = inspect_import(source, path)?;
                        let workspaces = match &conversion {
                            ImportConversion::Yaak { preview, .. } => preview.workspaces(),
                            _ => Vec::new(),
                        };
                        Ok::<_, probe_application::imports::ImportError>((conversion, workspaces))
                    })
                    .await;
                let _ = view.update_in(cx, |view, window, cx| match inspected {
                    Ok((ImportConversion::Yaak { preview, .. }, workspaces))
                        if workspaces.len() != 1 =>
                    {
                        view.show_application_dialog(
                            ApplicationDialog::SelectYaakWorkspace {
                                preview,
                                workspaces,
                            },
                            window,
                            cx,
                        );
                    }
                    Ok((conversion, _)) => view.convert_import(conversion, false, window, cx),
                    Err(error) => {
                        view.loading = false;
                        view.show_toast(
                            ToastIntent::Error,
                            format!("Could not inspect {source_label} data: {error}"),
                            cx,
                        );
                    }
                });
            })
            .detach();
    }

    pub(super) fn convert_import(
        &mut self,
        conversion: ImportConversion,
        allow_partial: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let source_label = import_source_label(conversion.source());
        self.application_dialog = None;
        self.loading = true;
        cx.notify();
        let view = cx.weak_entity();
        window
            .spawn(cx, async move |cx| {
                let (conversion, result) = cx
                    .background_spawn(async move {
                        let result = conversion.convert(allow_partial).map_err(|error| {
                            (
                                error.diagnostics().map(format_import_diagnostics),
                                error.to_string(),
                            )
                        });
                        (conversion, result)
                    })
                    .await;
                let _ = view.update_in(cx, |view, window, cx| match result {
                    Ok(import) => {
                        view.choose_import_destination(import, window, cx);
                    }
                    Err((Some(detail), _)) if !allow_partial => {
                        view.loading = false;
                        view.show_application_dialog(
                            ApplicationDialog::ConfirmPartialImport { conversion, detail },
                            window,
                            cx,
                        );
                    }
                    Err((_, error)) => {
                        view.loading = false;
                        view.show_toast(
                            ToastIntent::Error,
                            format!("Could not convert {source_label} data: {error}"),
                            cx,
                        );
                    }
                });
            })
            .detach();
    }

    pub(super) fn choose_import_destination(
        &mut self,
        import: CollectionImport,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let source_label = import_source_label(import.source());
        let imported_kind = match import.source() {
            ImportSource::Postman => "collection",
            ImportSource::Yaak => "workspace",
        };
        let filename = suggested_collection_filename(import.source_name());
        let warning_count = import.diagnostics.len();
        let selected_environment = import.selected_environment.clone();
        let receiver = cx.prompt_for_new_path(&self.new_collection_directory(), Some(&filename));
        let view = cx.weak_entity();
        window
            .spawn(cx, async move |cx| {
                let destination = match receiver.await {
                    Ok(Ok(Some(path))) => path,
                    Ok(Ok(None)) | Err(_) => {
                        let _ = view.update_in(cx, |view, _, cx| {
                            view.loading = false;
                            cx.notify();
                        });
                        return;
                    }
                    Ok(Err(error)) => {
                        let _ = view.update_in(cx, |view, _, cx| {
                            view.loading = false;
                            view.show_toast(
                                ToastIntent::Error,
                                format!("Could not open the import destination picker: {error}"),
                                cx,
                            );
                        });
                        return;
                    }
                };
                let result = cx
                    .background_spawn(async move {
                        let workspace =
                            import.create_workspace(&destination)
                                .map_err(|error| error.to_string())?;
                        let canonical_path = workspace
                            .source_path()
                            .ok_or_else(|| {
                                format!(
                                    "imported collection at {} has no filesystem path",
                                    destination.display()
                                )
                            })?
                            .to_owned();
                        Ok::<_, String>((canonical_path, workspace))
                    })
                    .await;
                let _ = view.update_in(cx, |view, window, cx| {
                    view.loading = false;
                    match result {
                        Ok((path, workspace)) => {
                            let projection_warning = workspace::projection_warning(&workspace);
                            view.set_workspace(path, workspace);
                            if let Some(message) = projection_warning {
                                view.show_toast(ToastIntent::Warning, message, cx);
                            }
                            if let Some(environment) = selected_environment {
                                view.shell.select_environment(Some(environment));
                                view.capture_selected_environment();
                            }
                            view.start_workspace_watcher(window, cx);
                            view.persist_session(cx);
                            if warning_count > 0 {
                                view.show_toast(
                                    ToastIntent::Warning,
                                    format!(
                                        "Imported {source_label} {imported_kind} with {} warning(s).",
                                        warning_count
                                    ),
                                    cx,
                                );
                            }
                        }
                        Err(error) => {
                            view.show_toast(
                                ToastIntent::Error,
                                format!("Could not import {source_label} data: {error}"),
                                cx,
                            );
                        }
                    }
                    cx.notify();
                });
            })
            .detach();
    }

    pub(super) fn new_collection_directory(&self) -> PathBuf {
        if let Some(base) = self
            .workspace_path
            .as_deref()
            .and_then(workspace_base_directory)
            .filter(|path| path.is_dir())
        {
            return base;
        }
        directories::UserDirs::new()
            .and_then(|dirs| {
                dirs.document_dir()
                    .map(Path::to_owned)
                    .or_else(|| Some(dirs.home_dir().to_owned()))
            })
            .filter(|path| path.is_dir())
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."))
    }

    pub(super) fn choose_file_path(
        &mut self,
        key: RequestKey,
        window: &mut Window,
        cx: &mut Context<Self>,
        apply: impl FnOnce(&mut Request, String) + Send + 'static,
    ) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose File".into()),
        });
        let workspace_path = self.workspace_path.clone();
        let view = cx.weak_entity();

        window
            .spawn(cx, async move |cx| {
                let paths = match receiver.await {
                    Ok(Ok(Some(paths))) => paths,
                    Ok(Ok(None)) => return,
                    Ok(Err(error)) => {
                        let _ = view.update_in(cx, |view, _, cx| {
                            view.show_toast(
                                ToastIntent::Error,
                                format!("Could not open the file picker: {error}"),
                                cx,
                            );
                        });
                        return;
                    }
                    Err(_) => return,
                };
                let Some(path) = paths.into_iter().next() else {
                    return;
                };
                let stored = body_file_path_for_storage(&path, workspace_path.as_deref());
                let _ = view.update_in(cx, |view, _, cx| {
                    view.edit_request(key, |request| apply(request, stored), cx);
                });
            })
            .detach();
    }

    pub(super) fn choose_body_file(
        &mut self,
        key: RequestKey,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.choose_file_path(key, window, cx, move |request, stored| {
            if let Some(RequestBody::Single(Body::File(files))) = request.http_body_mut()
                && let Some(file) = files.get_mut(index)
            {
                file.file_path = stored;
            }
        });
    }

    pub(super) fn choose_multipart_file(
        &mut self,
        key: RequestKey,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.choose_file_path(key, window, cx, move |request, stored| {
            if let Some(RequestBody::Single(Body::Multipart(parts))) = request.http_body_mut()
                && let Some(part) = parts.get_mut(index)
            {
                part.value = MultipartValue::Single(stored);
            }
        });
    }
}
