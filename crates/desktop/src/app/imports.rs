use probe_core::ImportDiagnostic;
use probe_postman::{ImportedPostmanCollection, PostmanImportPreview, inspect_postman_source};
use probe_yaak::{
    ImportedYaakWorkspace, YaakImportPreview, YaakWorkspaceSummary, inspect_yaak_source,
};

use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ImportSource {
    Postman,
    Yaak,
}

impl ImportSource {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Postman => "Postman",
            Self::Yaak => "Yaak",
        }
    }

    const fn imported_kind(self) -> &'static str {
        match self {
            Self::Postman => "collection",
            Self::Yaak => "workspace",
        }
    }

    fn picker_options(self) -> PathPromptOptions {
        PathPromptOptions {
            files: true,
            directories: matches!(self, Self::Yaak),
            multiple: false,
            prompt: Some(format!("Import from {}", self.label()).into()),
        }
    }
}

pub(crate) enum ImportConversion {
    Postman(Box<PostmanImportPreview>),
    Yaak {
        preview: Box<YaakImportPreview>,
        workspace_id: String,
    },
}

impl ImportConversion {
    pub(crate) const fn source(&self) -> ImportSource {
        match self {
            Self::Postman(_) => ImportSource::Postman,
            Self::Yaak { .. } => ImportSource::Yaak,
        }
    }

    pub(crate) fn convert(self, allow_partial: bool) -> ImportConversionResult {
        let converted = match &self {
            Self::Postman(preview) => preview
                .convert(allow_partial)
                .map(CollectionImport::from)
                .map_err(|error| conversion_error(error.diagnostics(), &error)),
            Self::Yaak {
                preview,
                workspace_id,
            } => preview
                .convert(Some(workspace_id), allow_partial)
                .map(CollectionImport::from)
                .map_err(|error| conversion_error(error.diagnostics(), &error)),
        };
        match converted {
            Ok(import) => ImportConversionResult::Imported(Box::new(import)),
            Err(ConversionError {
                unsupported: Some(detail),
                ..
            }) if !allow_partial => ImportConversionResult::NeedsPartialConfirmation {
                conversion: self,
                detail,
            },
            Err(ConversionError { message, .. }) => ImportConversionResult::Failed(message),
        }
    }
}

struct ConversionError {
    unsupported: Option<String>,
    message: String,
}

fn conversion_error(
    unsupported: Option<&[ImportDiagnostic]>,
    error: &impl std::fmt::Display,
) -> ConversionError {
    ConversionError {
        unsupported: unsupported.map(format_import_diagnostics),
        message: error.to_string(),
    }
}

pub(crate) enum ImportConversionResult {
    Imported(Box<CollectionImport>),
    NeedsPartialConfirmation {
        conversion: ImportConversion,
        detail: String,
    },
    Failed(String),
}

pub(crate) enum InspectedImport {
    Ready(ImportConversion),
    SelectYaakWorkspace {
        preview: Box<YaakImportPreview>,
        workspaces: Vec<YaakWorkspaceSummary>,
    },
}

pub(crate) fn inspect_import(
    source: ImportSource,
    path: PathBuf,
) -> Result<InspectedImport, String> {
    match source {
        ImportSource::Postman => inspect_postman_source(path)
            .map(|preview| InspectedImport::Ready(ImportConversion::Postman(Box::new(preview))))
            .map_err(|error| error.to_string()),
        ImportSource::Yaak => {
            let preview = Box::new(inspect_yaak_source(path).map_err(|error| error.to_string())?);
            let workspaces = preview.workspaces();
            Ok(match workspaces.as_slice() {
                [workspace] => InspectedImport::Ready(ImportConversion::Yaak {
                    workspace_id: workspace.id.clone(),
                    preview,
                }),
                _ => InspectedImport::SelectYaakWorkspace {
                    preview,
                    workspaces,
                },
            })
        }
    }
}

pub(crate) struct CollectionImport {
    pub(super) source: ImportSource,
    pub(super) source_name: String,
    pub(super) collection: Collection,
    pub(super) warning_count: usize,
    pub(super) selected_environment: Option<String>,
}

impl From<ImportedPostmanCollection> for CollectionImport {
    fn from(imported: ImportedPostmanCollection) -> Self {
        Self {
            source: ImportSource::Postman,
            source_name: imported.source.name,
            collection: imported.collection,
            warning_count: imported.diagnostics.len(),
            selected_environment: imported.collection_variables_environment,
        }
    }
}

impl From<ImportedYaakWorkspace> for CollectionImport {
    fn from(imported: ImportedYaakWorkspace) -> Self {
        Self {
            source: ImportSource::Yaak,
            source_name: imported.workspace.name,
            collection: imported.collection,
            warning_count: imported.diagnostics.len(),
            selected_environment: imported.default_environment,
        }
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
        let dirty = self.dirty_keys();
        if !dirty.is_empty() || self.documentation_blocks_close_or_open() {
            self.prompt_unsaved(dirty, PendingClose::Import(source), window, cx);
            return;
        }
        if self.has_pending_environment_work() {
            self.pending_close = Some(PendingClose::Import(source));
            self.start_next_environment_save(window, cx);
            return;
        }
        self.choose_import(source, window, cx);
    }

    pub(super) fn choose_import(
        &mut self,
        source: ImportSource,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let source_label = source.label();
        let receiver = cx.prompt_for_paths(source.picker_options());
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
                    .background_spawn(async move { inspect_import(source, path) })
                    .await;
                let _ = view.update_in(cx, |view, window, cx| match inspected {
                    Ok(InspectedImport::Ready(conversion)) => {
                        view.convert_import(conversion, false, window, cx);
                    }
                    Ok(InspectedImport::SelectYaakWorkspace {
                        preview,
                        workspaces,
                    }) => {
                        view.show_application_dialog(
                            ApplicationDialog::SelectYaakWorkspace {
                                preview,
                                workspaces,
                            },
                            window,
                            cx,
                        );
                    }
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
        let source_label = conversion.source().label();
        self.application_dialog = None;
        self.loading = true;
        cx.notify();
        let view = cx.weak_entity();
        window
            .spawn(cx, async move |cx| {
                let result = cx
                    .background_spawn(async move { conversion.convert(allow_partial) })
                    .await;
                let _ = view.update_in(cx, |view, window, cx| match result {
                    ImportConversionResult::Imported(import) => {
                        view.choose_import_destination(*import, window, cx);
                    }
                    ImportConversionResult::NeedsPartialConfirmation { conversion, detail } => {
                        view.loading = false;
                        view.show_application_dialog(
                            ApplicationDialog::ConfirmPartialImport { conversion, detail },
                            window,
                            cx,
                        );
                    }
                    ImportConversionResult::Failed(error) => {
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
        let source_label = import.source.label();
        let imported_kind = import.source.imported_kind();
        let filename = suggested_collection_filename(&import.source_name);
        let CollectionImport {
            collection,
            warning_count,
            selected_environment,
            ..
        } = import;
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
                            create_bundled_workspace_from_collection(&destination, &collection)
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
