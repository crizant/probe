use std::path::{Path, PathBuf};

use gpui::{
    App, AppContext as _, Bounds, Context, CursorStyle, FontWeight, InteractiveElement as _,
    IntoElement, MouseButton, MouseMoveEvent, ParentElement as _, PathPromptOptions, Render,
    StatefulInteractiveElement as _, Styled as _, TitlebarOptions, Window, WindowBounds,
    WindowOptions, div, prelude::FluentBuilder as _, px, relative, size,
};
use probe_core::{HttpRequest, RequestKey, WorkspaceItemRef};
use probe_opencollection::{LoadedWorkspace, load_workspace};

use crate::{
    components,
    shell::{ResizePane, ShellState},
    theme::Theme,
};

pub struct ProbeApp {
    loaded_workspace: Option<LoadedWorkspace>,
    workspace_path: Option<PathBuf>,
    shell: ShellState,
    loading: bool,
    message: Option<String>,
}

impl ProbeApp {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        cx.observe_window_appearance(window, |_, window, _| window.refresh())
            .detach();

        Self {
            loaded_workspace: None,
            workspace_path: None,
            shell: ShellState::default(),
            loading: false,
            message: None,
        }
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
                let _ = view.update_in(cx, |view, _, cx| {
                    view.loading = true;
                    view.message = None;
                    cx.notify();
                });
                let load_path = path.clone();
                let result = cx
                    .background_spawn(async move { load_workspace(load_path) })
                    .await;
                let _ = view.update_in(cx, |view, _, cx| {
                    view.loading = false;
                    match result {
                        Ok(workspace) => view.set_workspace(path, workspace),
                        Err(error) => view.message = Some(error.to_string()),
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
        self.message = None;
    }

    fn select_request(&mut self, key: RequestKey, cx: &mut Context<Self>) {
        if self
            .loaded_workspace
            .as_ref()
            .is_some_and(|loaded| loaded.workspace().request(key).is_some())
        {
            self.shell.open_request(key);
            cx.notify();
        }
    }

    fn close_tab(&mut self, key: RequestKey, cx: &mut Context<Self>) {
        self.shell.close_tab(key);
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
            Some(ResizePane::Response) => self.shell.resize_response(
                window.window_bounds().get_bounds().size.height.into(),
                event.position.y.into(),
            ),
            None => return,
        }
        cx.notify();
    }

    fn render_tree_item(
        &self,
        item: WorkspaceItemRef,
        depth: usize,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let Some(loaded) = &self.loaded_workspace else {
            return div().into_any_element();
        };
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
                let children = folder.children.clone();
                let label = folder.metadata.name.as_deref().unwrap_or("Untitled folder");
                let view = cx.weak_entity();
                let mut result = div().flex().flex_col().child(
                    div()
                        .id(("folder-tree-item", key.slot()))
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
                                cx.notify();
                            });
                        })
                        .child(if expanded { "▾" } else { "▸" })
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(label.to_owned()),
                        ),
                );
                if expanded {
                    for child in children {
                        result = result.child(self.render_tree_item(child, depth + 1, theme, cx));
                    }
                }
                result.into_any_element()
            }
        }
    }

    fn render_sidebar(&self, theme: Theme, cx: &mut Context<Self>) -> gpui::Div {
        let mut tree = div()
            .id("request-tree")
            .flex_1()
            .overflow_y_scroll()
            .p(px(theme.metrics.spacing_2))
            .flex()
            .flex_col();
        if let Some(loaded) = &self.loaded_workspace {
            for item in loaded.workspace().root_items() {
                tree = tree.child(self.render_tree_item(*item, 0, theme, cx));
            }
        } else {
            tree = tree.child(
                div()
                    .p(px(theme.metrics.spacing_3))
                    .text_color(theme.colors.text.muted)
                    .child("Open a collection to browse its requests."),
            );
        }

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
            .h(px(self.shell.response_height))
            .w_full()
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
        let open_view = cx.weak_entity();
        let sidebar_view = cx.weak_entity();
        let response_view = cx.weak_entity();
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
                cx.listener(|view, _, _, cx| {
                    view.shell.resizing = None;
                    cx.notify();
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|view, _, _, cx| {
                    view.shell.resizing = None;
                    cx.notify();
                }),
            )
            .child(
                div()
                    .h(px(50.0))
                    .w_full()
                    .px(px(theme.metrics.spacing_3))
                    .flex()
                    .items_center()
                    .justify_between()
                    .bg(theme.colors.surfaces.raised)
                    .border_b_1()
                    .border_color(theme.colors.borders.subtle)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(theme.metrics.spacing_3))
                            .child(div().font_weight(FontWeight::SEMIBOLD).child("Probe"))
                            .child(
                                div()
                                    .text_color(theme.colors.text.secondary)
                                    .child(self.workspace_name()),
                            ),
                    )
                    .child(components::primary_button(
                        theme,
                        "open-collection",
                        if self.loading {
                            "Opening…"
                        } else {
                            "Open Collection…"
                        },
                        move |_, window, cx| {
                            let _ = open_view.update(cx, |view, cx| {
                                if !view.loading {
                                    view.choose_workspace(window, cx);
                                }
                            });
                        },
                    )),
            )
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
                            .child(self.render_request_editor(theme))
                            .child(
                                div()
                                    .id("response-resize-handle")
                                    .h(px(5.0))
                                    .w_full()
                                    .flex_none()
                                    .cursor(CursorStyle::ResizeUpDown)
                                    .bg(theme.colors.borders.subtle)
                                    .hover(move |handle| handle.bg(theme.colors.borders.focused))
                                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                        let _ = response_view.update(cx, |view, cx| {
                                            view.shell.resizing = Some(ResizePane::Response);
                                            cx.notify();
                                        });
                                    }),
                            )
                            .child(self.render_response_panel(theme)),
                    ),
            )
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

pub fn run() {
    gpui_platform::application().run(|cx: &mut App| {
        base_gpui::init(cx);

        let bounds = Bounds::centered(None, size(px(1180.0), px(780.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Probe".into()),
                    ..Default::default()
                }),
                window_min_size: Some(size(px(760.0), px(560.0))),
                app_id: Some("dev.probe.desktop".to_owned()),
                ..Default::default()
            },
            |window, cx| cx.new(|cx| ProbeApp::new(window, cx)),
        )
        .expect("failed to open Probe's application window");

        cx.activate(true);
    });
}
