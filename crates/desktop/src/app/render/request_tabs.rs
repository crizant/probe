use super::*;

impl ProbeApp {
    pub(super) fn render_tabs(
        &self,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let mut active_tab_background: Hsla = theme.colors.actions.accent.into();
        active_tab_background.a = 0.12;
        let mut active_tab_close_hover: Hsla = theme.colors.actions.accent.into();
        active_tab_close_hover.a = 0.18;
        let request_tab_bar_height = theme.metrics.tab_bar_height + 2.0;
        let scroll_drag_view = cx.weak_entity();
        let drop_view = cx.weak_entity();
        let mut tab_strip = Tabs::new("request-tabs-scroll")
            .flex_initial()
            .min_w(px(0.0))
            .h_full()
            .px(px(theme.metrics.spacing_1))
            .flex()
            .items_center()
            .overflow_x_scroll()
            .track_scroll(&self.tab_bar_scroll)
            .on_drag_move(move |event: &DragMoveEvent<TabDrag>, _, cx| {
                let source = event.drag(cx).key;
                let _ = scroll_drag_view.update(cx, |view, cx| {
                    view.on_tab_drag_move(source, event.event.position, event.bounds, cx);
                });
            })
            .on_drop(move |drag: &TabDrag, _, cx| {
                let _ = drop_view.update(cx, |view, cx| view.drop_tab(drag.key, cx));
            })
            .can_drop(|value, _, _| value.downcast_ref::<TabDrag>().is_some());
        let Some(loaded) = &self.loaded_workspace else {
            return div()
                .id("request-tabs")
                .h(px(request_tab_bar_height))
                .w_full()
                .bg(theme.colors.surfaces.raised)
                .border_b_1()
                .border_color(theme.colors.borders.subtle);
        };
        for (index, tab) in self.shell.open_tabs().iter().copied().enumerate() {
            let (id, selector, label, active, dirty) = match tab {
                crate::shell::OpenTab::Request(key) => {
                    let Some(request) = loaded.workspace().request(key) else {
                        continue;
                    };
                    let detached = self.detached_requests.contains(&key);
                    let label = if detached {
                        request
                            .url
                            .as_deref()
                            .filter(|url| !url.trim().is_empty())
                            .unwrap_or("Untitled request")
                    } else {
                        request
                            .metadata
                            .name
                            .as_deref()
                            .unwrap_or("Untitled request")
                    };
                    let request_index = self.shell.tabs().position(|open| open == key).unwrap_or(0);
                    (
                        gpui::ElementId::from(("request-tab", key.slot())),
                        format!("request-tab-{request_index}"),
                        label,
                        self.shell.active_tab() == Some(key),
                        detached || self.persistence.is_dirty(key, request),
                    )
                }
                crate::shell::OpenTab::Overview(overview) => {
                    let label = match overview {
                        crate::shell::OverviewTab::Collection => loaded
                            .workspace()
                            .metadata()
                            .name
                            .as_deref()
                            .unwrap_or("Collection"),
                        crate::shell::OverviewTab::Folder(key) => {
                            let Some(folder) = loaded.workspace().folder(key) else {
                                continue;
                            };
                            folder.metadata.name.as_deref().unwrap_or("Folder")
                        }
                    };
                    let overview_index = self
                        .shell
                        .overview_tabs()
                        .position(|open| open == overview)
                        .unwrap_or(0);
                    (
                        match overview {
                            crate::shell::OverviewTab::Collection => {
                                gpui::ElementId::from("collection-tab")
                            }
                            crate::shell::OverviewTab::Folder(key) => {
                                gpui::ElementId::from(("folder-tab", key.slot()))
                            }
                        },
                        format!("overview-tab-{overview_index}"),
                        label,
                        self.shell.active_overview() == Some(overview),
                        self.overview_target(overview)
                            .and_then(|target| self.overview_drafts.get(&target))
                            .is_some_and(|draft| draft.is_dirty()),
                    )
                }
            };
            let select_view = cx.weak_entity();
            let close_view = cx.weak_entity();
            let context_menu_view = cx.weak_entity();
            let tooltip_hover_view = cx.weak_entity();
            let tooltip_move_view = cx.weak_entity();
            let tooltip_leave_view = cx.weak_entity();
            let middle_close_view = close_view.clone();
            let drag_view = cx.weak_entity();
            let close_hover = if active {
                active_tab_close_hover
            } else {
                theme.colors.actions.disabled.into()
            };
            let drop_before = self.tab_drop_target == Some((tab, true));
            let drop_after = self.tab_drop_target == Some((tab, false));
            let close_id = match tab {
                crate::shell::OpenTab::Request(key) => {
                    gpui::ElementId::from(("close-tab", key.slot()))
                }
                crate::shell::OpenTab::Overview(crate::shell::OverviewTab::Collection) => {
                    gpui::ElementId::from("close-collection-tab")
                }
                crate::shell::OpenTab::Overview(crate::shell::OverviewTab::Folder(key)) => {
                    gpui::ElementId::from(("close-folder-tab", key.slot()))
                }
            };
            tab_strip = tab_strip.child(
                Tab::new(id)
                    .debug_selector(move || selector.clone())
                    .accessibility_label(match tab {
                        crate::shell::OpenTab::Overview(_) => format!("{label} overview"),
                        _ => label.to_owned(),
                    })
                    .selected(active)
                    .set_position(index + 1, self.shell.open_tabs().len())
                    .h(px(request_tab_bar_height))
                    .min_w(px(80.0))
                    .max_w(px(176.0))
                    .pl(px(theme.metrics.spacing_3))
                    .pr(px(theme.metrics.spacing_1))
                    .flex()
                    .items_center()
                    .gap(px(theme.metrics.spacing_1))
                    .overflow_hidden()
                    .rounded_tl(px(theme.metrics.radius_medium))
                    .rounded_tr(px(theme.metrics.radius_medium))
                    .when(drop_before, |tab| {
                        tab.rounded_tl(px(0.0))
                            .border_l_2()
                            .border_color(theme.colors.actions.accent)
                    })
                    .when(drop_after, |tab| {
                        tab.rounded_tr(px(0.0))
                            .border_r_2()
                            .border_color(theme.colors.actions.accent)
                    })
                    .when(active, |tab| {
                        tab.bg(active_tab_background)
                            .border_b_1()
                            .border_color(theme.colors.actions.accent)
                            .text_color(theme.colors.actions.accent)
                    })
                    .when(!active, |tab| {
                        tab.text_color(theme.colors.text.secondary)
                            .hover(move |tab| tab.bg(theme.colors.surfaces.sidebar))
                    })
                    .cursor_pointer()
                    .on_drag(
                        TabDrag {
                            key: tab,
                            label: label.to_owned(),
                            active,
                            theme_mode: self.user_config.theme,
                        },
                        move |drag, _, _, cx| {
                            let preview = drag.clone();
                            let _ = drag_view.update(cx, |view, cx| {
                                view.tab_drag_source = Some(drag.key);
                                view.close_tab_tooltip(drag.key, cx);
                            });
                            cx.new(|_| preview)
                        },
                    )
                    .on_mouse_move(move |event, _, cx| {
                        let _ = tooltip_move_view.update(cx, |view, cx| {
                            view.update_tab_tooltip_position(tab, event.position, cx)
                        });
                    })
                    .on_hover(move |hovered, window, cx| {
                        let _ = if *hovered {
                            tooltip_hover_view.update(cx, |view, cx| {
                                view.open_tab_tooltip(tab, window.mouse_position(), cx)
                            })
                        } else {
                            tooltip_leave_view
                                .update(cx, |view, cx| view.close_tab_tooltip(tab, cx))
                        };
                    })
                    .on_click(move |_, _, cx| {
                        let _ = select_view.update(cx, |view, cx| view.select_open_tab(tab, cx));
                    })
                    .on_mouse_down(MouseButton::Middle, |_, _, cx| cx.stop_propagation())
                    .on_mouse_down(MouseButton::Right, move |event: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        let _ = context_menu_view.update(cx, |view, cx| {
                            view.open_tab_context_menu(tab, event.position, cx);
                        });
                    })
                    .on_aux_click(move |event, window, cx| {
                        if event.is_middle_click() {
                            cx.stop_propagation();
                            let _ = middle_close_view.update(cx, |view, cx| {
                                view.request_close_open_tab(tab, window, cx)
                            });
                        }
                    })
                    .child(components::truncated_label(label.to_owned()).flex_1().when(
                        active && matches!(tab, crate::shell::OpenTab::Request(_)),
                        |label| label.debug_selector(|| "request-tab-label".into()),
                    ))
                    .when(dirty, |tab| {
                        tab.child(
                            div()
                                .flex_none()
                                .w(px(6.0))
                                .h(px(6.0))
                                .rounded(px(3.0))
                                .bg(theme.colors.actions.accent),
                        )
                    })
                    .child(
                        Button::new(close_id)
                            .accessibility_label("Close tab")
                            .focusable(false)
                            .tab_stop(false)
                            .flex_none()
                            .w(px(theme.metrics.icon_standard + 4.0))
                            .h(px(theme.metrics.icon_standard + 4.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(theme.metrics.radius_small))
                            .hover(move |close| close.bg(close_hover))
                            .child(components::close_icon(theme))
                            .on_click(move |_, window, cx| {
                                cx.stop_propagation();
                                let _ = close_view.update(cx, |view, cx| {
                                    view.request_close_open_tab(tab, window, cx)
                                });
                            }),
                    ),
            );
        }

        let mut tabs = div()
            .id("request-tabs")
            .h(px(request_tab_bar_height))
            .w_full()
            .flex()
            .items_center()
            .bg(theme.colors.surfaces.raised)
            .border_b_1()
            .border_color(theme.colors.borders.subtle);
        let menu_view = cx.weak_entity();
        let http_view = cx.weak_entity();
        let graphql_view = cx.weak_entity();
        let add_popup = components::popup_surface(theme, "request-tab-add-popup", 160.0)
            .child(components::menu_button_with_leading(
                theme,
                "request-tab-new-http",
                "HTTP",
                "New HTTP Request",
                components::menu_leading_slot(
                    ADD_MENU_MARKER_WIDTH,
                    components::request_icon(
                        theme,
                        &components::RequestIcon::Http { method: None },
                    ),
                ),
                move |window, cx| {
                    let _ = http_view.update(cx, |view, cx| {
                        view.new_detached_request(probe_core::RequestProtocol::Http, window, cx);
                    });
                },
            ))
            .child(components::menu_button_with_leading(
                theme,
                "request-tab-new-graphql",
                "GraphQL",
                "New GraphQL Request",
                components::menu_leading_slot(
                    ADD_MENU_MARKER_WIDTH,
                    components::request_icon(theme, &components::RequestIcon::Graphql),
                ),
                move |window, cx| {
                    let _ = graphql_view.update(cx, |view, cx| {
                        view.new_detached_request(probe_core::RequestProtocol::Graphql, window, cx);
                    });
                },
            ));
        let add_menu = div().flex_none().child(
            Popover::new("request-tab-add-menu")
                .open(self.transient.request_tab_add_menu_open)
                .on_open_change(move |open, _, cx| {
                    let _ = menu_view.update(cx, |view, cx| {
                        view.transient.request_tab_add_menu_open = *open;
                        cx.notify();
                    });
                })
                .trigger(components::add_menu_button_with_id(
                    theme,
                    "request-tab-add-trigger",
                    "New request tab",
                    self.transient.request_tab_add_menu_open,
                    true,
                ))
                .content(move |_, _, _| add_popup),
        );
        tabs = tabs.child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .h_full()
                .flex()
                .items_center()
                .child(tab_strip)
                .child(add_menu),
        );
        let selected = self.shell.selected_environment().unwrap_or("").to_owned();
        let mut options = vec![(String::new(), "No environment".to_owned())];
        options.extend(
            loaded
                .workspace()
                .environments()
                .iter()
                .map(|environment| (environment.name.clone(), environment.name.clone())),
        );
        let environment_view = cx.weak_entity();
        let create_environment_view = cx.weak_entity();
        let manage_environment_view = cx.weak_entity();
        tabs = tabs.child(
            div().flex_none().px(px(theme.metrics.spacing_2)).child(
                components::dropdown(
                    theme,
                    "request-environment",
                    "Request environment",
                    Some(selected),
                    options,
                    190.0,
                    move |value, _, cx| {
                        let value = value.cloned().unwrap_or_default();
                        let _ = environment_view.update(cx, |view, cx| {
                            view.select_environment((!value.is_empty()).then_some(value), cx);
                        });
                    },
                )
                .with_action("Create environment…", move |window, cx| {
                    let _ = create_environment_view.update(cx, |view, cx| {
                        view.open_create_environment_dialog(window, cx);
                    });
                })
                .with_action("Manage environments…", move |window, cx| {
                    let _ = manage_environment_view.update(cx, |view, cx| {
                        view.open_environment_manager_dialog(window, cx);
                    });
                }),
            ),
        );
        tabs
    }

    pub(in crate::app) fn edit_request(
        &mut self,
        key: RequestKey,
        edit: impl FnOnce(&mut Request),
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
        self.persistence.edited(key);
        cx.notify();
    }

    pub(in crate::app) fn change_body_kind(
        &mut self,
        key: RequestKey,
        kind: BodyEditorKind,
        cx: &mut Context<Self>,
    ) {
        let Some(request) = self
            .loaded_workspace
            .as_mut()
            .and_then(|loaded| loaded.request_mut(key))
        else {
            return;
        };
        self.request_editor.switch_body_kind(key, request, kind);
        self.persistence.edited(key);
        cx.notify();
    }
}
