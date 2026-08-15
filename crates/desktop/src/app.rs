use std::{
    fs,
    path::{Path, PathBuf},
};

use base_gpui::popover::{
    PopoverPopup, PopoverPortal, PopoverPositioner, PopoverRoot, PopoverTrigger,
};
use gpui::{
    App, AppContext as _, Bounds, Context, CursorStyle, FontWeight, InteractiveElement as _,
    IntoElement, MouseButton, MouseMoveEvent, ParentElement as _, PathPromptOptions, Render,
    StatefulInteractiveElement as _, Styled as _, Task, TitlebarOptions, Window, WindowBounds,
    WindowControlArea, WindowOptions, div, point, prelude::FluentBuilder as _, px, relative, size,
    uniform_list,
};
use probe_core::{HttpRequest, RequestKey, Workspace, WorkspaceItemRef};
use probe_opencollection::{LoadedWorkspace, load_workspace};

use crate::{
    components,
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
    #[cfg(test)]
    rendered_sidebar_rows: usize,
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
            #[cfg(test)]
            rendered_sidebar_rows: 0,
            _quit_subscription: quit_subscription,
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
        self.loaded_workspace = Some(workspace);
        self.workspace_path = Some(path);
        self.shell.reset_for_workspace();
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
        self.loaded_workspace = None;
        self.workspace_path = None;
        self.shell.reset_for_workspace();
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
            self.persist_session(cx);
            cx.notify();
        }
    }

    fn close_tab(&mut self, key: RequestKey, cx: &mut Context<Self>) {
        self.shell.close_tab(key);
        self.persist_session(cx);
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
                            .text_size(px(10.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(if selected {
                                theme.colors.selection.active_foreground
                            } else {
                                method_color(theme, &method)
                            })
                            .child(method),
                    )
                    .child(div().flex_1().overflow_hidden().child(label.to_owned()))
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
                    .child(if expanded { "▾" } else { "▸" })
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(label.to_owned()),
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
                        .child(label)
                        .child(
                            div()
                                .text_size(px(theme.typography.caption_size))
                                .text_color(theme.colors.text.muted)
                                .overflow_hidden()
                                .child(detail),
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
        let mut tabs = div()
            .id("request-tabs")
            .h(px(38.0))
            .w_full()
            .flex()
            .items_end()
            .overflow_x_scroll()
            .bg(theme.colors.surfaces.raised)
            .border_b_1()
            .border_color(theme.colors.borders.subtle);
        let Some(loaded) = &self.loaded_workspace else {
            return tabs;
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
            tabs = tabs.child(
                div()
                    .id(("request-tab", key.slot()))
                    .h_full()
                    .min_w(px(120.0))
                    .max_w(px(220.0))
                    .px(px(theme.metrics.spacing_3))
                    .flex()
                    .items_center()
                    .gap(px(theme.metrics.spacing_2))
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
                    .child(div().flex_1().overflow_hidden().child(label.to_owned()))
                    .child(
                        div()
                            .id(("close-tab", key.slot()))
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
        tabs
    }

    fn render_request_editor(&self, theme: Theme) -> gpui::Div {
        let Some(request) = self.active_request() else {
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
        let method = request.method.as_deref().unwrap_or("GET").to_uppercase();
        let url = request.url.as_deref().unwrap_or("No URL configured");
        div()
            .flex_1()
            .min_w(px(0.0))
            .min_h(px(120.0))
            .p(px(theme.metrics.spacing_4))
            .flex()
            .flex_col()
            .gap(px(theme.metrics.spacing_4))
            .bg(theme.colors.surfaces.editor)
            .child(
                div()
                    .text_size(px(theme.typography.title_size))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(
                        request
                            .metadata
                            .name
                            .as_deref()
                            .unwrap_or("Untitled request")
                            .to_owned(),
                    ),
            )
            .child(
                div()
                    .h(px(40.0))
                    .w_full()
                    .flex()
                    .items_center()
                    .border_1()
                    .border_color(theme.colors.borders.standard)
                    .rounded(px(theme.metrics.radius_medium))
                    .child(
                        div()
                            .h_full()
                            .px(px(theme.metrics.spacing_3))
                            .flex()
                            .items_center()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(method_color(theme, &method))
                            .border_r_1()
                            .border_color(theme.colors.borders.standard)
                            .child(method),
                    )
                    .child(
                        div()
                            .px(px(theme.metrics.spacing_3))
                            .font_family(theme.typography.monospace_family)
                            .child(url.to_owned()),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap(px(theme.metrics.spacing_4))
                    .text_color(theme.colors.text.secondary)
                    .child(format!("Query  {}", request.query_parameters.len()))
                    .child(format!("Headers  {}", request.headers.len()))
                    .child(if request.body.is_some() {
                        "Body"
                    } else {
                        "No body"
                    })
                    .child(if request.authentication.is_some() {
                        "Authentication"
                    } else {
                        "No authentication"
                    }),
            )
    }

    fn render_response_panel(&self, theme: Theme) -> gpui::Div {
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
                    .border_b_1()
                    .border_color(theme.colors.borders.subtle)
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("Response"))
                    .child(
                        div()
                            .text_size(px(theme.typography.caption_size))
                            .text_color(theme.colors.text.muted)
                            .child("Send is available in Phase 12"),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(theme.colors.text.muted)
                    .child("Responses will appear here."),
            )
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
            .child(self.render_request_editor(theme))
            .child(handle)
            .child(self.render_response_panel(theme))
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
            .border_color(theme.colors.borders.standard);

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
                    .child(div().overflow_hidden().child(self.workspace_name()))
                    .child(
                        div()
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_window_appearance(_window.appearance());
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

fn method_color(theme: Theme, method: &str) -> gpui::Rgba {
    match method {
        "GET" => theme.colors.methods.get,
        "POST" => theme.colors.methods.post,
        "PUT" => theme.colors.methods.put,
        "PATCH" => theme.colors.methods.patch,
        "DELETE" => theme.colors.methods.delete,
        _ => theme.colors.methods.other,
    }
}

#[cfg(target_os = "windows")]
fn render_windows_controls(theme: Theme) -> gpui::Div {
    let control = move |id: &'static str, label: &'static str, area, action: fn(&mut Window)| {
        div()
            .id(id)
            .w(px(44.0))
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .window_control_area(area)
            .hover(move |control| control.bg(theme.colors.surfaces.sidebar))
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
            |window| window.minimize_window(),
        ))
        .child(control(
            "window-maximize",
            "□",
            WindowControlArea::Max,
            |window| window.zoom_window(),
        ))
        .child(
            control(
                "window-close",
                "×",
                WindowControlArea::Close,
                Window::remove_window,
            )
            .hover(move |control| {
                control
                    .bg(theme.colors.status.error)
                    .text_color(theme.colors.text.inverse)
            }),
        )
}

#[cfg(not(target_os = "windows"))]
fn render_windows_controls(_: Theme) -> gpui::Div {
    div()
}

pub fn run() {
    gpui_platform::application().run(|cx: &mut App| {
        base_gpui::init(cx);

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
    use std::path::PathBuf;

    use gpui::{Modifiers, TestAppContext, VisualTestContext, px, size};

    use super::ProbeApp;

    fn bundled_fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/opencollection/phase1-bundled.yml")
    }

    fn large_fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/opencollection/phase2-large-workspace.yml")
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
}
