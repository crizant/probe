use super::*;

#[derive(Clone)]
pub(super) struct TabDrag {
    pub(super) key: RequestKey,
    pub(super) label: String,
    pub(super) active: bool,
}

impl Render for TabDrag {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_window_appearance(window.appearance());
        div()
            .px(px(theme.metrics.spacing_2))
            .py(px(theme.metrics.spacing_1))
            .rounded(px(theme.metrics.radius_small))
            .bg(theme.colors.surfaces.overlay)
            .border_1()
            .border_color(theme.colors.borders.standard)
            .font_family(theme.typography.interface_family)
            .text_size(px(theme.typography.body_size))
            .line_height(relative(theme.typography.body_line_height))
            .text_color(if self.active {
                theme.colors.actions.accent
            } else {
                theme.colors.text.secondary
            })
            .child(self.label.clone())
    }
}

impl ProbeApp {
    pub(super) fn on_tab_drag_move(
        &mut self,
        source: RequestKey,
        pointer: Point<Pixels>,
        bounds: Bounds<Pixels>,
        cx: &mut Context<Self>,
    ) {
        if !self.shell.tabs().contains(&source) || !bounds.contains(&pointer) {
            self.tab_auto_scroll.stop();
            self.tab_drop_target = None;
            cx.notify();
            return;
        }
        self.tab_auto_scroll.last_drag_position = Some(pointer);
        self.recompute_tab_drop_from_pointer(source, pointer, bounds, cx);
        let edge = 24.0;
        let x = f32::from(pointer.x);
        let left = f32::from(bounds.left());
        let right = f32::from(bounds.right());
        let delta = if x <= left + edge {
            Some(px(16.0))
        } else if x >= right - edge {
            Some(px(-16.0))
        } else {
            None
        };
        self.tab_auto_scroll.set(delta, cx, |delta, view, cx| {
            view.scroll_tab_strip_by(delta, cx);
            if let Some(source) = view.tab_drag_source
                && let Some(pointer) = view.tab_auto_scroll.last_drag_position
            {
                view.recompute_tab_drop_from_pointer(
                    source,
                    pointer,
                    view.tab_bar_scroll.bounds(),
                    cx,
                );
            }
        });
    }

    fn scroll_tab_strip_by(&mut self, delta: Pixels, cx: &mut Context<Self>) {
        let mut offset = self.tab_bar_scroll.offset();
        let max = self.tab_bar_scroll.max_offset();
        let next = (f32::from(offset.x) + f32::from(delta)).clamp(-f32::from(max.x), 0.0);
        if next != f32::from(offset.x) {
            offset.x = px(next);
            self.tab_bar_scroll.set_offset(offset);
            cx.notify();
        } else {
            self.tab_auto_scroll.stop();
        }
    }

    fn recompute_tab_drop_from_pointer(
        &mut self,
        source: RequestKey,
        pointer: Point<Pixels>,
        viewport: Bounds<Pixels>,
        cx: &mut Context<Self>,
    ) {
        if !viewport.contains(&pointer) {
            self.tab_drop_target = None;
            cx.notify();
            return;
        }
        let x = f32::from(pointer.x);
        let offset = f32::from(self.tab_bar_scroll.offset().x);
        let mut candidate = None;
        for (index, key) in self.shell.tabs().iter().enumerate() {
            let Some(bounds) = self.tab_bar_scroll.bounds_for_item(index) else {
                continue;
            };
            let left = f32::from(bounds.left()) + offset;
            let right = f32::from(bounds.right()) + offset;
            if x < right {
                candidate = Some((*key, x < (left + right) / 2.0));
                break;
            }
            candidate = Some((*key, false));
        }
        if let Some((target, before)) = candidate {
            self.update_tab_drop_target(source, target, before, cx);
        } else if self.tab_drop_target.take().is_some() {
            cx.notify();
        }
    }

    pub(super) fn update_tab_drop_target(
        &mut self,
        source: RequestKey,
        target: RequestKey,
        before: bool,
        cx: &mut Context<Self>,
    ) {
        let next = (source != target
            && self.shell.tabs().contains(&source)
            && self.shell.tabs().contains(&target))
        .then_some((target, before));
        if self.tab_drop_target != next {
            self.tab_drop_target = next;
            cx.notify();
        }
    }

    pub(super) fn drop_tab(&mut self, source: RequestKey, cx: &mut Context<Self>) {
        let target = self.tab_drop_target.take();
        self.tab_drag_source = None;
        self.tab_auto_scroll.stop();
        if let Some((target, before)) = target
            && self.shell.move_tab(source, target, before)
        {
            self.persist_session(cx);
        }
        cx.notify();
    }

    pub(super) fn close_tab_now(&mut self, key: RequestKey, cx: &mut Context<Self>) {
        let previous_active = self.shell.active_tab();
        self.shell.close_tab(key);
        if self.shell.active_tab() != previous_active {
            self.select_and_reveal_active_request_in_sidebar();
        }
        self.reveal_active_tab();
        self.persist_session(cx);
        cx.notify();
    }

    pub(super) fn close_other_tabs_now(&mut self, keep: RequestKey, cx: &mut Context<Self>) {
        let open_tabs = self.shell.tabs().to_vec();
        if !open_tabs.contains(&keep) {
            return;
        }
        for key in open_tabs {
            if key != keep {
                self.shell.close_tab(key);
            }
        }
        self.shell.open_request(keep);
        self.select_and_reveal_active_request_in_sidebar();
        self.reveal_active_tab();
        self.persist_session(cx);
        cx.notify();
    }

    pub(super) fn dirty_keys(&self) -> Vec<RequestKey> {
        let Some(loaded) = &self.loaded_workspace else {
            return Vec::new();
        };
        self.persistence
            .dirty_keys(loaded.requests().iter().filter_map(|located| {
                loaded
                    .workspace()
                    .request(located.key())
                    .map(|request| (located.key(), request))
            }))
    }

    pub(super) fn request_is_dirty(&self, key: RequestKey) -> bool {
        self.loaded_workspace
            .as_ref()
            .and_then(|loaded| loaded.workspace().request(key))
            .is_some_and(|request| self.persistence.is_dirty(key, request))
    }

    pub(super) fn request_close_tab(
        &mut self,
        key: RequestKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_tab_context_menu(cx);
        if self.request_is_dirty(key) {
            self.prompt_unsaved(vec![key], PendingClose::Tab(key), window, cx);
        } else {
            self.close_tab_now(key, cx);
        }
    }

    pub(super) fn other_dirty_tab_keys(&self, keep: RequestKey) -> Vec<RequestKey> {
        self.shell
            .tabs()
            .iter()
            .copied()
            .filter(|key| *key != keep)
            .filter(|key| self.request_is_dirty(*key))
            .collect()
    }

    pub(super) fn request_close_other_tabs(
        &mut self,
        keep: RequestKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_tab_context_menu(cx);
        if !self.shell.tabs().contains(&keep) {
            return;
        }
        let dirty = self.other_dirty_tab_keys(keep);
        if dirty.is_empty() {
            self.close_other_tabs_now(keep, cx);
        } else {
            self.prompt_unsaved(dirty, PendingClose::OtherTabs { keep }, window, cx);
        }
    }

    pub(super) fn request_close_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dirty = self.dirty_keys();
        if !dirty.is_empty() {
            self.prompt_unsaved(dirty, PendingClose::Workspace, window, cx);
            return;
        }
        if self.has_pending_environment_work() {
            self.pending_close = Some(PendingClose::Workspace);
            self.start_next_environment_save(window, cx);
            return;
        }
        self.close_workspace_now(cx);
    }

    pub(super) fn request_close_window(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.application_dialog.is_some() {
            return false;
        }
        let dirty = self.dirty_keys();
        if !dirty.is_empty() {
            self.prompt_unsaved(dirty, PendingClose::Window, window, cx);
            return false;
        }
        if self.has_pending_environment_work() {
            self.pending_close = Some(PendingClose::Window);
            self.start_next_environment_save(window, cx);
            return false;
        }
        true
    }

    pub(super) fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.shell.toggle_sidebar();
        self.persist_session(cx);
        cx.notify();
    }

    pub(super) fn set_pane_layout(&mut self, layout: PaneLayout, cx: &mut Context<Self>) {
        self.shell.set_pane_layout(layout);
        self.refresh_system_menu(cx);
        self.persist_session(cx);
        cx.notify();
    }

    #[cfg(target_os = "macos")]
    pub(super) fn refresh_system_menu(&self, cx: &mut Context<Self>) {
        cx.set_menus(system_menus(self.shell.pane_layout));
    }

    #[cfg(not(target_os = "macos"))]
    pub(super) fn refresh_system_menu(&self, _: &mut Context<Self>) {}

    pub(super) fn quit_application(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.application_dialog.is_some() {
            return;
        }
        let dirty = self.dirty_keys();
        if !dirty.is_empty() {
            self.prompt_unsaved(dirty, PendingClose::Quit, window, cx);
            return;
        }
        if self.has_pending_environment_work() {
            self.pending_close = Some(PendingClose::Quit);
            self.start_next_environment_save(window, cx);
            return;
        }
        cx.quit();
    }

    pub(super) fn prompt_unsaved(
        &mut self,
        keys: Vec<RequestKey>,
        pending: PendingClose,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pending_close.is_some() {
            return;
        }
        self.show_application_dialog(ApplicationDialog::Unsaved { keys, pending }, window, cx);
    }

    pub(super) fn discard_dirty_requests(&mut self, keys: &[RequestKey]) {
        let Some(loaded) = self.loaded_workspace.as_mut() else {
            return;
        };
        for key in keys {
            let Some(saved) = self.persistence.saved_request(*key).cloned() else {
                continue;
            };
            if let Some(request) = loaded.request_mut(*key) {
                *request = saved;
            }
            self.execution.remove(*key);
            self.response_viewer.remove(*key);
        }
    }

    pub(super) fn save_active_request(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(key) = self.shell.active_tab() {
            let dirty = self
                .loaded_workspace
                .as_ref()
                .and_then(|loaded| loaded.workspace().request(key))
                .is_some_and(|request| self.persistence.is_dirty(key, request));
            if dirty {
                self.persistence.enqueue([key]);
                self.start_next_request_save(window, cx);
            }
        }
    }

    pub(super) fn start_next_request_save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.request_save_task.is_some() || self.environment_save_task.is_some() {
            return;
        }
        let Some(key) = self.persistence.next() else {
            self.finish_pending_close_if_idle(window, cx);
            return;
        };
        let Some(loaded) = &self.loaded_workspace else {
            self.persistence.fail(key);
            return;
        };
        let Some(request) = loaded.workspace().request(key) else {
            self.persistence.fail(key);
            return;
        };
        let Some(selector) = loaded.request_selector(key).map(str::to_owned) else {
            self.persistence.fail(key);
            return;
        };
        let (_revision, snapshot, update) = self.persistence.begin(key, request);
        let prepared = match loaded.prepare_request_save(&selector, update) {
            Ok(prepared) => prepared,
            Err(error) => {
                self.persistence.fail(key);
                self.pending_close = None;
                self.show_toast(
                    ToastIntent::Error,
                    format!("Could not save request: {error}"),
                    cx,
                );
                return;
            }
        };
        self.request_save_task = Some(cx.spawn_in(window, async move |view, window| {
            let result = window
                .background_spawn(async move { prepared.execute() })
                .await;
            let _ = view.update_in(window, |view, window, cx| {
                view.request_save_task = None;
                match result {
                    Ok(saved) => {
                        if let Some(loaded) = view.loaded_workspace.as_mut() {
                            loaded.complete_request_save(saved);
                        }
                        view.persistence.complete(key, snapshot);
                        view.show_toast(ToastIntent::Success, "Request saved.", cx);
                        view.start_next_request_save(window, cx);
                        view.start_next_environment_save(window, cx);
                    }
                    Err(error) => {
                        view.persistence.fail(key);
                        view.pending_close = None;
                        view.show_toast(
                            ToastIntent::Error,
                            format!("Could not save request: {error}"),
                            cx,
                        );
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn finish_pending_close(
        &mut self,
        pending: PendingClose,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pending_close = None;
        match pending {
            PendingClose::Tab(key) => self.close_tab_now(key, cx),
            PendingClose::OtherTabs { keep } => self.close_other_tabs_now(keep, cx),
            PendingClose::Workspace => self.close_workspace_now(cx),
            PendingClose::Window => window.remove_window(),
            PendingClose::Quit => cx.quit(),
            PendingClose::Open {
                path,
                restored_state,
            } => self.load_workspace_path(path, restored_state, window, cx),
            PendingClose::Create { path } => self.create_workspace_path(path, window, cx),
            PendingClose::Import(source) => self.choose_import(source, window, cx),
        }
    }

    pub(super) fn reveal_active_tab(&mut self) {
        self.scroll_active_tab_into_view();
        self.pending_tab_reveal = true;
    }

    pub(super) fn select_and_reveal_active_request_in_sidebar(&mut self) {
        let Some(key) = self.shell.active_tab() else {
            return;
        };
        let is_graphql = self
            .loaded_workspace
            .as_ref()
            .and_then(|loaded| loaded.workspace().request(key))
            .is_some_and(|request| {
                matches!(request.protocol, probe_core::RequestProtocol::Graphql(_))
            });
        self.request_editor.ensure_available_section(is_graphql);
        self.selected_tree_item = Some(WorkspaceItemRef::Request(key));
        self.reveal_request_in_sidebar(key);
    }

    pub(super) fn scroll_active_tab_into_view(&self) {
        let Some(active) = self.shell.active_tab() else {
            return;
        };
        let Some(index) = self.shell.tabs().iter().position(|tab| *tab == active) else {
            return;
        };
        self.tab_bar_scroll.scroll_to_item(index);
    }
}
