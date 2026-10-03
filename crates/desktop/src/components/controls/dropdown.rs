use super::*;

#[derive(Clone)]
struct DropdownAction {
    label: String,
    on_activate: DropdownActionHandler,
}

#[derive(Debug)]
struct DropdownState {
    open: bool,
    highlighted: usize,
}

#[derive(Clone)]
struct DropdownController {
    state: Entity<DropdownState>,
    parent: EntityId,
    trigger_focus: FocusHandle,
    selected_index: usize,
}

impl DropdownController {
    fn highlighted(&self, cx: &App) -> usize {
        self.state.read(cx).highlighted
    }

    fn repaint(&self, cx: &mut App) {
        cx.notify(self.parent);
    }

    fn set_open(&self, open: bool, list_focus: &FocusHandle, window: &mut Window, cx: &mut App) {
        self.state.update(cx, |state, cx| {
            state.open = open;
            if open {
                state.highlighted = self.selected_index;
            }
            cx.notify();
        });
        if open {
            let list_focus = list_focus.clone();
            let parent = self.parent;
            window.defer(cx, move |window, cx| {
                list_focus.focus(window, cx);
                cx.notify(parent);
            });
        } else {
            self.repaint(cx);
        }
    }

    fn toggle_open(&self, list_focus: &FocusHandle, window: &mut Window, cx: &mut App) {
        let open = !self.state.read(cx).open;
        self.set_open(open, list_focus, window, cx);
    }

    fn close(&self, cx: &mut App) {
        self.state.update(cx, |state, cx| {
            state.open = false;
            cx.notify();
        });
        self.repaint(cx);
    }

    fn restore_trigger_focus(&self, window: &mut Window, cx: &mut App) {
        let trigger_focus = self.trigger_focus.clone();
        window.defer(cx, move |window, cx| trigger_focus.focus(window, cx));
    }

    fn close_and_restore_trigger(&self, window: &mut Window, cx: &mut App) {
        self.close(cx);
        self.restore_trigger_focus(window, cx);
    }

    fn move_highlight(&self, delta: i32, len: usize, cx: &mut App) {
        if len == 0 {
            return;
        }
        self.state.update(cx, |state, cx| {
            let next = state.highlighted as i32 + delta;
            state.highlighted = next.rem_euclid(len as i32) as usize;
            cx.notify();
        });
        self.repaint(cx);
    }

    fn set_highlight(&self, index: usize, cx: &mut App) {
        self.state.update(cx, |state, cx| {
            state.highlighted = index;
            cx.notify();
        });
        self.repaint(cx);
    }
}

struct DropdownSelection {
    index: usize,
    label: String,
    color: gpui::Rgba,
}

/// Index, label, and color for the current value.
/// An empty or unmatched value keeps the first row highlighted and shows "None".
fn dropdown_selection<T: PartialEq>(
    value: Option<&T>,
    options: &[(T, String, Option<gpui::Rgba>)],
    fallback_color: gpui::Rgba,
) -> DropdownSelection {
    value
        .and_then(|value| {
            options
                .iter()
                .enumerate()
                .find_map(|(index, (option, label, color))| {
                    (option == value).then(|| DropdownSelection {
                        index,
                        label: label.clone(),
                        color: color.unwrap_or(fallback_color),
                    })
                })
        })
        .unwrap_or(DropdownSelection {
            index: 0,
            label: "None".to_owned(),
            color: fallback_color,
        })
}

fn dropdown_row(
    theme: Theme,
    row_id: String,
    index: usize,
    highlighted: bool,
    color: gpui::Rgba,
    controller: DropdownController,
) -> gpui::Stateful<gpui::Div> {
    let highlight_background = theme.colors.selection.inactive_background;
    let debug_id = row_id.clone();
    div()
        .id(row_id)
        .role(Role::ListBoxOption)
        .when(highlighted, |item| item.aria_active_descendant())
        .w_full()
        .h(px(theme.metrics.control_height))
        .px(px(theme.metrics.spacing_2))
        .flex()
        .items_center()
        .gap(px(theme.metrics.spacing_1))
        .overflow_hidden()
        .rounded(px(theme.metrics.radius_small))
        .text_color(color)
        .cursor_pointer()
        .debug_selector(move || debug_id)
        .when(highlighted, |item| {
            item.bg(highlight_background)
                .border_1()
                .border_color(theme.colors.borders.focused)
        })
        .when(!highlighted, |item| {
            item.border_1().border_color(transparent_black())
        })
        .hover(move |item| item.bg(theme.colors.surfaces.sidebar))
        .on_hover(move |hovered, _, cx| {
            if *hovered {
                controller.set_highlight(index, cx);
            }
        })
}

#[derive(IntoElement)]
struct DropdownOption<T: Clone + Eq + 'static> {
    theme: Theme,
    id: &'static str,
    index: usize,
    value: T,
    label: String,
    color: gpui::Rgba,
    selected: bool,
    highlighted: bool,
    controller: DropdownController,
    on_value_change: DropdownChangeHandler<T>,
}

impl<T: Clone + Eq + 'static> RenderOnce for DropdownOption<T> {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let id = self.id;
        let index = self.index;
        dropdown_row(
            self.theme,
            format!("{id}-item-{index}"),
            index,
            self.highlighted,
            self.color,
            self.controller.clone(),
        )
        .aria_selected(self.selected)
        .on_click({
            let controller = self.controller;
            let value = self.value;
            let on_value_change = self.on_value_change;
            move |_, window, cx| {
                on_value_change(Some(&value), window, cx);
                controller.close_and_restore_trigger(window, cx);
            }
        })
        .child(
            div()
                .flex_none()
                .w(px(14.0))
                .when(!self.selected, |marker| marker.invisible())
                .child("✓"),
        )
        .child(truncated_label(self.label).min_w(px(0.0)).flex_1())
    }
}

#[derive(IntoElement)]
struct DropdownActionItem {
    theme: Theme,
    id: &'static str,
    index: usize,
    action_index: usize,
    label: String,
    highlighted: bool,
    controller: DropdownController,
    on_activate: DropdownActionHandler,
}

impl RenderOnce for DropdownActionItem {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let id = self.id;
        let action_index = self.action_index;
        dropdown_row(
            self.theme,
            format!("{id}-action-{action_index}"),
            self.index,
            self.highlighted,
            self.theme.colors.text.primary,
            self.controller.clone(),
        )
        .on_click({
            let controller = self.controller;
            let on_activate = self.on_activate;
            move |_, window, cx| {
                controller.close_and_restore_trigger(window, cx);
                on_activate(window, cx);
            }
        })
        .child(div().flex_none().w(px(14.0)))
        .child(truncated_label(self.label).min_w(px(0.0)).flex_1())
    }
}

pub(crate) fn dropdown<T: Clone + Eq + 'static>(
    theme: Theme,
    id: &'static str,
    aria_label: &'static str,
    value: Option<T>,
    options: Vec<(T, String)>,
    width: f32,
    on_value_change: impl Fn(Option<&T>, &mut Window, &mut App) + 'static,
) -> ProbeDropdown<T> {
    ProbeDropdown {
        theme,
        id,
        aria_label,
        value,
        options: options
            .into_iter()
            .map(|(value, label)| (value, label, None))
            .collect(),
        width,
        actions: Vec::new(),
        disabled: false,
        on_value_change: Rc::new(on_value_change),
    }
}

pub(crate) fn dropdown_with_option_colors<T: Clone + Eq + 'static>(
    theme: Theme,
    id: &'static str,
    aria_label: &'static str,
    value: Option<T>,
    options: Vec<(T, String, Option<gpui::Rgba>)>,
    width: f32,
    on_value_change: impl Fn(Option<&T>, &mut Window, &mut App) + 'static,
) -> ProbeDropdown<T> {
    ProbeDropdown {
        theme,
        id,
        aria_label,
        value,
        options,
        width,
        actions: Vec::new(),
        disabled: false,
        on_value_change: Rc::new(on_value_change),
    }
}

#[derive(IntoElement)]
pub(crate) struct ProbeDropdown<T: Clone + Eq + 'static> {
    theme: Theme,
    id: &'static str,
    aria_label: &'static str,
    value: Option<T>,
    options: Vec<(T, String, Option<gpui::Rgba>)>,
    width: f32,
    actions: Vec<DropdownAction>,
    disabled: bool,
    on_value_change: DropdownChangeHandler<T>,
}

impl<T: Clone + Eq + 'static> ProbeDropdown<T> {
    pub(crate) fn with_action(
        mut self,
        label: impl Into<String>,
        on_activate: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.actions.push(DropdownAction {
            label: label.into(),
            on_activate: Rc::new(on_activate),
        });
        self
    }

    pub(crate) fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl<T: Clone + Eq + 'static> RenderOnce for ProbeDropdown<T> {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = self.theme;
        let DropdownSelection {
            index: selected_index,
            label: selected_label,
            color: selected_color,
        } = dropdown_selection(
            self.value.as_ref(),
            &self.options,
            theme.colors.text.primary,
        );
        let state =
            window.use_keyed_state(ElementId::from(format!("{}-state", self.id)), cx, |_, _| {
                DropdownState {
                    open: false,
                    highlighted: selected_index,
                }
            });
        state.update(cx, |state, _| {
            let item_count = self.options.len() + self.actions.len();
            if item_count == 0 {
                state.highlighted = 0;
            } else if state.highlighted >= item_count {
                state.highlighted = selected_index.min(item_count - 1);
            }
        });

        let id = self.id;
        let open = state.read(cx).open;
        let highlighted_index = state.read(cx).highlighted;
        let parent = window.current_view();
        let trigger_focus = window
            .use_keyed_state(format!("{id}-trigger-focus"), cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        let list_focus = window
            .use_keyed_state(format!("{id}-list-focus"), cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        let controller = DropdownController {
            state: state.clone(),
            parent,
            trigger_focus: trigger_focus.clone(),
            selected_index,
        };

        let option_count = self.options.len();
        let item_count = option_count + self.actions.len();
        let on_value_change = self.on_value_change.clone();
        let selected_value = self.value;
        let options = self.options;
        let actions = self.actions;
        let list = div()
            .id(format!("{id}-list"))
            .track_focus(&list_focus)
            .role(Role::ListBox)
            .key_context("Select")
            .on_action({
                let controller = controller.clone();
                move |_: &SelectDown, _, cx| controller.move_highlight(1, item_count, cx)
            })
            .on_action({
                let controller = controller.clone();
                move |_: &SelectUp, _, cx| controller.move_highlight(-1, item_count, cx)
            })
            .on_action({
                let controller = controller.clone();
                let on_value_change = on_value_change.clone();
                let options = options.clone();
                let actions = actions.clone();
                move |_: &Confirm, window, cx| {
                    let index = controller.highlighted(cx);
                    if let Some((value, _, _)) = options.get(index) {
                        on_value_change(Some(value), window, cx);
                        controller.close_and_restore_trigger(window, cx);
                    } else if let Some(action) = actions.get(index.saturating_sub(options.len())) {
                        controller.close_and_restore_trigger(window, cx);
                        (action.on_activate)(window, cx);
                    }
                }
            })
            .flex()
            .flex_col()
            .w(px(self.width.max(160.0)))
            .p(px(theme.metrics.spacing_1))
            .rounded(px(theme.metrics.radius_medium))
            .bg(theme.colors.surfaces.overlay)
            .border_1()
            .border_color(theme.colors.borders.standard)
            .children(
                options
                    .into_iter()
                    .enumerate()
                    .map(|(index, (value, label, color))| {
                        let selected = selected_value.as_ref() == Some(&value);
                        DropdownOption {
                            theme,
                            id,
                            index,
                            value,
                            label,
                            color: color.unwrap_or(theme.colors.text.primary),
                            selected,
                            highlighted: index == highlighted_index,
                            controller: controller.clone(),
                            on_value_change: on_value_change.clone(),
                        }
                        .into_any_element()
                    }),
            )
            .when(!actions.is_empty(), |list| {
                list.child(
                    div()
                        .my(px(theme.metrics.spacing_1))
                        .mx(px(theme.metrics.spacing_2))
                        .flex_none()
                        .h(px(1.0))
                        .bg(theme.colors.borders.standard),
                )
            })
            .children(
                actions
                    .into_iter()
                    .enumerate()
                    .map(|(action_index, action)| {
                        let index = option_count + action_index;
                        DropdownActionItem {
                            theme,
                            id,
                            index,
                            action_index,
                            label: action.label,
                            highlighted: index == highlighted_index,
                            controller: controller.clone(),
                            on_activate: action.on_activate,
                        }
                        .into_any_element()
                    }),
            );

        let popup_content = div()
            .occlude()
            .key_context("Select")
            .on_action({
                let controller = controller.clone();
                move |_: &Cancel, window, cx| {
                    cx.stop_propagation();
                    controller.close_and_restore_trigger(window, cx);
                }
            })
            .on_mouse_down_out({
                let controller = controller.clone();
                move |_, _, cx| controller.close(cx)
            })
            .child(list);

        let trigger = Button::new(format!("{id}-trigger"))
            .track_focus(&trigger_focus)
            .accessibility_label(self.aria_label)
            .selected(open)
            .w_full()
            .h(px(theme.metrics.control_height))
            .px(px(theme.metrics.spacing_2))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(theme.metrics.spacing_1))
            .overflow_hidden()
            .rounded(px(theme.metrics.radius_small))
            .bg(theme.colors.surfaces.raised)
            .border_1()
            .border_color(theme.colors.borders.standard)
            .text_color(if self.disabled {
                theme.colors.actions.disabled_foreground
            } else {
                selected_color
            })
            .debug_selector(move || format!("{id}-trigger"))
            .when(!self.disabled, |trigger| {
                trigger.hover(move |trigger| trigger.bg(theme.colors.selection.inactive_background))
            })
            .focus_visible(move |trigger| trigger.border_color(theme.colors.borders.focused))
            .disabled(self.disabled)
            .on_click({
                let controller = controller.clone();
                let list_focus = list_focus.clone();
                let disabled = self.disabled;
                move |_, window, cx| {
                    if disabled {
                        return;
                    }
                    controller.toggle_open(&list_focus, window, cx)
                }
            })
            .child(truncated_label(selected_label).min_w(px(0.0)).flex_1())
            .child(chevron_icon(theme, open));

        let select_root = Select::new(format!("{id}-select"))
            .open(open && !self.disabled)
            .accessibility_label(self.aria_label)
            .focus_handle(&trigger_focus)
            .content_focus_handle(&list_focus)
            .on_open_change({
                let controller = controller.clone();
                let list_focus = list_focus.clone();
                move |next, window, cx| controller.set_open(next, &list_focus, window, cx)
            })
            .child(trigger);

        Popup::new(format!("{id}-popup"), select_root)
            .when(open, |popup| popup.content(popup_content))
    }
}

#[cfg(test)]
mod tests {
    use super::dropdown_selection;
    use gpui::rgba;

    #[test]
    fn selected_option_resolves_index_label_and_color_together() {
        let fallback = rgba(0x111111ff);
        let blue = rgba(0x2244aaff);
        let options = vec![
            ("GET".to_owned(), "GET".to_owned(), None),
            ("POST".to_owned(), "Send".to_owned(), Some(blue)),
        ];

        let missing = dropdown_selection(None, &options, fallback);
        assert_eq!(missing.index, 0);
        assert_eq!(missing.label, "None");
        assert_eq!(missing.color, fallback);

        let unknown = dropdown_selection(Some(&"PUT".to_owned()), &options, fallback);
        assert_eq!(unknown.index, 0);
        assert_eq!(unknown.label, "None");
        assert_eq!(unknown.color, fallback);

        let plain = dropdown_selection(Some(&"GET".to_owned()), &options, fallback);
        assert_eq!(plain.index, 0);
        assert_eq!(plain.label, "GET");
        assert_eq!(plain.color, fallback);

        let colored = dropdown_selection(Some(&"POST".to_owned()), &options, fallback);
        assert_eq!(colored.index, 1);
        assert_eq!(colored.label, "Send");
        assert_eq!(colored.color, blue);
    }
}
