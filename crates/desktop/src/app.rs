use std::{
    fs,
    path::{Path, PathBuf},
    thread,
};

use base_gpui::popover::{
    PopoverPopup, PopoverPortal, PopoverPositioner, PopoverRoot, PopoverTrigger,
};
use gpui::{
    App, AppContext as _, Bounds, Context, CursorStyle, FontWeight, InteractiveElement as _,
    IntoElement, MouseButton, MouseMoveEvent, ParentElement as _, PathPromptOptions, Render,
    ScrollHandle, ScrollStrategy, StatefulInteractiveElement as _, Styled as _, Task,
    TitlebarOptions, UniformListScrollHandle, Window, WindowBounds, WindowControlArea,
    WindowOptions, div, point, prelude::FluentBuilder as _, px, relative, size, uniform_list,
};
use probe_core::{
    AuthenticationKind, AuthenticationValue, Body, FileReference, FormField, Header, HttpRequest,
    MultipartPart, MultipartPartKind, MultipartValue, QueryParameter, RawBodyKind, RequestBody,
    RequestKey, Workspace, WorkspaceItemRef, resolve_environment, resolve_request,
};
use probe_http::{ExecutionOptions, HttpError, HttpResponse};
use probe_opencollection::{LoadedWorkspace, load_workspace};

use crate::{
    components,
    execution::{
        ExecutionState, ResponseState, execute_http_request, format_duration, format_size,
        workspace_base_directory,
    },
    request_editor::{
        BodyEditorKind, EditorSection, RequestEditorState, auth_label, auth_value, body_kind,
        raw_body_mut, set_auth_property, set_authentication,
    },
    response_viewer::{
        PreparedDocument, ResponseViewerState, ResponseViewerTab, prepare_document,
        pretty_json_body,
    },
    session::{SessionState, SessionStore},
    shell::{PaneLayout, ResizePane, ShellState},
    theme::Theme,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TreeRow {
    item: WorkspaceItemRef,
    depth: usize,
}

pub struct ProbeApp {
    loaded_workspace: Option<LoadedWorkspace>,
    workspace_path: Option<PathBuf>,
    shell: ShellState,
    loading: bool,
    message: Option<String>,
    session_store: Option<SessionStore>,
    session: SessionState,
    save_task: Option<Task<()>>,
    workspace_switcher_open: bool,
    visible_tree_rows: Vec<TreeRow>,
    request_editor: RequestEditorState,
    execution: ExecutionState,
    response_viewer: ResponseViewerState,
    response_scroll: UniformListScrollHandle,
    tab_bar_scroll: ScrollHandle,
    pending_tab_reveal: bool,
    #[cfg(test)]
    rendered_sidebar_rows: usize,
    #[cfg(test)]
    rendered_response_rows: usize,
    _caret_blink: Task<()>,
    _keystrokes: gpui::Subscription,
    _quit_subscription: gpui::Subscription,
}

impl ProbeApp {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        cx.observe_window_appearance(window, |_, window, _| window.refresh())
            .detach();
        let quit_subscription = cx.on_app_quit(|view, cx| {
            view.capture_session();
            let store = view.session_store.clone();
            let state = view.session.clone();
            let executor = cx.background_executor().clone();
            async move {
                if let Some(store) = store {
                    let _ = executor.spawn(async move { store.save(&state) }).await;
                }
            }
        });
        crate::caret::CaretBlink::show(cx);
        let keystrokes = cx.observe_keystrokes(|this, _, _, cx| {
            this.reset_caret_blink(cx);
        });

        Self {
            loaded_workspace: None,
            workspace_path: None,
            shell: ShellState::default(),
            loading: false,
            message: None,
            session_store: SessionStore::for_application(),
            session: SessionState::default(),
            save_task: None,
            workspace_switcher_open: false,
            visible_tree_rows: Vec::new(),
            request_editor: RequestEditorState::default(),
            execution: ExecutionState::default(),
            response_viewer: ResponseViewerState::default(),
            response_scroll: UniformListScrollHandle::new(),
            tab_bar_scroll: ScrollHandle::new(),
            pending_tab_reveal: false,
            #[cfg(test)]
            rendered_sidebar_rows: 0,
            #[cfg(test)]
            rendered_response_rows: 0,
            _caret_blink: Self::spawn_caret_blink(cx),
            _keystrokes: keystrokes,
            _quit_subscription: quit_subscription,
        }
    }

    fn spawn_caret_blink(cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(crate::caret::CARET_BLINK_INTERVAL)
                    .await;
                if this
                    .update(cx, |_, cx| {
                        crate::caret::CaretBlink::toggle(cx);
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
    }

    fn reset_caret_blink(&mut self, cx: &mut Context<Self>) {
        let was_visible = crate::caret::CaretBlink::is_visible(cx);
        crate::caret::CaretBlink::show(cx);
        self._caret_blink = Self::spawn_caret_blink(cx);
        if !was_visible {
            cx.notify();
        }
    }

    fn restore_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(store) = self.session_store.clone() else {
            return;
        };
        let view = cx.weak_entity();
        window
            .spawn(cx, async move |cx| {
                let result = cx.background_spawn(async move { store.load() }).await;
                let _ = view.update_in(cx, |view, window, cx| match result {
                    Ok(state) => {
                        let active_path = state.active_collection.clone();
                        view.session = state.clone();
                        if let Some(path) = active_path {
                            view.load_workspace_path(path, Some(state), window, cx);
                        }
                    }
                    Err(error) => {
                        view.message = Some(format!(
                            "Could not restore the previous desktop session: {error}"
                        ));
                        cx.notify();
                    }
                });
            })
            .detach();
    }

    fn choose_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
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
                            view.message = Some(format!("Could not open the file picker: {error}"));
                            cx.notify();
                        });
                        return;
                    }
                    Err(_) => return,
                };
                let Some(path) = paths.into_iter().next() else {
                    return;
                };
                let _ = view.update_in(cx, |view, window, cx| {
                    view.load_workspace_path(path, None, window, cx);
                });
            })
            .detach();
    }

    fn load_workspace_path(
        &mut self,
        path: PathBuf,
        restored_state: Option<SessionState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.loading = true;
        self.message = None;
        cx.notify();
        let view = cx.weak_entity();
        window
            .spawn(cx, async move |cx| {
                let load_path = path.clone();
                let result = cx
                    .background_spawn(async move {
                        let canonical_path = fs::canonicalize(&load_path).map_err(|error| {
                            format!("failed to locate {}: {error}", load_path.display())
                        })?;
                        let workspace =
                            load_workspace(&canonical_path).map_err(|error| error.to_string())?;
                        Ok::<_, String>((canonical_path, workspace))
                    })
                    .await;
                let _ = view.update_in(cx, |view, _, cx| {
                    view.loading = false;
                    match result {
                        Ok((canonical_path, workspace)) => {
                            view.set_workspace(canonical_path, workspace);
                            if let Some(state) = restored_state {
                                view.session = state;
                                view.restore_shell_state();
                            }
                            view.persist_session(cx);
                        }
                        Err(error) => {
                            if let Some(state) = restored_state {
                                view.session = state;
                                view.session.clear_active_collection();
                                view.persist_session(cx);
                                view.message = Some(format!(
                                    "Could not restore the previous collection. {error}"
                                ));
                            } else {
                                view.message = Some(error);
                            }
                        }
                    }
                    cx.notify();
                });
            })
            .detach();
    }

    fn set_workspace(&mut self, path: PathBuf, workspace: LoadedWorkspace) {
        self.execution.clear();
        self.response_viewer.clear();
        self.loaded_workspace = Some(workspace);
        self.workspace_path = Some(path);
        self.shell.reset_for_workspace();
        self.request_editor.clear();
        self.rebuild_visible_tree_rows();
        self.message = None;
    }

    fn restore_shell_state(&mut self) {
        let Some(loaded) = &self.loaded_workspace else {
            return;
        };
        let tabs: Vec<_> = self
            .session
            .open_tabs
            .iter()
            .filter_map(|selector| loaded.request_key(selector))
            .collect();
        let active_tab = self
            .session
            .active_tab
            .as_deref()
            .and_then(|selector| loaded.request_key(selector));
        let collapsed_folders: Vec<_> = self
            .session
            .collapsed_folders
            .iter()
            .filter_map(|selector| loaded.folder_key(selector))
            .collect();

        self.shell.restore_pane_sizes(
            self.session.sidebar_width,
            self.session.response_height,
            self.session.response_width,
        );
        self.shell
            .set_pane_layout(if self.session.horizontal_panes {
                PaneLayout::Horizontal
            } else {
                PaneLayout::Vertical
            });
        for key in tabs {
            self.shell.open_request(key);
        }
        if let Some(key) = active_tab {
            self.shell.open_request(key);
        }
        for key in collapsed_folders {
            self.shell.collapse_folder(key);
        }
        self.rebuild_visible_tree_rows();
        self.reveal_active_tab();
    }

    fn capture_session(&mut self) {
        self.session.sidebar_width = self.shell.sidebar_width;
        self.session.response_height = self.shell.response_height;
        self.session.response_width = self.shell.response_width;
        self.session.horizontal_panes = self.shell.pane_layout == PaneLayout::Horizontal;
        let (Some(path), Some(loaded)) = (&self.workspace_path, &self.loaded_workspace) else {
            self.session.clear_active_collection();
            return;
        };
        self.session.activate_collection(path.clone());
        self.session.open_tabs = self
            .shell
            .tabs()
            .iter()
            .filter_map(|key| loaded.request_selector(*key).map(str::to_owned))
            .collect();
        self.session.active_tab = self
            .shell
            .active_tab()
            .and_then(|key| loaded.request_selector(key))
            .map(str::to_owned);
        self.session.collapsed_folders = self
            .shell
            .collapsed_folders()
            .filter_map(|key| loaded.folder_selector(key).map(str::to_owned))
            .collect();
        self.session.collapsed_folders.sort();
    }

    fn persist_session(&mut self, cx: &mut Context<Self>) {
        self.capture_session();
        let Some(store) = self.session_store.clone() else {
            return;
        };
        let state = self.session.clone();
        self.save_task = Some(cx.spawn(async move |view, cx| {
            let result = cx.background_spawn(async move { store.save(&state) }).await;
            if let Err(error) = result {
                let _ = view.update(cx, |view, cx| {
                    view.message = Some(format!("Could not save desktop session state: {error}"));
                    cx.notify();
                });
            }
        }));
    }

    fn close_workspace(&mut self, cx: &mut Context<Self>) {
        self.execution.clear();
        self.response_viewer.clear();
        self.loaded_workspace = None;
        self.workspace_path = None;
        self.shell.reset_for_workspace();
        self.request_editor.clear();
        self.visible_tree_rows.clear();
        self.session.clear_active_collection();
        self.persist_session(cx);
        cx.notify();
    }

    fn select_request(&mut self, key: RequestKey, cx: &mut Context<Self>) {
        if self
            .loaded_workspace
            .as_ref()
            .is_some_and(|loaded| loaded.workspace().request(key).is_some())
        {
            self.shell.open_request(key);
            self.reveal_active_tab();
            self.persist_session(cx);
            cx.notify();
        }
    }

    fn close_tab(&mut self, key: RequestKey, cx: &mut Context<Self>) {
        self.shell.close_tab(key);
        self.reveal_active_tab();
        self.persist_session(cx);
        cx.notify();
    }

    fn reveal_active_tab(&mut self) {
        self.scroll_active_tab_into_view();
        self.pending_tab_reveal = true;
    }

    fn scroll_active_tab_into_view(&self) {
        let Some(active) = self.shell.active_tab() else {
            return;
        };
        let Some(index) = self.shell.tabs().iter().position(|tab| *tab == active) else {
            return;
        };
        self.tab_bar_scroll.scroll_to_item(index);
    }

    fn send_request(&mut self, key: RequestKey, cx: &mut Context<Self>) {
        let Some(request) = self
            .loaded_workspace
            .as_ref()
            .and_then(|loaded| loaded.workspace().request(key))
            .cloned()
        else {
            return;
        };
        let selected_environment = self
            .request_editor
            .selected_environment()
            .map(str::to_owned);
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
        };
        let (cancellation_sender, cancellation_receiver) = tokio::sync::oneshot::channel();
        let (result_sender, result_receiver) = tokio::sync::oneshot::channel();
        let generation = self.execution.begin(key, cancellation_sender);
        let spawn_result = thread::Builder::new()
            .name("probe-http-request".to_owned())
            .spawn(move || {
                let result = execute_http_request(request, options, cancellation_receiver);
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
            let result = result_receiver.await.unwrap_or_else(|_| {
                Err(HttpError::Transport(
                    "HTTP execution ended without a result".to_owned(),
                ))
            });
            let _ = view.update(cx, |view, cx| {
                view.complete_execution(key, generation, result, cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn complete_execution(
        &mut self,
        key: RequestKey,
        generation: u64,
        result: Result<HttpResponse, HttpError>,
        cx: &mut Context<Self>,
    ) {
        self.execution.finish(key, generation, result);
        self.refresh_response_document(key, cx);
    }

    fn refresh_response_document(&mut self, key: RequestKey, cx: &mut Context<Self>) {
        let Some(ResponseState::Complete(response)) = self.execution.response(key) else {
            self.response_viewer.remove(key);
            return;
        };
        let generation = self.response_viewer.allocate_generation();
        let (document, pending) = prepare_document(response, generation);
        let body = pending.then(|| response.body.clone());
        self.response_viewer.insert(key, document);
        let Some(body) = body else {
            return;
        };
        cx.spawn(async move |view, cx| {
            let pretty = cx
                .background_spawn(async move { pretty_json_body(&body) })
                .await;
            let _ = view.update(cx, |view, cx| {
                view.response_viewer.apply_pretty(key, generation, pretty);
                cx.notify();
            });
        })
        .detach();
    }

    fn cancel_request(&mut self, key: RequestKey, cx: &mut Context<Self>) {
        self.execution.cancel(key);
        self.response_viewer.remove(key);
        cx.notify();
    }

    fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.shell.resizing {
            Some(ResizePane::Sidebar) => self.shell.resize_sidebar(event.position.x.into()),
            Some(ResizePane::Response) => match self.shell.pane_layout {
                PaneLayout::Vertical => self.shell.resize_response(
                    window.window_bounds().get_bounds().size.height.into(),
                    event.position.y.into(),
                ),
                PaneLayout::Horizontal => self.shell.resize_response_width(
                    window.window_bounds().get_bounds().size.width.into(),
                    event.position.x.into(),
                ),
            },
            None => return,
        }
        cx.notify();
    }

    fn finish_resize(&mut self, cx: &mut Context<Self>) {
        if self.shell.resizing.take().is_none() {
            return;
        }
        self.persist_session(cx);
        cx.notify();
    }

    fn rebuild_visible_tree_rows(&mut self) {
        let Some(loaded) = &self.loaded_workspace else {
            self.visible_tree_rows.clear();
            return;
        };
        let workspace = loaded.workspace();
        let mut rows = Vec::with_capacity(workspace.request_count());
        flatten_visible_tree_rows(workspace, workspace.root_items(), 0, &self.shell, &mut rows);
        self.visible_tree_rows = rows;
    }

    fn render_tree_row(
        &self,
        row: TreeRow,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let Some(loaded) = &self.loaded_workspace else {
            return div().into_any_element();
        };
        let TreeRow { item, depth } = row;
        match item {
            WorkspaceItemRef::Request(key) => {
                let Some(request) = loaded.workspace().request(key) else {
                    return div().into_any_element();
                };
                let label = request
                    .metadata
                    .name
                    .as_deref()
                    .unwrap_or("Untitled request");
                let method = request.method.as_deref().unwrap_or("HTTP").to_uppercase();
                let selected = self.shell.active_tab() == Some(key);
                let view = cx.weak_entity();
                div()
                    .id(("request-tree-item", key.slot()))
                    .w_full()
                    .h(px(30.0))
                    .pl(px(12.0 + depth as f32 * 16.0))
                    .pr(px(theme.metrics.spacing_2))
                    .flex()
                    .items_center()
                    .gap(px(theme.metrics.spacing_2))
                    .overflow_hidden()
                    .rounded(px(theme.metrics.radius_small))
                    .when(selected, |row| {
                        row.bg(theme.colors.selection.active_background)
                            .text_color(theme.colors.selection.active_foreground)
                    })
                    .when(!selected, |row| {
                        row.hover(move |row| row.bg(theme.colors.surfaces.raised))
                    })
                    .cursor_pointer()
                    .on_click(move |_, _, cx| {
                        let _ = view.update(cx, |view, cx| view.select_request(key, cx));
                    })
                    .child(
                        div()
                            .w(px(42.0))
                            .flex_none()
                            .truncate()
                            .text_size(px(10.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(if selected {
                                theme.colors.selection.active_foreground
                            } else {
                                theme.method_color(&method)
                            })
                            .child(method),
                    )
                    .child(
                        components::truncated_label(label.to_owned())
                            .flex_1()
                            .when(selected, |label| {
                                label.debug_selector(|| "request-tree-label".into())
                            }),
                    )
                    .into_any_element()
            }
            WorkspaceItemRef::Folder(key) => {
                let Some(folder) = loaded.workspace().folder(key) else {
                    return div().into_any_element();
                };
                let expanded = self.shell.folder_is_expanded(key);
                let label = folder.metadata.name.as_deref().unwrap_or("Untitled folder");
                let view = cx.weak_entity();
                div()
                    .id(("folder-tree-item", key.slot()))
                    .w_full()
                    .h(px(30.0))
                    .pl(px(8.0 + depth as f32 * 16.0))
                    .pr(px(theme.metrics.spacing_2))
                    .flex()
                    .items_center()
                    .gap(px(theme.metrics.spacing_2))
                    .overflow_hidden()
                    .rounded(px(theme.metrics.radius_small))
                    .cursor_pointer()
                    .hover(move |row| row.bg(theme.colors.surfaces.raised))
                    .on_click(move |_, _, cx| {
                        let _ = view.update(cx, |view, cx| {
                            view.shell.toggle_folder(key);
                            view.rebuild_visible_tree_rows();
                            view.persist_session(cx);
                            cx.notify();
                        });
                    })
                    .child(div().flex_none().child(if expanded { "▾" } else { "▸" }))
                    .child(
                        components::truncated_label(label.to_owned())
                            .flex_1()
                            .font_weight(FontWeight::SEMIBOLD),
                    )
                    .into_any_element()
            }
        }
    }

    fn render_sidebar(&self, theme: Theme, cx: &mut Context<Self>) -> gpui::Div {
        let tree = if self.loaded_workspace.is_some() {
            let row_count = self.visible_tree_rows.len();
            uniform_list("request-tree", row_count, {
                cx.processor(move |view, range: std::ops::Range<usize>, _, cx| {
                    #[cfg(test)]
                    {
                        view.rendered_sidebar_rows = range.len();
                    }
                    range
                        .filter_map(|index| view.visible_tree_rows.get(index).copied())
                        .map(|row| view.render_tree_row(row, theme, cx))
                        .collect::<Vec<_>>()
                })
            })
            .flex_1()
            .min_h(px(0.0))
            .px(px(theme.metrics.spacing_2))
            .into_any_element()
        } else {
            let mut tree = div()
                .id("request-tree")
                .flex_1()
                .overflow_y_scroll()
                .p(px(theme.metrics.spacing_2))
                .flex()
                .flex_col()
                .child(
                    div()
                        .p(px(theme.metrics.spacing_3))
                        .text_color(theme.colors.text.muted)
                        .child("Open a collection to browse its requests."),
                );
            if !self.session.recent_collections.is_empty() {
                tree = tree.child(
                    div()
                        .px(px(theme.metrics.spacing_3))
                        .pb(px(theme.metrics.spacing_2))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Recent Collections"),
                );
                for (index, path) in self.session.recent_collections.iter().enumerate() {
                    let open_path = path.clone();
                    let label = path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("Collection")
                        .to_owned();
                    let detail = path.display().to_string();
                    let view = cx.weak_entity();
                    let row = div()
                        .id(("recent-collection", index))
                        .mx(px(theme.metrics.spacing_2))
                        .p(px(theme.metrics.spacing_2))
                        .flex()
                        .flex_col()
                        .gap(px(theme.metrics.spacing_1))
                        .overflow_hidden()
                        .rounded(px(theme.metrics.radius_small))
                        .cursor_pointer()
                        .hover(move |row| row.bg(theme.colors.surfaces.raised))
                        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                            cx.stop_propagation();
                            let path = open_path.clone();
                            let _ = view.update(cx, |view, cx| {
                                if !view.loading {
                                    view.load_workspace_path(path, None, window, cx);
                                }
                            });
                        })
                        .child(components::truncated_label(label))
                        .child(
                            components::truncated_label(detail)
                                .text_size(px(theme.typography.caption_size))
                                .text_color(theme.colors.text.muted),
                        );
                    #[cfg(test)]
                    let row = row.debug_selector(move || format!("recent-collection-{index}"));
                    tree = tree.child(row);
                }
            }
            tree.into_any_element()
        };

        div()
            .w(px(self.shell.sidebar_width))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .bg(theme.colors.surfaces.sidebar)
            .child(
                div()
                    .h(px(42.0))
                    .px(px(theme.metrics.spacing_3))
                    .flex()
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(theme.colors.borders.subtle)
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("Collection"))
                    .child(
                        div()
                            .text_size(px(theme.typography.caption_size))
                            .text_color(theme.colors.text.muted)
                            .child(self.request_count_label()),
                    ),
            )
            .child(tree)
    }

    fn render_tabs(&self, theme: Theme, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let mut tab_strip = div()
            .id("request-tabs-scroll")
            .flex_1()
            .min_w(px(0.0))
            .h_full()
            .flex()
            .items_end()
            .overflow_x_scroll()
            .track_scroll(&self.tab_bar_scroll);
        let Some(loaded) = &self.loaded_workspace else {
            return div()
                .id("request-tabs")
                .h(px(38.0))
                .w_full()
                .bg(theme.colors.surfaces.raised)
                .border_b_1()
                .border_color(theme.colors.borders.subtle);
        };
        for key in self.shell.tabs() {
            let Some(request) = loaded.workspace().request(*key) else {
                continue;
            };
            let active = self.shell.active_tab() == Some(*key);
            let label = request
                .metadata
                .name
                .as_deref()
                .unwrap_or("Untitled request");
            let select_view = cx.weak_entity();
            let close_view = cx.weak_entity();
            let tab_key = *key;
            tab_strip = tab_strip.child(
                div()
                    .id(("request-tab", key.slot()))
                    .h_full()
                    .min_w(px(120.0))
                    .max_w(px(220.0))
                    .px(px(theme.metrics.spacing_3))
                    .flex()
                    .items_center()
                    .gap(px(theme.metrics.spacing_2))
                    .overflow_hidden()
                    .border_r_1()
                    .border_color(theme.colors.borders.subtle)
                    .when(active, |tab| tab.bg(theme.colors.surfaces.editor))
                    .when(!active, |tab| {
                        tab.text_color(theme.colors.text.secondary)
                            .hover(move |tab| tab.bg(theme.colors.surfaces.window))
                    })
                    .cursor_pointer()
                    .on_click(move |_, _, cx| {
                        let _ = select_view.update(cx, |view, cx| view.select_request(tab_key, cx));
                    })
                    .child(
                        components::truncated_label(label.to_owned())
                            .flex_1()
                            .when(active, |label| {
                                label.debug_selector(|| "request-tab-label".into())
                            }),
                    )
                    .child(
                        div()
                            .id(("close-tab", key.slot()))
                            .flex_none()
                            .px(px(4.0))
                            .rounded(px(theme.metrics.radius_small))
                            .hover(move |close| close.bg(theme.colors.actions.disabled))
                            .child("×")
                            .on_click(move |_, _, cx| {
                                cx.stop_propagation();
                                let _ =
                                    close_view.update(cx, |view, cx| view.close_tab(tab_key, cx));
                            }),
                    ),
            );
        }

        let mut tabs = div()
            .id("request-tabs")
            .h(px(38.0))
            .w_full()
            .flex()
            .items_center()
            .bg(theme.colors.surfaces.raised)
            .border_b_1()
            .border_color(theme.colors.borders.subtle)
            .child(tab_strip);
        if self.shell.active_tab().is_some() {
            let selected = self
                .request_editor
                .selected_environment()
                .unwrap_or("")
                .to_owned();
            let mut options = vec![(String::new(), "No environment".to_owned())];
            options.extend(
                loaded
                    .workspace()
                    .environments()
                    .iter()
                    .map(|environment| (environment.name.clone(), environment.name.clone())),
            );
            let environment_view = cx.weak_entity();
            tabs = tabs.child(div().flex_none().px(px(theme.metrics.spacing_2)).child(
                components::dropdown(
                    theme,
                    "request-environment",
                    "Request environment",
                    Some(selected),
                    options,
                    170.0,
                    move |value, _, cx| {
                        let value = value.cloned().unwrap_or_default();
                        let _ = environment_view.update(cx, |view, cx| {
                            view.request_editor
                                .select_environment((!value.is_empty()).then_some(value));
                            cx.notify();
                        });
                    },
                ),
            ));
        }
        tabs
    }

    fn edit_request(
        &mut self,
        key: RequestKey,
        edit: impl FnOnce(&mut HttpRequest),
        cx: &mut Context<Self>,
    ) {
        let Some(request) = self
            .loaded_workspace
            .as_mut()
            .and_then(|loaded| loaded.request_mut(key))
        else {
            return;
        };
        edit(request);
        cx.notify();
    }

    fn change_body_kind(&mut self, key: RequestKey, kind: BodyEditorKind, cx: &mut Context<Self>) {
        let Some(request) = self
            .loaded_workspace
            .as_mut()
            .and_then(|loaded| loaded.request_mut(key))
        else {
            return;
        };
        self.request_editor.switch_body_kind(key, request, kind);
        cx.notify();
    }

    fn render_request_editor(&self, theme: Theme, cx: &mut Context<Self>) -> gpui::Div {
        let Some(key) = self.shell.active_tab() else {
            return div()
                .flex_1()
                .min_w(px(0.0))
                .min_h(px(0.0))
                .flex()
                .items_center()
                .justify_center()
                .bg(theme.colors.surfaces.editor)
                .text_color(theme.colors.text.muted)
                .child("Select a request from the collection sidebar.");
        };
        let Some(request) = self.active_request().cloned() else {
            return div().flex_1();
        };
        let method = request.method.as_deref().unwrap_or("GET").to_uppercase();
        let url = request.url.clone().unwrap_or_default();
        let url_view = cx.weak_entity();
        let execution_view = cx.weak_entity();
        let request_running = self
            .execution
            .response(key)
            .is_some_and(ResponseState::is_running);
        let mut section_tabs = div().flex().items_center().gap(px(theme.metrics.spacing_1));
        for (index, section) in EditorSection::ALL.into_iter().enumerate() {
            let section_view = cx.weak_entity();
            section_tabs = section_tabs.child(components::editor_button(
                theme,
                ("request-editor-section", index),
                format!(
                    "{}{}",
                    section.label(),
                    match section {
                        EditorSection::Query => format!("  {}", request.query_parameters.len()),
                        EditorSection::Headers => format!("  {}", request.headers.len()),
                        EditorSection::Body | EditorSection::Authentication => String::new(),
                    }
                ),
                self.request_editor.section == section,
                move |_, _, cx| {
                    let _ = section_view.update(cx, |view, cx| {
                        view.request_editor.section = section;
                        cx.notify();
                    });
                },
            ));
        }

        let section = match self.request_editor.section {
            EditorSection::Query => self.render_query_editor(key, &request, theme, cx),
            EditorSection::Headers => self.render_header_editor(key, &request, theme, cx),
            EditorSection::Body => self.render_body_editor(key, &request, theme, cx),
            EditorSection::Authentication => {
                self.render_authentication_editor(key, &request, theme, cx)
            }
        };

        div()
            .flex_1()
            .min_w(px(0.0))
            .min_h(px(120.0))
            .flex()
            .flex_col()
            .bg(theme.colors.surfaces.editor)
            .child(
                div()
                    .p(px(theme.metrics.spacing_3))
                    .pb(px(theme.metrics.spacing_2))
                    .flex()
                    .flex_col()
                    .gap(px(theme.metrics.spacing_2))
                    .child(
                        div()
                            .id("request-url-bar")
                            .debug_selector(|| "request-url-bar".into())
                            .h(px(40.0))
                            .w_full()
                            .flex()
                            .items_center()
                            .child(div().w(px(92.0)).mr(px(theme.metrics.spacing_2)).child(
                                components::dropdown_with_option_colors(
                                    theme,
                                    "request-method",
                                    "HTTP method",
                                    Some(method.clone()),
                                    request_method_options(theme, &method),
                                    92.0,
                                    {
                                        let method_view = cx.weak_entity();
                                        move |value, _, cx| {
                                            let Some(value) = value.cloned() else {
                                                return;
                                            };
                                            let _ = method_view.update(cx, |view, cx| {
                                                view.edit_request(
                                                    key,
                                                    |request| request.method = Some(value),
                                                    cx,
                                                );
                                            });
                                        }
                                    },
                                ),
                            ))
                            .child(div().flex_1().min_w(px(0.0)).child(
                                components::variable_text_input(
                                    theme,
                                    ("request-url", key.slot()),
                                    url.clone(),
                                    "https://api.example.com/path",
                                    self.variable_context(),
                                    move |value, _, input_cx| {
                                        let _ = url_view.update(input_cx, |view, cx| {
                                            view.edit_request(
                                                key,
                                                |request| request.url = Some(value.to_string()),
                                                cx,
                                            );
                                        });
                                    },
                                ),
                            ))
                            .child(div().ml(px(theme.metrics.spacing_2)).flex_none().child(
                                components::primary_button(
                                    theme,
                                    "request-execution",
                                    if request_running { "Cancel" } else { "Send" },
                                    move |_, _, cx| {
                                        let _ = execution_view.update(cx, |view, cx| {
                                            if view
                                                .execution
                                                .response(key)
                                                .is_some_and(ResponseState::is_running)
                                            {
                                                view.cancel_request(key, cx);
                                            } else {
                                                view.send_request(key, cx);
                                            }
                                        });
                                    },
                                ),
                            )),
                    )
                    .child(section_tabs),
            )
            .child(
                div()
                    .id("request-editor-section-content")
                    .flex_1()
                    .min_h(px(0.0))
                    .px(px(theme.metrics.spacing_3))
                    .pb(px(theme.metrics.spacing_3))
                    .when(
                        self.request_editor.section != EditorSection::Body,
                        |content| content.overflow_y_scroll(),
                    )
                    .child(section),
            )
    }

    fn render_query_editor(
        &self,
        key: RequestKey,
        request: &HttpRequest,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let mut rows = div().flex().flex_col().gap(px(theme.metrics.spacing_2));
        for (index, parameter) in request.query_parameters.iter().enumerate() {
            let name_view = cx.weak_entity();
            let value_view = cx.weak_entity();
            let enabled_view = cx.weak_entity();
            let remove_view = cx.weak_entity();
            rows =
                rows.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(theme.metrics.spacing_2))
                        .child(div().flex_1().min_w(px(0.0)).child(
                            components::variable_text_input(
                                theme,
                                ("query-name", index),
                                parameter.name.clone(),
                                "Parameter",
                                self.variable_context(),
                                move |value, _, input_cx| {
                                    let _ = name_view.update(input_cx, |view, cx| {
                                        view.edit_request(
                                            key,
                                            |request| {
                                                if let Some(parameter) =
                                                    request.query_parameters.get_mut(index)
                                                {
                                                    parameter.name = value.to_string();
                                                }
                                            },
                                            cx,
                                        );
                                    });
                                },
                            ),
                        ))
                        .child(div().flex_1().min_w(px(0.0)).child(
                            components::variable_text_input(
                                theme,
                                ("query-value", index),
                                parameter.value.clone(),
                                "Value",
                                self.variable_context(),
                                move |value, _, input_cx| {
                                    let _ = value_view.update(input_cx, |view, cx| {
                                        view.edit_request(
                                            key,
                                            |request| {
                                                if let Some(parameter) =
                                                    request.query_parameters.get_mut(index)
                                                {
                                                    parameter.value = value.to_string();
                                                }
                                            },
                                            cx,
                                        );
                                    });
                                },
                            ),
                        ))
                        .child(components::switch(
                            theme,
                            ("query-enabled", index),
                            "Enable query parameter",
                            !parameter.disabled,
                            move |enabled, _, cx| {
                                let _ = enabled_view.update(cx, |view, cx| {
                                    view.edit_request(
                                        key,
                                        |request| {
                                            if let Some(parameter) =
                                                request.query_parameters.get_mut(index)
                                            {
                                                parameter.disabled = !enabled;
                                            }
                                        },
                                        cx,
                                    );
                                });
                            },
                        ))
                        .child(components::remove_row_button(
                            theme,
                            ("remove-query", index),
                            "Remove query parameter",
                            move |_, _, cx| {
                                let _ = remove_view.update(cx, |view, cx| {
                                    view.edit_request(
                                        key,
                                        |request| {
                                            if index < request.query_parameters.len() {
                                                request.query_parameters.remove(index);
                                            }
                                        },
                                        cx,
                                    );
                                });
                            },
                        )),
                );
        }
        let add_view = cx.weak_entity();
        rows.child(components::editor_button(
            theme,
            "add-query-parameter",
            "+ Add parameter",
            false,
            move |_, _, cx| {
                let _ = add_view.update(cx, |view, cx| {
                    view.edit_request(
                        key,
                        |request| {
                            request.query_parameters.push(QueryParameter {
                                name: String::new(),
                                value: String::new(),
                                disabled: false,
                            })
                        },
                        cx,
                    );
                });
            },
        ))
        .into_any_element()
    }

    fn render_header_editor(
        &self,
        key: RequestKey,
        request: &HttpRequest,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let mut rows = div().flex().flex_col().gap(px(theme.metrics.spacing_2));
        for (index, header) in request.headers.iter().enumerate() {
            let name_view = cx.weak_entity();
            let value_view = cx.weak_entity();
            let enabled_view = cx.weak_entity();
            let remove_view = cx.weak_entity();
            rows =
                rows.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(theme.metrics.spacing_2))
                        .child(div().flex_1().min_w(px(0.0)).child(
                            components::variable_text_input(
                                theme,
                                ("header-name", index),
                                header.name.clone(),
                                "Header",
                                self.variable_context(),
                                move |value, _, input_cx| {
                                    let _ = name_view.update(input_cx, |view, cx| {
                                        view.edit_request(
                                            key,
                                            |request| {
                                                if let Some(header) = request.headers.get_mut(index)
                                                {
                                                    header.name = value.to_string();
                                                }
                                            },
                                            cx,
                                        );
                                    });
                                },
                            ),
                        ))
                        .child(div().flex_1().min_w(px(0.0)).child(
                            components::variable_text_input(
                                theme,
                                ("header-value", index),
                                header.value.clone(),
                                "Value",
                                self.variable_context(),
                                move |value, _, input_cx| {
                                    let _ = value_view.update(input_cx, |view, cx| {
                                        view.edit_request(
                                            key,
                                            |request| {
                                                if let Some(header) = request.headers.get_mut(index)
                                                {
                                                    header.value = value.to_string();
                                                }
                                            },
                                            cx,
                                        );
                                    });
                                },
                            ),
                        ))
                        .child(components::switch(
                            theme,
                            ("header-enabled", index),
                            "Enable header",
                            !header.disabled,
                            move |enabled, _, cx| {
                                let _ = enabled_view.update(cx, |view, cx| {
                                    view.edit_request(
                                        key,
                                        |request| {
                                            if let Some(header) = request.headers.get_mut(index) {
                                                header.disabled = !enabled;
                                            }
                                        },
                                        cx,
                                    );
                                });
                            },
                        ))
                        .child(components::remove_row_button(
                            theme,
                            ("remove-header", index),
                            "Remove header",
                            move |_, _, cx| {
                                let _ = remove_view.update(cx, |view, cx| {
                                    view.edit_request(
                                        key,
                                        |request| {
                                            if index < request.headers.len() {
                                                request.headers.remove(index);
                                            }
                                        },
                                        cx,
                                    );
                                });
                            },
                        )),
                );
        }
        let add_view = cx.weak_entity();
        rows.child(components::editor_button(
            theme,
            "add-header",
            "+ Add header",
            false,
            move |_, _, cx| {
                let _ = add_view.update(cx, |view, cx| {
                    view.edit_request(
                        key,
                        |request| {
                            request.headers.push(Header {
                                name: String::new(),
                                value: String::new(),
                                disabled: false,
                            })
                        },
                        cx,
                    );
                });
            },
        ))
        .into_any_element()
    }

    fn render_body_editor(
        &self,
        key: RequestKey,
        request: &HttpRequest,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let active_kind = body_kind(request);
        let choices = [
            ("None", BodyEditorKind::None),
            ("JSON", BodyEditorKind::Json),
            ("Text", BodyEditorKind::Text),
            ("XML", BodyEditorKind::Xml),
            ("SPARQL", BodyEditorKind::Sparql),
            ("Form", BodyEditorKind::Form),
            ("Multipart", BodyEditorKind::Multipart),
            ("File", BodyEditorKind::File),
        ];
        let mut kind_buttons = div().flex().flex_wrap().gap(px(theme.metrics.spacing_1));
        for (index, (label, kind)) in choices.into_iter().enumerate() {
            let kind_view = cx.weak_entity();
            kind_buttons = kind_buttons.child(components::editor_button(
                theme,
                ("body-kind", index),
                label,
                active_kind == label,
                move |_, _, cx| {
                    let _ = kind_view.update(cx, |view, cx| {
                        view.change_body_kind(key, kind, cx);
                    });
                },
            ));
        }

        let mut editor = div()
            .size_full()
            .min_h(px(0.0))
            .flex()
            .flex_col()
            .gap(px(theme.metrics.spacing_3))
            .child(kind_buttons);
        match request.body.as_ref() {
            Some(RequestBody::Single(Body::Raw(raw))) => {
                let body_view = cx.weak_entity();
                editor = editor
                    .child(
                        div()
                            .text_size(px(theme.typography.caption_size))
                            .text_color(theme.colors.text.muted)
                            .child("Request body"),
                    )
                    .child(
                        div()
                            .id("request-body-editor")
                            .debug_selector(|| "request-body-editor".into())
                            .flex_1()
                            .min_h(px(0.0))
                            .child(components::body_text_input(
                                theme,
                                ("request-body", key.slot()),
                                raw.data.clone(),
                                self.variable_context(),
                                raw.kind == RawBodyKind::Json,
                                move |value, _, input_cx| {
                                    let _ = body_view.update(input_cx, |view, cx| {
                                        view.edit_request(
                                            key,
                                            |request| {
                                                if let Some(data) = raw_body_mut(request) {
                                                    *data = value.to_string();
                                                }
                                            },
                                            cx,
                                        );
                                    });
                                },
                            )),
                    );
            }
            Some(RequestBody::Single(Body::FormUrlEncoded(fields))) => {
                editor = editor.child(self.render_form_body_editor(key, fields, theme, cx));
            }
            Some(RequestBody::Single(Body::Multipart(parts))) => {
                editor = editor.child(self.render_multipart_body_editor(key, parts, theme, cx));
            }
            Some(RequestBody::Single(Body::File(files))) => {
                editor = editor.child(self.render_file_body_editor(key, files, theme, cx));
            }
            Some(_) => {
                editor = editor.child(
                    div()
                        .p(px(theme.metrics.spacing_3))
                        .rounded(px(theme.metrics.radius_small))
                        .bg(theme.colors.surfaces.window)
                        .text_color(theme.colors.text.secondary)
                        .child(format!(
                            "This request uses a {active_kind} body. Choose a raw body type to replace it."
                        )),
                );
            }
            None => {
                editor = editor.child(
                    div()
                        .text_color(theme.colors.text.muted)
                        .child("This request has no body."),
                );
            }
        }
        editor.into_any_element()
    }

    fn render_form_body_editor(
        &self,
        key: RequestKey,
        fields: &[FormField],
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let mut rows = div().flex().flex_col().gap(px(theme.metrics.spacing_2));
        for (index, field) in fields.iter().enumerate() {
            let name_view = cx.weak_entity();
            let value_view = cx.weak_entity();
            let enabled_view = cx.weak_entity();
            let remove_view = cx.weak_entity();
            rows =
                rows.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(theme.metrics.spacing_2))
                        .child(div().flex_1().min_w(px(0.0)).child(
                            components::variable_text_input(
                                theme,
                                ("form-field-name", index),
                                field.name.clone(),
                                "Field",
                                self.variable_context(),
                                move |value, _, input_cx| {
                                    let _ = name_view.update(input_cx, |view, cx| {
                                        view.edit_request(
                                            key,
                                            |request| {
                                                if let Some(RequestBody::Single(
                                                    Body::FormUrlEncoded(fields),
                                                )) = request.body.as_mut()
                                                    && let Some(field) = fields.get_mut(index)
                                                {
                                                    field.name = value.to_string();
                                                }
                                            },
                                            cx,
                                        );
                                    });
                                },
                            ),
                        ))
                        .child(div().flex_1().min_w(px(0.0)).child(
                            components::variable_text_input(
                                theme,
                                ("form-field-value", index),
                                field.value.clone(),
                                "Value",
                                self.variable_context(),
                                move |value, _, input_cx| {
                                    let _ = value_view.update(input_cx, |view, cx| {
                                        view.edit_request(
                                            key,
                                            |request| {
                                                if let Some(RequestBody::Single(
                                                    Body::FormUrlEncoded(fields),
                                                )) = request.body.as_mut()
                                                    && let Some(field) = fields.get_mut(index)
                                                {
                                                    field.value = value.to_string();
                                                }
                                            },
                                            cx,
                                        );
                                    });
                                },
                            ),
                        ))
                        .child(components::switch(
                            theme,
                            ("form-field-enabled", index),
                            "Enable form field",
                            !field.disabled,
                            move |enabled, _, cx| {
                                let _ = enabled_view.update(cx, |view, cx| {
                                    view.edit_request(
                                        key,
                                        |request| {
                                            if let Some(RequestBody::Single(Body::FormUrlEncoded(
                                                fields,
                                            ))) = request.body.as_mut()
                                                && let Some(field) = fields.get_mut(index)
                                            {
                                                field.disabled = !enabled;
                                            }
                                        },
                                        cx,
                                    );
                                });
                            },
                        ))
                        .child(components::remove_row_button(
                            theme,
                            ("remove-form-field", index),
                            "Remove form field",
                            move |_, _, cx| {
                                let _ = remove_view.update(cx, |view, cx| {
                                    view.edit_request(
                                        key,
                                        |request| {
                                            if let Some(RequestBody::Single(Body::FormUrlEncoded(
                                                fields,
                                            ))) = request.body.as_mut()
                                                && index < fields.len()
                                            {
                                                fields.remove(index);
                                            }
                                        },
                                        cx,
                                    );
                                });
                            },
                        )),
                );
        }
        let add_view = cx.weak_entity();
        rows.child(components::editor_button(
            theme,
            "add-form-field",
            "+ Add field",
            false,
            move |_, _, cx| {
                let _ = add_view.update(cx, |view, cx| {
                    view.edit_request(
                        key,
                        |request| {
                            if let Some(RequestBody::Single(Body::FormUrlEncoded(fields))) =
                                request.body.as_mut()
                            {
                                fields.push(FormField {
                                    name: String::new(),
                                    value: String::new(),
                                    disabled: false,
                                });
                            }
                        },
                        cx,
                    );
                });
            },
        ))
        .into_any_element()
    }

    fn render_multipart_body_editor(
        &self,
        key: RequestKey,
        parts: &[MultipartPart],
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let mut rows = div().flex().flex_col().gap(px(theme.metrics.spacing_2));
        for (index, part) in parts.iter().enumerate() {
            let value = match &part.value {
                MultipartValue::Single(value) => value.clone(),
                MultipartValue::Multiple(values) => values.join(", "),
            };
            let name_view = cx.weak_entity();
            let value_view = cx.weak_entity();
            let kind_view = cx.weak_entity();
            let enabled_view = cx.weak_entity();
            let remove_view = cx.weak_entity();
            rows =
                rows.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(theme.metrics.spacing_2))
                        .child(components::editor_button(
                            theme,
                            ("multipart-kind", index),
                            if part.kind == MultipartPartKind::File {
                                "File"
                            } else {
                                "Text"
                            },
                            part.kind == MultipartPartKind::File,
                            move |_, _, cx| {
                                let _ = kind_view.update(cx, |view, cx| {
                                    view.edit_request(
                                    key,
                                    |request| {
                                        if let Some(RequestBody::Single(Body::Multipart(parts))) =
                                            request.body.as_mut()
                                            && let Some(part) = parts.get_mut(index)
                                        {
                                            part.kind = if part.kind == MultipartPartKind::Text {
                                                MultipartPartKind::File
                                            } else {
                                                MultipartPartKind::Text
                                            };
                                        }
                                    },
                                    cx,
                                );
                                });
                            },
                        ))
                        .child(div().flex_1().min_w(px(0.0)).child(
                            components::variable_text_input(
                                theme,
                                ("multipart-name", index),
                                part.name.clone(),
                                "Part",
                                self.variable_context(),
                                move |value, _, input_cx| {
                                    let _ = name_view.update(input_cx, |view, cx| {
                                        view.edit_request(
                                            key,
                                            |request| {
                                                if let Some(RequestBody::Single(Body::Multipart(
                                                    parts,
                                                ))) = request.body.as_mut()
                                                    && let Some(part) = parts.get_mut(index)
                                                {
                                                    part.name = value.to_string();
                                                }
                                            },
                                            cx,
                                        );
                                    });
                                },
                            ),
                        ))
                        .child(div().flex_1().min_w(px(0.0)).child(
                            components::variable_text_input(
                                theme,
                                ("multipart-value", index),
                                value,
                                "Value or file path",
                                self.variable_context(),
                                move |value, _, input_cx| {
                                    let _ = value_view.update(input_cx, |view, cx| {
                                        view.edit_request(
                                            key,
                                            |request| {
                                                if let Some(RequestBody::Single(Body::Multipart(
                                                    parts,
                                                ))) = request.body.as_mut()
                                                    && let Some(part) = parts.get_mut(index)
                                                {
                                                    part.value =
                                                        MultipartValue::Single(value.to_string());
                                                }
                                            },
                                            cx,
                                        );
                                    });
                                },
                            ),
                        ))
                        .child(components::switch(
                            theme,
                            ("multipart-enabled", index),
                            "Enable multipart part",
                            !part.disabled,
                            move |enabled, _, cx| {
                                let _ = enabled_view.update(cx, |view, cx| {
                                    view.edit_request(
                                    key,
                                    |request| {
                                        if let Some(RequestBody::Single(Body::Multipart(parts))) =
                                            request.body.as_mut()
                                            && let Some(part) = parts.get_mut(index)
                                        {
                                            part.disabled = !enabled;
                                        }
                                    },
                                    cx,
                                );
                                });
                            },
                        ))
                        .child(components::remove_row_button(
                            theme,
                            ("remove-multipart-part", index),
                            "Remove multipart part",
                            move |_, _, cx| {
                                let _ = remove_view.update(cx, |view, cx| {
                                    view.edit_request(
                                    key,
                                    |request| {
                                        if let Some(RequestBody::Single(Body::Multipart(parts))) =
                                            request.body.as_mut()
                                            && index < parts.len()
                                        {
                                            parts.remove(index);
                                        }
                                    },
                                    cx,
                                );
                                });
                            },
                        )),
                );
        }
        let add_view = cx.weak_entity();
        rows.child(components::editor_button(
            theme,
            "add-multipart-part",
            "+ Add part",
            false,
            move |_, _, cx| {
                let _ = add_view.update(cx, |view, cx| {
                    view.edit_request(
                        key,
                        |request| {
                            if let Some(RequestBody::Single(Body::Multipart(parts))) =
                                request.body.as_mut()
                            {
                                parts.push(MultipartPart {
                                    name: String::new(),
                                    kind: MultipartPartKind::Text,
                                    value: MultipartValue::Single(String::new()),
                                    content_type: None,
                                    disabled: false,
                                });
                            }
                        },
                        cx,
                    );
                });
            },
        ))
        .into_any_element()
    }

    fn render_file_body_editor(
        &self,
        key: RequestKey,
        files: &[FileReference],
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let mut rows = div().flex().flex_col().gap(px(theme.metrics.spacing_2));
        for (index, file) in files.iter().enumerate() {
            let path_view = cx.weak_entity();
            let type_view = cx.weak_entity();
            let selected_view = cx.weak_entity();
            let remove_view = cx.weak_entity();
            rows =
                rows.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(theme.metrics.spacing_2))
                        .child(div().flex_1().min_w(px(0.0)).child(
                            components::variable_text_input(
                                theme,
                                ("body-file-path", index),
                                file.file_path.clone(),
                                "File path",
                                self.variable_context(),
                                move |value, _, input_cx| {
                                    let _ = path_view.update(input_cx, |view, cx| {
                                        view.edit_request(
                                    key,
                                    |request| {
                                        if let Some(RequestBody::Single(Body::File(files))) =
                                            request.body.as_mut()
                                            && let Some(file) = files.get_mut(index)
                                        {
                                            file.file_path = value.to_string();
                                        }
                                    },
                                    cx,
                                );
                                    });
                                },
                            ),
                        ))
                        .child(div().flex_1().min_w(px(0.0)).child(
                            components::variable_text_input(
                                theme,
                                ("body-file-content-type", index),
                                file.content_type.clone(),
                                "Content type",
                                self.variable_context(),
                                move |value, _, input_cx| {
                                    let _ = type_view.update(input_cx, |view, cx| {
                                        view.edit_request(
                                    key,
                                    |request| {
                                        if let Some(RequestBody::Single(Body::File(files))) =
                                            request.body.as_mut()
                                            && let Some(file) = files.get_mut(index)
                                        {
                                            file.content_type = value.to_string();
                                        }
                                    },
                                    cx,
                                );
                                    });
                                },
                            ),
                        ))
                        .child(components::switch(
                            theme,
                            ("body-file-selected", index),
                            "Select body file",
                            file.selected,
                            move |selected, _, cx| {
                                let _ = selected_view.update(cx, |view, cx| {
                                    view.edit_request(
                                        key,
                                        |request| {
                                            if let Some(RequestBody::Single(Body::File(files))) =
                                                request.body.as_mut()
                                                && let Some(file) = files.get_mut(index)
                                            {
                                                file.selected = selected;
                                            }
                                        },
                                        cx,
                                    );
                                });
                            },
                        ))
                        .child(components::remove_row_button(
                            theme,
                            ("remove-body-file", index),
                            "Remove file",
                            move |_, _, cx| {
                                let _ = remove_view.update(cx, |view, cx| {
                                    view.edit_request(
                                        key,
                                        |request| {
                                            if let Some(RequestBody::Single(Body::File(files))) =
                                                request.body.as_mut()
                                                && index < files.len()
                                            {
                                                files.remove(index);
                                            }
                                        },
                                        cx,
                                    );
                                });
                            },
                        )),
                );
        }
        let add_view = cx.weak_entity();
        rows.child(components::editor_button(
            theme,
            "add-body-file",
            "+ Add file",
            false,
            move |_, _, cx| {
                let _ = add_view.update(cx, |view, cx| {
                    view.edit_request(
                        key,
                        |request| {
                            if let Some(RequestBody::Single(Body::File(files))) =
                                request.body.as_mut()
                            {
                                files.push(FileReference {
                                    file_path: String::new(),
                                    content_type: "application/octet-stream".to_owned(),
                                    selected: files.is_empty(),
                                });
                            }
                        },
                        cx,
                    );
                });
            },
        ))
        .into_any_element()
    }

    fn render_authentication_editor(
        &self,
        key: RequestKey,
        request: &HttpRequest,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let active = request
            .authentication
            .as_ref()
            .map(|auth| auth_label(&auth.kind));
        let choices = [
            ("None", None),
            ("Inherit", Some(AuthenticationKind::Inherit)),
            ("Basic", Some(AuthenticationKind::Basic)),
            ("Bearer", Some(AuthenticationKind::Bearer)),
            ("API Key", Some(AuthenticationKind::ApiKey)),
            ("OAuth 1", Some(AuthenticationKind::OAuth1)),
            ("OAuth 2", Some(AuthenticationKind::OAuth2)),
            ("AWS v4", Some(AuthenticationKind::AwsV4)),
            ("WSSE", Some(AuthenticationKind::Wsse)),
            ("Digest", Some(AuthenticationKind::Digest)),
            ("NTLM", Some(AuthenticationKind::Ntlm)),
        ];
        let mut kind_buttons = div().flex().flex_wrap().gap(px(theme.metrics.spacing_1));
        for (index, (label, kind)) in choices.into_iter().enumerate() {
            let kind_view = cx.weak_entity();
            kind_buttons = kind_buttons.child(components::editor_button(
                theme,
                ("authentication-kind", index),
                label,
                active == Some(label) || (active.is_none() && label == "None"),
                move |_, _, cx| {
                    let kind = kind.clone();
                    let _ = kind_view.update(cx, |view, cx| {
                        view.edit_request(key, |request| set_authentication(request, kind), cx);
                    });
                },
            ));
        }

        let mut editor = div()
            .flex()
            .flex_col()
            .gap(px(theme.metrics.spacing_3))
            .child(kind_buttons);
        if let Some(authentication) = &request.authentication {
            for (index, (property_name, value)) in authentication.properties.iter().enumerate() {
                let old_name = property_name.clone();
                let name_view = cx.weak_entity();
                let value_name = property_name.clone();
                let value_view = cx.weak_entity();
                let remove_name = property_name.clone();
                let remove_view = cx.weak_entity();
                editor = editor.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(theme.metrics.spacing_2))
                        .child(div().flex_1().min_w(px(0.0)).child(
                            components::variable_text_input(
                                theme,
                                ("authentication-property-name", index),
                                property_name.clone(),
                                "Property",
                                self.variable_context(),
                                move |value, _, input_cx| {
                                    let old_name = old_name.clone();
                                    let _ = name_view.update(input_cx, |view, cx| {
                                        view.edit_request(
                                            key,
                                            |request| {
                                                let Some(authentication) =
                                                    request.authentication.as_mut()
                                                else {
                                                    return;
                                                };
                                                if let Some(old_value) =
                                                    authentication.properties.remove(&old_name)
                                                {
                                                    authentication
                                                        .properties
                                                        .insert(value.to_string(), old_value);
                                                }
                                            },
                                            cx,
                                        );
                                    });
                                },
                            ),
                        ))
                        .child(div().flex_1().min_w(px(0.0)).child(
                            components::variable_text_input(
                                theme,
                                ("authentication-property-value", index),
                                auth_value(value),
                                "Value",
                                self.variable_context(),
                                move |value, _, input_cx| {
                                    let value_name = value_name.clone();
                                    let _ = value_view.update(input_cx, |view, cx| {
                                        view.edit_request(
                                            key,
                                            |request| {
                                                set_auth_property(
                                                    request,
                                                    value_name,
                                                    value.to_string(),
                                                )
                                            },
                                            cx,
                                        );
                                    });
                                },
                            ),
                        ))
                        .child(components::remove_row_button(
                            theme,
                            ("remove-authentication-property", index),
                            "Remove authentication property",
                            move |_, _, cx| {
                                let remove_name = remove_name.clone();
                                let _ = remove_view.update(cx, |view, cx| {
                                    view.edit_request(
                                        key,
                                        |request| {
                                            if let Some(authentication) =
                                                request.authentication.as_mut()
                                            {
                                                authentication.properties.remove(&remove_name);
                                            }
                                        },
                                        cx,
                                    );
                                });
                            },
                        )),
                );
            }
            let add_view = cx.weak_entity();
            editor = editor.child(components::editor_button(
                theme,
                "add-authentication-property",
                "+ Add property",
                false,
                move |_, _, cx| {
                    let _ = add_view.update(cx, |view, cx| {
                        view.edit_request(
                            key,
                            |request| {
                                let Some(authentication) = request.authentication.as_mut() else {
                                    return;
                                };
                                let mut index = authentication.properties.len() + 1;
                                let mut name = "property".to_owned();
                                while authentication.properties.contains_key(&name) {
                                    name = format!("property{index}");
                                    index += 1;
                                }
                                authentication
                                    .properties
                                    .insert(name, AuthenticationValue::String(String::new()));
                            },
                            cx,
                        );
                    });
                },
            ));
        } else {
            editor = editor.child(
                div()
                    .text_color(theme.colors.text.muted)
                    .child("This request does not use authentication."),
            );
        }
        editor.into_any_element()
    }

    fn render_response_panel(&self, theme: Theme, cx: &mut Context<Self>) -> gpui::Div {
        let active_key = self.shell.active_tab();
        let state = active_key.and_then(|key| self.execution.response(key));
        let (summary, content) = match state {
            Some(ResponseState::Running) => (
                "Sending…".to_owned(),
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(theme.colors.text.muted)
                    .child("Waiting for the server…")
                    .into_any_element(),
            ),
            Some(ResponseState::Cancelled) => (
                "Cancelled".to_owned(),
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(theme.colors.text.muted)
                    .child("Request cancelled.")
                    .into_any_element(),
            ),
            Some(ResponseState::Failed(error)) => (
                "Failed".to_owned(),
                div()
                    .id("response-error-scroll")
                    .flex_1()
                    .p(px(theme.metrics.spacing_3))
                    .overflow_y_scroll()
                    .text_color(theme.colors.status.error)
                    .child(error.clone())
                    .into_any_element(),
            ),
            Some(ResponseState::Complete(response)) => {
                let status = format!("{} {}", response.status, response.reason);
                let summary = format!(
                    "{}  •  {}  •  {}",
                    status.trim_end(),
                    format_duration(response.duration),
                    format_size(response.size)
                );
                let document = active_key.and_then(|key| self.response_viewer.document(key));
                (summary, self.render_response_document(theme, document, cx))
            }
            None => (
                String::new(),
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(theme.colors.text.muted)
                    .child("Send a request to see its response.")
                    .into_any_element(),
            ),
        };

        div()
            .when(self.shell.pane_layout == PaneLayout::Vertical, |panel| {
                panel.h(px(self.shell.response_height)).w_full()
            })
            .when(self.shell.pane_layout == PaneLayout::Horizontal, |panel| {
                panel.w(px(self.shell.response_width)).h_full()
            })
            .flex_none()
            .flex()
            .flex_col()
            .bg(theme.colors.surfaces.window)
            .child(
                div()
                    .h(px(38.0))
                    .px(px(theme.metrics.spacing_3))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(theme.metrics.spacing_2))
                    .border_b_1()
                    .border_color(theme.colors.borders.subtle)
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("Response"))
                    .child(
                        components::truncated_label(summary)
                            .id("response-status")
                            .debug_selector(|| "response-status".into())
                            .flex_1()
                            .text_size(px(theme.typography.caption_size))
                            .text_color(theme.colors.text.muted),
                    ),
            )
            .child(content)
    }

    fn render_response_document(
        &self,
        theme: Theme,
        document: Option<&PreparedDocument>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let Some(key) = self.shell.active_tab() else {
            return div().into_any_element();
        };
        let Some(document) = document else {
            return div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(theme.colors.text.muted)
                .child("Preparing response…")
                .into_any_element();
        };

        let mut tabs = div().flex().items_center().gap(px(theme.metrics.spacing_1));
        for (index, tab) in ResponseViewerTab::ALL.into_iter().enumerate() {
            let tab_view = cx.weak_entity();
            let selected = self.response_viewer.tab() == tab;
            tabs = tabs.child(
                div()
                    .debug_selector(move || {
                        format!("response-tab-{}", tab.label().to_ascii_lowercase())
                    })
                    .child(components::editor_button(
                        theme,
                        ("response-view-tab", index),
                        tab.label(),
                        selected,
                        move |_, _, cx| {
                            let _ = tab_view.update(cx, |view, cx| {
                                view.response_viewer.set_tab(tab);
                                view.response_scroll.scroll_to_item(0, ScrollStrategy::Top);
                                cx.notify();
                            });
                        },
                    )),
            );
        }

        let matches = self.response_viewer.matches(key);
        let match_count = matches.len();
        let search_label = if self.response_viewer.search().is_empty() {
            String::new()
        } else if match_count == 0 {
            "No matches".to_owned()
        } else {
            format!(
                "{} of {match_count}",
                self.response_viewer.active_match() + 1
            )
        };
        let search_view = cx.weak_entity();
        let enter_view = cx.weak_entity();
        let previous_view = cx.weak_entity();
        let next_view = cx.weak_entity();
        let search = div()
            .flex()
            .items_center()
            .gap(px(theme.metrics.spacing_1))
            .child(components::search_input(
                theme,
                "response-search-input",
                self.response_viewer.search().to_owned(),
                "Search",
                move |value, _, input_cx| {
                    let _ = search_view.update(input_cx, |view, cx| {
                        view.response_viewer.set_search(value.to_string());
                        if let Some(first) = view.response_viewer.matches(key).first() {
                            view.response_scroll
                                .scroll_to_item(first.row, ScrollStrategy::Center);
                        }
                        cx.notify();
                    });
                },
                move |_, _, input_cx| {
                    let _ = enter_view.update(input_cx, |view, cx| {
                        view.step_response_match(key, 1);
                        cx.notify();
                    });
                },
            ))
            .child(
                div()
                    .id("response-search-count")
                    .debug_selector(|| "response-search-count".into())
                    .text_size(px(theme.typography.caption_size))
                    .text_color(theme.colors.text.muted)
                    .child(search_label),
            )
            .child(components::editor_button(
                theme,
                "response-search-previous",
                "↑",
                false,
                move |_, _, cx| {
                    let _ = previous_view.update(cx, |view, cx| {
                        view.step_response_match(key, -1);
                        cx.notify();
                    });
                },
            ))
            .child(components::editor_button(
                theme,
                "response-search-next",
                "↓",
                false,
                move |_, _, cx| {
                    let _ = next_view.update(cx, |view, cx| {
                        view.step_response_match(key, 1);
                        cx.notify();
                    });
                },
            ));

        let mut banners = div()
            .px(px(theme.metrics.spacing_3))
            .pt(px(theme.metrics.spacing_2))
            .flex()
            .flex_col()
            .gap(px(theme.metrics.spacing_1));
        let mut has_banner = false;
        if document.truncated {
            has_banner = true;
            banners = banners.child(
                div()
                    .text_color(theme.colors.status.warning)
                    .text_size(px(theme.typography.caption_size))
                    .child("Response body is truncated at the in-memory limit."),
            );
        }
        if let Some(notice) = &document.pretty_notice
            && self.response_viewer.tab() != ResponseViewerTab::Headers
        {
            has_banner = true;
            banners = banners.child(
                div()
                    .text_color(theme.colors.text.muted)
                    .text_size(px(theme.typography.caption_size))
                    .child(notice.clone()),
            );
        }

        let list = match self.response_viewer.tab() {
            ResponseViewerTab::Headers => self.render_response_headers(theme, key, document, cx),
            ResponseViewerTab::Pretty | ResponseViewerTab::Raw => {
                self.render_response_body(theme, key, document, cx)
            }
        };

        div()
            .flex_1()
            .min_h(px(0.0))
            .flex()
            .flex_col()
            .child(
                div()
                    .px(px(theme.metrics.spacing_3))
                    .py(px(theme.metrics.spacing_1))
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap(px(theme.metrics.spacing_2))
                    .border_b_1()
                    .border_color(theme.colors.borders.subtle)
                    .child(tabs)
                    .child(search),
            )
            .when(has_banner, |panel| panel.child(banners))
            .child(list)
            .into_any_element()
    }

    fn step_response_match(&mut self, key: probe_core::RequestKey, delta: isize) {
        if let Some(index) = self.response_viewer.step_match(key, delta)
            && let Some(found) = self.response_viewer.matches(key).get(index)
        {
            self.response_scroll
                .scroll_to_item(found.row, ScrollStrategy::Center);
        }
    }

    fn render_response_body(
        &self,
        theme: Theme,
        key: probe_core::RequestKey,
        document: &PreparedDocument,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        if document.binary {
            return placeholder_message(theme, "Binary response body cannot be displayed as text.");
        }
        let lines = self.response_viewer.visible_lines(key);
        if lines.is_empty() {
            return placeholder_message(theme, "Empty response body.");
        }
        let matches = self.response_viewer.matches(key);
        let active_match = self.response_viewer.active_match();
        let view = cx.weak_entity();
        div()
            .id("response-body")
            .debug_selector(|| "response-body".into())
            .flex_1()
            .min_h(px(0.0))
            .px(px(theme.metrics.spacing_3))
            .pb(px(theme.metrics.spacing_2))
            .child(components::response_body_input(
                theme,
                "response-body-editor",
                lines,
                &matches,
                active_match,
                self.response_scroll.clone(),
                move |range, cx| {
                    #[cfg(test)]
                    {
                        let _ = view.update(cx, |this, _| {
                            this.rendered_response_rows = range.len();
                        });
                    }
                    #[cfg(not(test))]
                    {
                        let _ = (&view, range, cx);
                    }
                },
            ))
            .into_any_element()
    }

    fn render_response_headers(
        &self,
        theme: Theme,
        key: probe_core::RequestKey,
        document: &PreparedDocument,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        if document.headers.is_empty() {
            return placeholder_message(theme, "No response headers");
        }
        let matches = self.response_viewer.matches(key);
        let active_match = self.response_viewer.active_match();
        let view = cx.weak_entity();
        div()
            .id("response-headers")
            .debug_selector(|| "response-headers".into())
            .flex_1()
            .min_h(px(0.0))
            .px(px(theme.metrics.spacing_3))
            .pb(px(theme.metrics.spacing_2))
            .child(components::response_headers_input(
                theme,
                "response-headers-editor",
                &document.headers,
                &matches,
                active_match,
                self.response_scroll.clone(),
                move |range, cx| {
                    #[cfg(test)]
                    {
                        let _ = view.update(cx, |this, _| {
                            this.rendered_response_rows = range.len();
                        });
                    }
                    #[cfg(not(test))]
                    {
                        let _ = (&view, range, cx);
                    }
                },
            ))
            .into_any_element()
    }

    fn render_editor_response(&self, theme: Theme, cx: &mut Context<Self>) -> gpui::Div {
        let response_view = cx.weak_entity();
        let horizontal = self.shell.pane_layout == PaneLayout::Horizontal;
        let handle = div()
            .id("response-resize-handle")
            .flex_none()
            .bg(theme.colors.borders.subtle)
            .when(horizontal, |handle| {
                handle
                    .w(px(5.0))
                    .h_full()
                    .cursor(CursorStyle::ResizeLeftRight)
            })
            .when(!horizontal, |handle| {
                handle.h(px(5.0)).w_full().cursor(CursorStyle::ResizeUpDown)
            })
            .hover(move |handle| handle.bg(theme.colors.borders.focused))
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                let _ = response_view.update(cx, |view, cx| {
                    view.shell.resizing = Some(ResizePane::Response);
                    cx.notify();
                });
            });

        div()
            .flex_1()
            .min_w(px(0.0))
            .min_h(px(0.0))
            .flex()
            .when(horizontal, |work_area| work_area.flex_row())
            .when(!horizontal, |work_area| work_area.flex_col())
            .child(self.render_request_editor(theme, cx))
            .child(handle)
            .child(self.render_response_panel(theme, cx))
    }

    fn render_titlebar(&self, theme: Theme, cx: &mut Context<Self>) -> gpui::Div {
        let switcher_view = cx.weak_entity();
        let open_view = cx.weak_entity();
        let close_view = cx.weak_entity();
        let layout_view = cx.weak_entity();
        let mut popup = PopoverPopup::new()
            .id("workspace-switcher-popup")
            .aria_label("Workspaces")
            .w(px(300.0))
            .p(px(theme.metrics.spacing_2))
            .flex()
            .flex_col()
            .gap(px(theme.metrics.spacing_1))
            .rounded(px(theme.metrics.radius_medium))
            .bg(theme.colors.surfaces.overlay)
            .border_1()
            .border_color(theme.colors.borders.standard)
            .style_with_state(|_, popup| popup.occlude());

        if !self.session.recent_collections.is_empty() {
            popup = popup.child_any(
                div()
                    .px(px(theme.metrics.spacing_2))
                    .py(px(theme.metrics.spacing_1))
                    .text_size(px(theme.typography.caption_size))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.colors.text.muted)
                    .child("RECENT WORKSPACES"),
            );
            for (index, path) in self.session.recent_collections.iter().enumerate() {
                let open_path = path.clone();
                let label = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("Collection")
                    .to_owned();
                let view = cx.weak_entity();
                popup = popup.child_any(components::menu_button(
                    theme,
                    ("workspace-switcher-recent", index),
                    label,
                    move |window, cx| {
                        let path = open_path.clone();
                        let _ = view.update(cx, |view, cx| {
                            view.workspace_switcher_open = false;
                            if !view.loading {
                                view.load_workspace_path(path, None, window, cx);
                            }
                        });
                    },
                ));
            }
            popup = popup.child_any(
                div()
                    .h(px(1.0))
                    .my(px(theme.metrics.spacing_1))
                    .bg(theme.colors.borders.subtle),
            );
        }

        popup = popup.child_any(components::menu_button(
            theme,
            "workspace-switcher-open",
            "Open Collection…",
            move |window, cx| {
                let _ = open_view.update(cx, |view, cx| {
                    view.workspace_switcher_open = false;
                    if !view.loading {
                        view.choose_workspace(window, cx);
                    }
                });
            },
        ));
        if self.loaded_workspace.is_some() {
            popup = popup.child_any(components::menu_button(
                theme,
                "workspace-switcher-close",
                "Close Current Collection",
                move |_, cx| {
                    let _ = close_view.update(cx, |view, cx| {
                        view.workspace_switcher_open = false;
                        view.close_workspace(cx);
                    });
                },
            ));
        }

        let switcher = PopoverRoot::<()>::new()
            .id("workspace-switcher")
            .open(self.workspace_switcher_open)
            .on_open_change(move |open, _, _, cx| {
                let _ = switcher_view.update(cx, |view, cx| {
                    view.workspace_switcher_open = open;
                    cx.notify();
                });
            })
            .child(
                PopoverTrigger::new()
                    .id("workspace-switcher-trigger")
                    .aria_label("Switch workspace")
                    .h(px(28.0))
                    .max_w(px(260.0))
                    .px(px(theme.metrics.spacing_2))
                    .flex()
                    .items_center()
                    .gap(px(theme.metrics.spacing_2))
                    .overflow_hidden()
                    .rounded(px(theme.metrics.radius_small))
                    .style_with_state(move |state, trigger| {
                        trigger
                            .border_1()
                            .border_color(if state.focused {
                                theme.colors.borders.focused
                            } else {
                                theme.colors.borders.subtle
                            })
                            .when(state.open, |trigger| {
                                trigger.bg(theme.colors.surfaces.sidebar)
                            })
                            .hover(move |trigger| trigger.bg(theme.colors.surfaces.sidebar))
                    })
                    .child(components::truncated_label(self.workspace_name()).flex_1())
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(theme.typography.caption_size))
                            .text_color(theme.colors.text.muted)
                            .child("▾"),
                    ),
            )
            .child(
                PopoverPortal::new().child(
                    PopoverPositioner::new()
                        .side_offset(px(theme.metrics.spacing_1))
                        .child(popup),
                ),
            );

        div()
            .h(px(38.0))
            .w_full()
            .pl(px(if cfg!(target_os = "macos") {
                78.0
            } else {
                theme.metrics.spacing_3
            }))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(theme.metrics.spacing_2))
            .bg(theme.colors.surfaces.raised)
            .border_b_1()
            .border_color(theme.colors.borders.subtle)
            .child(switcher)
            .child(
                div()
                    .h_full()
                    .flex_1()
                    .window_control_area(WindowControlArea::Drag)
                    .on_mouse_down(MouseButton::Left, |_, window, _| {
                        window.start_window_move();
                    }),
            )
            .child(components::pane_layout_toggle(
                theme,
                self.shell.pane_layout,
                move |layout, _, cx| {
                    let _ = layout_view.update(cx, |view, cx| {
                        view.shell.set_pane_layout(layout);
                        view.persist_session(cx);
                        cx.notify();
                    });
                },
            ))
            .child(render_windows_controls(theme))
    }

    fn active_request(&self) -> Option<&HttpRequest> {
        let key = self.shell.active_tab()?;
        self.loaded_workspace.as_ref()?.workspace().request(key)
    }

    fn variable_context(&self) -> components::VariableContext {
        let Some(selected) = self.request_editor.selected_environment() else {
            return components::VariableContext {
                values: Default::default(),
                unavailable_message: "Select an environment to resolve this variable".to_owned(),
            };
        };
        let Some(loaded) = &self.loaded_workspace else {
            return components::VariableContext::default();
        };
        match resolve_environment(loaded.workspace().environments(), selected) {
            Ok(environment) => components::VariableContext {
                values: environment.variables().clone(),
                unavailable_message: "Variable value is unavailable".to_owned(),
            },
            Err(error) => components::VariableContext {
                values: Default::default(),
                unavailable_message: error.to_string(),
            },
        }
    }

    fn request_count_label(&self) -> String {
        self.loaded_workspace.as_ref().map_or_else(
            || "No workspace".to_owned(),
            |loaded| format!("{} requests", loaded.workspace().request_count()),
        )
    }

    fn workspace_name(&self) -> String {
        if let Some(name) = self
            .loaded_workspace
            .as_ref()
            .and_then(|loaded| loaded.workspace().metadata().name.as_deref())
        {
            return name.to_owned();
        }
        self.workspace_path
            .as_deref()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .unwrap_or("No collection open")
            .to_owned()
    }
}

impl Render for ProbeApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.pending_tab_reveal {
            self.pending_tab_reveal = false;
            cx.on_next_frame(window, |this, _, cx| {
                this.scroll_active_tab_into_view();
                cx.notify();
            });
        }
        let theme = Theme::for_window_appearance(window.appearance());
        let sidebar_view = cx.weak_entity();
        let status_message = self.message.clone();

        div()
            .size_full()
            .bg(theme.colors.surfaces.window)
            .text_color(theme.colors.text.primary)
            .font_family(theme.typography.interface_family)
            .text_size(px(theme.typography.body_size))
            .line_height(relative(theme.typography.body_line_height))
            .flex()
            .flex_col()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, _, _, cx| view.reset_caret_blink(cx)),
            )
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|view, _, _, cx| view.finish_resize(cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|view, _, _, cx| view.finish_resize(cx)),
            )
            .child(self.render_titlebar(theme, cx))
            .when_some(status_message, |root, message| {
                root.child(
                    div()
                        .px(px(theme.metrics.spacing_3))
                        .py(px(theme.metrics.spacing_2))
                        .bg(theme.colors.status.error)
                        .text_color(theme.colors.text.inverse)
                        .child(message),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.0))
                    .flex()
                    .child(self.render_sidebar(theme, cx))
                    .child(
                        div()
                            .id("sidebar-resize-handle")
                            .w(px(5.0))
                            .h_full()
                            .flex_none()
                            .cursor(CursorStyle::ResizeLeftRight)
                            .bg(theme.colors.borders.subtle)
                            .hover(move |handle| handle.bg(theme.colors.borders.focused))
                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                let _ = sidebar_view.update(cx, |view, cx| {
                                    view.shell.resizing = Some(ResizePane::Sidebar);
                                    cx.notify();
                                });
                            }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .h_full()
                            .flex()
                            .flex_col()
                            .child(self.render_tabs(theme, cx))
                            .child(self.render_editor_response(theme, cx)),
                    ),
            )
    }
}

fn flatten_visible_tree_rows(
    workspace: &Workspace,
    items: &[WorkspaceItemRef],
    depth: usize,
    shell: &ShellState,
    rows: &mut Vec<TreeRow>,
) {
    for item in items {
        rows.push(TreeRow { item: *item, depth });
        if let WorkspaceItemRef::Folder(key) = item
            && shell.folder_is_expanded(*key)
            && let Some(folder) = workspace.folder(*key)
        {
            flatten_visible_tree_rows(workspace, &folder.children, depth + 1, shell, rows);
        }
    }
}

fn placeholder_message(theme: Theme, message: &str) -> gpui::AnyElement {
    div()
        .flex_1()
        .flex()
        .items_center()
        .justify_center()
        .p(px(theme.metrics.spacing_3))
        .text_color(theme.colors.text.muted)
        .child(message.to_owned())
        .into_any_element()
}

fn request_method_options(
    theme: Theme,
    active_method: &str,
) -> Vec<(String, String, Option<gpui::Rgba>)> {
    let mut methods = vec!["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];
    if !methods.contains(&active_method) {
        methods.push(active_method);
    }
    methods
        .into_iter()
        .map(|method| {
            (
                method.to_owned(),
                method.to_owned(),
                Some(theme.method_color(method)),
            )
        })
        .collect()
}

#[cfg(target_os = "windows")]
fn render_windows_controls(theme: Theme) -> gpui::Div {
    let control = move |id: &'static str,
                        label: &'static str,
                        area,
                        destructive: bool,
                        action: fn(&mut Window)| {
        div()
            .id(id)
            .w(px(44.0))
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .window_control_area(area)
            .hover(move |control| {
                if destructive {
                    control
                        .bg(theme.colors.status.error)
                        .text_color(theme.colors.text.inverse)
                } else {
                    control.bg(theme.colors.surfaces.sidebar)
                }
            })
            .on_click(move |_, window, _| action(window))
            .child(label)
    };

    div()
        .h_full()
        .flex()
        .child(control(
            "window-minimize",
            "—",
            WindowControlArea::Min,
            false,
            |window| window.minimize_window(),
        ))
        .child(control(
            "window-maximize",
            "□",
            WindowControlArea::Max,
            false,
            |window| window.zoom_window(),
        ))
        .child(control(
            "window-close",
            "×",
            WindowControlArea::Close,
            true,
            Window::remove_window,
        ))
}

#[cfg(not(target_os = "windows"))]
fn render_windows_controls(_: Theme) -> gpui::Div {
    div()
}

pub fn run() {
    gpui_platform::application().run(|cx: &mut App| {
        base_gpui::init(cx);
        crate::multiline_input::init(cx);

        let bounds = Bounds::centered(None, size(px(1180.0), px(780.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: None,
                    appears_transparent: cfg!(any(target_os = "macos", target_os = "windows")),
                    traffic_light_position: if cfg!(target_os = "macos") {
                        Some(point(px(9.0), px(9.0)))
                    } else {
                        None
                    },
                }),
                app_owns_titlebar_drag: cfg!(target_os = "macos"),
                window_min_size: Some(size(px(760.0), px(560.0))),
                app_id: Some("dev.probe.desktop".to_owned()),
                ..Default::default()
            },
            |window, cx| {
                let view = cx.new(|cx| ProbeApp::new(window, cx));
                view.update(cx, |view, cx| view.restore_session(window, cx));
                view
            },
        )
        .expect("failed to open Probe's application window");

        cx.activate(true);
    });
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, time::Duration};

    use gpui::{Modifiers, TestAppContext, VisualTestContext, px, size};
    use probe_http::{HttpResponse, ResponseHeader};

    use super::ProbeApp;
    use crate::{
        request_editor::{BodyEditorKind, EditorSection},
        response_viewer::ResponseViewerTab,
    };

    fn bundled_fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/opencollection/phase1-bundled.yml")
    }

    fn large_fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/opencollection/phase2-large-workspace.yml")
    }

    fn environment_fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/opencollection/phase4-environments.yml")
    }

    #[gpui::test]
    fn recent_collection_in_sidebar_loads_the_workspace(cx: &mut TestAppContext) {
        cx.update(base_gpui::init);
        let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
            ProbeApp::new(window, cx)
        });
        let fixture = bundled_fixture();
        window
            .update(cx, |view, _, cx| {
                view.session_store = None;
                view.session.recent_collections = vec![fixture.clone()];
                cx.notify();
            })
            .expect("test window should be open");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let recent = visual
            .debug_bounds("recent-collection-0")
            .expect("recent collection should be rendered");
        visual.simulate_click(recent.center(), Modifiers::default());
        visual.run_until_parked();
        cx.run_until_parked();

        let expected = fixture.canonicalize().expect("fixture should exist");
        let (actual, loading, message) = window
            .update(cx, |view, _, _| {
                (
                    view.workspace_path.clone(),
                    view.loading,
                    view.message.clone(),
                )
            })
            .expect("test window should remain open");
        assert_eq!(
            actual.as_deref(),
            Some(expected.as_path()),
            "loading={loading}, message={message:?}"
        );
    }

    #[gpui::test]
    fn large_sidebar_only_renders_the_visible_rows(cx: &mut TestAppContext) {
        cx.update(base_gpui::init);
        let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
            ProbeApp::new(window, cx)
        });
        let fixture = large_fixture()
            .canonicalize()
            .expect("fixture should exist");
        let workspace =
            probe_opencollection::load_workspace(&fixture).expect("large fixture should load");
        window
            .update(cx, |view, _, cx| {
                view.session_store = None;
                view.set_workspace(fixture, workspace);
                cx.notify();
            })
            .expect("test window should be open");
        cx.run_until_parked();

        let (total_rows, rendered_rows) = window
            .update(cx, |view, _, _| {
                (view.visible_tree_rows.len(), view.rendered_sidebar_rows)
            })
            .expect("test window should remain open");
        assert!(total_rows >= 1_000);
        assert!(rendered_rows > 0);
        assert!(
            rendered_rows < total_rows,
            "virtualized sidebar rendered all {total_rows} rows"
        );
    }

    #[gpui::test]
    fn request_editor_sections_render_for_an_open_request(cx: &mut TestAppContext) {
        cx.update(base_gpui::init);
        let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
            ProbeApp::new(window, cx)
        });
        let fixture = bundled_fixture()
            .canonicalize()
            .expect("fixture should exist");
        let workspace =
            probe_opencollection::load_workspace(&fixture).expect("fixture should load");
        let request_key = workspace.requests()[0].key();
        window
            .update(cx, |view, _, cx| {
                view.session_store = None;
                view.set_workspace(fixture, workspace);
                view.select_request(request_key, cx);
            })
            .expect("test window should be open");

        for section in EditorSection::ALL {
            window
                .update(cx, |view, _, cx| {
                    view.request_editor.section = section;
                    if section == EditorSection::Body {
                        view.change_body_kind(request_key, BodyEditorKind::Json, cx);
                    }
                    cx.notify();
                })
                .expect("test window should remain open");
            cx.run_until_parked();
            {
                let mut visual = VisualTestContext::from_window(window.into(), cx);
                assert!(visual.debug_bounds("request-url-bar").is_some());
                assert!(visual.debug_bounds("request-method-trigger").is_some());
                assert!(visual.debug_bounds("request-environment-trigger").is_some());
                if section == EditorSection::Body {
                    let body = visual
                        .debug_bounds("request-body-editor")
                        .expect("JSON body editor should render");
                    assert!(body.size.height > px(120.0));
                }
            }
        }
    }

    #[gpui::test]
    fn request_editor_renders_multiline_json_body(cx: &mut TestAppContext) {
        cx.update(base_gpui::init);
        cx.update(crate::multiline_input::init);
        let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
            ProbeApp::new(window, cx)
        });
        let fixture = bundled_fixture()
            .canonicalize()
            .expect("fixture should exist");
        let workspace =
            probe_opencollection::load_workspace(&fixture).expect("fixture should load");
        let request_key = workspace.requests()[0].key();
        window
            .update(cx, |view, _, cx| {
                view.session_store = None;
                view.set_workspace(fixture, workspace);
                view.select_request(request_key, cx);
                view.request_editor.section = EditorSection::Body;
                view.edit_request(
                    request_key,
                    |request| {
                        request.body = Some(probe_core::RequestBody::Single(
                            probe_core::Body::Raw(probe_core::RawBody {
                                kind: probe_core::RawBodyKind::Json,
                                data: "{\n  \"name\": \"Milo\"\n}".to_owned(),
                            }),
                        ));
                    },
                    cx,
                );
            })
            .expect("test window should be open");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let body = visual
            .debug_bounds("request-body-editor")
            .expect("multiline JSON body editor should render");
        assert!(body.size.height > px(120.0));
    }

    #[gpui::test]
    fn completed_response_renders_pretty_raw_headers_and_search(cx: &mut TestAppContext) {
        cx.update(base_gpui::init);
        cx.update(crate::multiline_input::init);
        let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
            ProbeApp::new(window, cx)
        });
        let fixture = bundled_fixture()
            .canonicalize()
            .expect("fixture should exist");
        let workspace =
            probe_opencollection::load_workspace(&fixture).expect("fixture should load");
        let request_key = workspace.requests()[0].key();
        window
            .update(cx, |view, _, cx| {
                view.session_store = None;
                view.set_workspace(fixture, workspace);
                view.select_request(request_key, cx);
                let (cancellation, _) = tokio::sync::oneshot::channel();
                let generation = view.execution.begin(request_key, cancellation);
                view.complete_execution(
                    request_key,
                    generation,
                    Ok(HttpResponse {
                        status: 201,
                        reason: "Created".to_owned(),
                        url: "https://api.example.test/users".to_owned(),
                        duration: Duration::from_millis(42),
                        size: 11,
                        headers: vec![ResponseHeader {
                            name: "content-type".to_owned(),
                            value: "application/json".to_owned(),
                        }],
                        body: br#"{"ok":true}"#.to_vec(),
                        body_complete: true,
                    }),
                    cx,
                );
                cx.notify();
            })
            .expect("test window should be open");
        cx.run_until_parked();

        {
            let mut visual = VisualTestContext::from_window(window.into(), cx);
            assert!(visual.debug_bounds("response-status").is_some());
            assert!(visual.debug_bounds("response-tab-pretty").is_some());
            assert!(visual.debug_bounds("response-tab-raw").is_some());
            assert!(visual.debug_bounds("response-tab-headers").is_some());
            assert!(visual.debug_bounds("response-search").is_some());
            assert!(visual.debug_bounds("response-body").is_some());
            assert!(visual.debug_bounds("response-headers").is_none());
        }

        window
            .update(cx, |view, _, cx| {
                view.response_viewer.set_tab(ResponseViewerTab::Headers);
                cx.notify();
            })
            .expect("test window should remain open");
        cx.run_until_parked();
        {
            let mut visual = VisualTestContext::from_window(window.into(), cx);
            assert!(visual.debug_bounds("response-headers").is_some());
            assert!(visual.debug_bounds("response-body").is_none());
        }

        window
            .update(cx, |view, _, cx| {
                view.response_viewer.set_tab(ResponseViewerTab::Pretty);
                view.response_viewer.set_search("ok".to_owned());
                cx.notify();
            })
            .expect("test window should remain open");
        cx.run_until_parked();
        let match_count = window
            .update(cx, |view, _, _| {
                view.response_viewer.matches(request_key).len()
            })
            .expect("test window should remain open");
        assert!(match_count >= 1);
        {
            let mut visual = VisualTestContext::from_window(window.into(), cx);
            assert!(visual.debug_bounds("response-search-count").is_some());
            assert!(visual.debug_bounds("response-body").is_some());
        }
    }

    #[gpui::test]
    fn large_response_body_only_renders_visible_rows(cx: &mut TestAppContext) {
        cx.update(base_gpui::init);
        cx.update(crate::multiline_input::init);
        let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
            ProbeApp::new(window, cx)
        });
        let fixture = bundled_fixture()
            .canonicalize()
            .expect("fixture should exist");
        let workspace =
            probe_opencollection::load_workspace(&fixture).expect("fixture should load");
        let request_key = workspace.requests()[0].key();
        let body = (0..20_000)
            .map(|index| format!("line-{index:05}"))
            .collect::<Vec<_>>()
            .join("\n")
            .into_bytes();
        window
            .update(cx, |view, _, cx| {
                view.session_store = None;
                view.set_workspace(fixture, workspace);
                view.select_request(request_key, cx);
                view.shell.response_height = 220.0;
                let (cancellation, _) = tokio::sync::oneshot::channel();
                let generation = view.execution.begin(request_key, cancellation);
                view.complete_execution(
                    request_key,
                    generation,
                    Ok(HttpResponse {
                        status: 200,
                        reason: "OK".to_owned(),
                        url: "https://api.example.test/lines".to_owned(),
                        duration: Duration::from_millis(12),
                        size: body.len(),
                        headers: vec![ResponseHeader {
                            name: "content-type".to_owned(),
                            value: "text/plain".to_owned(),
                        }],
                        body,
                        body_complete: true,
                    }),
                    cx,
                );
                view.response_viewer.set_tab(ResponseViewerTab::Raw);
                cx.notify();
            })
            .expect("test window should be open");
        cx.run_until_parked();

        let (total_rows, rendered_rows) = window
            .update(cx, |view, _, _| {
                (
                    view.response_viewer.visible_lines(request_key).len(),
                    view.rendered_response_rows,
                )
            })
            .expect("test window should remain open");
        assert!(total_rows >= 20_000);
        assert!(rendered_rows > 0);
        assert!(
            rendered_rows < total_rows,
            "virtualized response viewer rendered all {total_rows} rows"
        );
    }

    #[gpui::test]
    fn environment_selection_is_shared_when_opening_another_request(cx: &mut TestAppContext) {
        cx.update(base_gpui::init);
        let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
            ProbeApp::new(window, cx)
        });
        let fixture = environment_fixture()
            .canonicalize()
            .expect("fixture should exist");
        let workspace =
            probe_opencollection::load_workspace(&fixture).expect("fixture should load");
        let first = workspace.requests()[0].key();
        let second = workspace.requests()[1].key();
        window
            .update(cx, |view, _, cx| {
                view.session_store = None;
                view.set_workspace(fixture, workspace);
                view.select_request(first, cx);
                view.request_editor
                    .select_environment(Some("development".to_owned()));
                view.select_request(second, cx);
                assert_eq!(
                    view.request_editor.selected_environment(),
                    Some("development")
                );
            })
            .expect("test window should be open");
    }

    #[gpui::test]
    fn request_variables_render_inline_and_show_resolved_tooltips(cx: &mut TestAppContext) {
        cx.update(base_gpui::init);
        let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
            ProbeApp::new(window, cx)
        });
        let fixture = environment_fixture()
            .canonicalize()
            .expect("fixture should exist");
        let workspace =
            probe_opencollection::load_workspace(&fixture).expect("fixture should load");
        let request_key = workspace.requests()[0].key();
        window
            .update(cx, |view, _, cx| {
                view.session_store = None;
                view.set_workspace(fixture, workspace);
                view.select_request(request_key, cx);
                view.request_editor
                    .select_environment(Some("development".to_owned()));
                cx.notify();
            })
            .expect("test window should be open");
        cx.run_until_parked();

        let (variable_point, input_point) = {
            let mut visual = VisualTestContext::from_window(window.into(), cx);
            let variable = visual
                .debug_bounds("variable-highlight-overlay")
                .expect("variable overlay should render");
            let url_bar = visual
                .debug_bounds("request-url-bar")
                .expect("request URL bar should render");
            (
                variable.center(),
                gpui::point(url_bar.right() - px(110.0), url_bar.center().y),
            )
        };
        {
            let mut visual = VisualTestContext::from_window(window.into(), cx);
            visual.simulate_mouse_move(variable_point, None, Modifiers::default());
            visual.run_until_parked();
        }
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(200));
        cx.run_until_parked();
        cx.run_until_parked();
        {
            let mut visual = VisualTestContext::from_window(window.into(), cx);
            assert!(
                visual
                    .debug_bounds("variable-input-tooltip-popup")
                    .is_some()
            );
            visual.simulate_click(input_point, Modifiers::default());
            visual.run_until_parked();
        }
        let select_all = if cfg!(target_os = "macos") {
            "cmd-a"
        } else {
            "ctrl-a"
        };
        cx.simulate_keystrokes(window.into(), select_all);
        cx.simulate_input(window.into(), "https://changed.example");
        cx.run_until_parked();
        let edited_url = window
            .update(cx, |view, _, _| {
                view.active_request()
                    .and_then(|request| request.url.clone())
            })
            .expect("test window should remain open");
        assert_eq!(edited_url.as_deref(), Some("https://changed.example"));
    }

    #[gpui::test]
    fn long_request_names_ellipsis_instead_of_wrapping(cx: &mut TestAppContext) {
        cx.update(base_gpui::init);
        let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
            ProbeApp::new(window, cx)
        });
        let fixture = bundled_fixture()
            .canonicalize()
            .expect("fixture should exist");
        let workspace =
            probe_opencollection::load_workspace(&fixture).expect("fixture should load");
        let request_key = workspace.requests()[0].key();
        let long_name =
            "List every pet owned by the currently authenticated user across every environment";
        window
            .update(cx, |view, _, cx| {
                view.session_store = None;
                view.set_workspace(fixture, workspace);
                view.edit_request(
                    request_key,
                    |request| request.metadata.name = Some(long_name.to_owned()),
                    cx,
                );
                view.select_request(request_key, cx);
            })
            .expect("test window should be open");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let tab_label = visual
            .debug_bounds("request-tab-label")
            .expect("request tab label should render");
        let tree_label = visual
            .debug_bounds("request-tree-label")
            .expect("sidebar request label should render");
        assert!(
            tab_label.size.height < px(28.0),
            "request tab label wrapped onto multiple lines: {:?}",
            tab_label.size
        );
        assert!(
            tree_label.size.height < px(28.0),
            "sidebar request label wrapped onto multiple lines: {:?}",
            tree_label.size
        );
        assert!(
            tab_label.size.width <= px(220.0),
            "request tab label exceeded the tab max width: {:?}",
            tab_label.size
        );
    }

    #[gpui::test]
    fn opening_many_request_tabs_scrolls_to_the_active_tab(cx: &mut TestAppContext) {
        cx.update(base_gpui::init);
        let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
            ProbeApp::new(window, cx)
        });
        let fixture = large_fixture()
            .canonicalize()
            .expect("fixture should exist");
        let workspace =
            probe_opencollection::load_workspace(&fixture).expect("large fixture should load");
        let keys: Vec<_> = workspace
            .requests()
            .iter()
            .take(12)
            .map(|request| request.key())
            .collect();
        assert!(keys.len() >= 12, "large fixture should have many requests");
        window
            .update(cx, |view, _, cx| {
                view.session_store = None;
                view.set_workspace(fixture, workspace);
                cx.notify();
            })
            .expect("test window should be open");
        cx.run_until_parked();

        window
            .update(cx, |view, _, cx| {
                for key in &keys {
                    view.select_request(*key, cx);
                }
            })
            .expect("test window should remain open");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.update(|window, cx| {
            window.simulate_next_frame(cx);
        });
        cx.run_until_parked();

        let (offset, max_offset, child_count, last_visible) = window
            .update(cx, |view, _, _| {
                let last = view.tab_bar_scroll.children_count().saturating_sub(1);
                let viewport = view.tab_bar_scroll.bounds();
                let offset = view.tab_bar_scroll.offset();
                let last_visible =
                    view.tab_bar_scroll
                        .bounds_for_item(last)
                        .is_some_and(|bounds| {
                            bounds.right() + offset.x <= viewport.right() + px(1.0)
                                && bounds.left() + offset.x >= viewport.left() - viewport.size.width
                        });
                (
                    offset,
                    view.tab_bar_scroll.max_offset(),
                    view.tab_bar_scroll.children_count(),
                    last_visible,
                )
            })
            .expect("test window should remain open");

        assert!(
            child_count >= 12,
            "tab strip should track opened request tabs, got {child_count}"
        );
        assert!(
            max_offset.x > px(0.0),
            "opening many tabs should overflow the tab bar, max_offset={max_offset:?}"
        );
        assert!(
            offset.x < px(0.0),
            "tab bar should scroll right to reveal the newest tab, offset={offset:?}"
        );
        assert!(
            last_visible,
            "the newly opened tab should be visible in the tab bar"
        );
    }
}
