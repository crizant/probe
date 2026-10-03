use std::{cell::Cell, rc::Rc};

use gpui::{
    AppContext as _, Axis, ClipboardItem, Context, Entity, Focusable as _, Image,
    InteractiveElement as _, IntoElement, KeyBinding, Modifiers, MouseButton, Render, ScrollHandle,
    SharedString, TestAppContext, VisualTestContext, div, point, prelude::*, px, size,
};
use gpui_base::{
    Button, Popover,
    input::{Copy, Cut, InputState, Paste, SelectAll, Undo},
};

use super::{
    DropdownButton, EditorInsets, ProbeEditor, SecretTooltipState, VariableContext,
    VariableHoverState, VariableTooltipPresentation, clipboard_has_pasteable_text, dropdown,
    editor_value_needs_refresh, menu_button, pane_splitter,
};
use crate::app::{FocusNextControl, FocusPreviousControl, ShiftTabOrOutdent, TabOrIndent};
use crate::theme::Theme;

struct MenuTestView {
    open: bool,
    activations: usize,
}

struct VariablePopupTestView {
    hover: Entity<VariableHoverState>,
    secret: bool,
    editor_focus: gpui::FocusHandle,
}

impl Render for VariablePopupTestView {
    fn render(&mut self, _window: &mut gpui::Window, cx: &mut Context<Self>) -> impl IntoElement {
        let presentation = VariableTooltipPresentation {
            value: String::new(),
            placeholder: "Variable value",
            editable: !self.secret,
            hint: None,
            secret: self.secret.then_some(SecretTooltipState::Unknown),
        };
        super::variable_tooltip_popup(
            Theme::light(),
            "token".into(),
            presentation,
            self.hover.clone(),
            self.hover.read(cx).value_input.clone(),
            VariableContext::default(),
            self.editor_focus.clone(),
        )
    }
}

#[gpui::test]
fn secret_popup_does_not_focus_its_hidden_value_input(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(400.0), px(240.0)), |window, cx| {
        VariablePopupTestView {
            hover: cx.new(|cx| VariableHoverState::new(window, cx)),
            secret: true,
            editor_focus: cx.focus_handle(),
        }
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(
        visual
            .debug_bounds("variable-tooltip-value-input")
            .is_none()
    );
    let status = visual.debug_bounds("variable-tooltip-create-hint").unwrap();
    visual.simulate_click(status.center(), Modifiers::default());
    visual.run_until_parked();
    window
        .update(cx, |view, window, cx| {
            let input = view.hover.read(cx).value_input.clone();
            assert!(!input.read(cx).focus_handle(cx).is_focused(window));
        })
        .unwrap();
}

struct PlaceholderHarness {
    input: Entity<InputState>,
    placeholder: SharedString,
}

impl Render for PlaceholderHarness {
    fn render(&mut self, _: &mut gpui::Window, _: &mut Context<Self>) -> impl IntoElement {
        let mut input = super::text_input_base(
            Theme::light(),
            "placeholder-input",
            "",
            self.placeholder.clone(),
        );
        input.shared_input = Some(self.input.clone());
        div().size_full().child(input)
    }
}

#[gpui::test]
fn input_placeholder_updates_when_label_or_shared_state_changes(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(320.0), px(180.0)), |window, cx| {
        PlaceholderHarness {
            input: cx.new(|cx| InputState::new(window, cx).placeholder("Name")),
            placeholder: "Name".into(),
        }
    });
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    window
        .update(cx, |view, _, cx| {
            assert_eq!(
                view.input.read(cx).presentation().placeholder().as_ref(),
                "Name"
            );
            view.placeholder = "Value".into();
            cx.notify();
        })
        .unwrap();
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    window
        .update(cx, |view, window, cx| {
            assert_eq!(
                view.input.read(cx).presentation().placeholder().as_ref(),
                "Value"
            );
            // Replacing the shared state must apply the controlled placeholder even
            // when the component's label has not changed.
            view.input = cx.new(|cx| InputState::new(window, cx));
            cx.notify();
        })
        .unwrap();
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    window
        .update(cx, |view, _, cx| {
            assert_eq!(
                view.input.read(cx).presentation().placeholder().as_ref(),
                "Value"
            );
        })
        .unwrap();
}

#[gpui::test]
fn unchanged_placeholder_redraw_does_not_notify_input_state(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    // `set_placeholder` replaces the stored string and then notifies. GPUI does
    // not deliver entity observers for a notify raised while the window is
    // drawing, and painting the input notifies too, so an observer count stays
    // at zero either way. These strings are long enough to be distinct heap
    // allocations; the original allocation remaining is what shows the setter
    // was skipped. The direct call below shows that setter does notify.
    let text = "unchanged-placeholder-".repeat(4);
    let original: SharedString = text.clone().into();
    let label: SharedString = text.into();
    let original_ptr = original.as_ptr();
    let label_ptr = label.as_ptr();
    assert_ne!(original_ptr, label_ptr);
    assert_eq!(original.as_ref(), label.as_ref());

    let window = cx.open_window(size(px(320.0), px(180.0)), |window, cx| {
        PlaceholderHarness {
            input: cx.new(|cx| InputState::new(window, cx).placeholder(original)),
            placeholder: label,
        }
    });
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });

    let notifications = Rc::new(Cell::new(0));
    let observed = notifications.clone();
    let _subscription = window
        .update(cx, |view, _, cx| {
            cx.observe(&view.input, move |_, _, _| {
                observed.set(observed.get() + 1);
            })
        })
        .unwrap();

    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    assert_eq!(notifications.get(), 0);
    window
        .update(cx, |view, _, cx| {
            let presentation = view.input.read(cx).presentation();
            let shown = presentation.placeholder();
            assert_eq!(shown.as_ref(), view.placeholder.as_ref());
            assert_eq!(
                shown.as_ptr(),
                original_ptr,
                "unchanged redraw stored a new placeholder"
            );
            assert_ne!(shown.as_ptr(), label_ptr);
        })
        .unwrap();

    window
        .update(cx, |view, window, cx| {
            let placeholder = view.placeholder.clone();
            view.input.update(cx, |input, cx| {
                input.set_placeholder(placeholder, window, cx);
            });
        })
        .unwrap();
    assert_eq!(notifications.get(), 1);
    window
        .update(cx, |view, _, cx| {
            assert_eq!(
                view.input.read(cx).presentation().placeholder().as_ptr(),
                label_ptr
            );
        })
        .unwrap();
}

struct PersistentInputFocusHarness {
    field: Option<Entity<super::FieldInput>>,
    visible: bool,
    focus_changes: Vec<bool>,
    field_focus_changes: Vec<bool>,
}

impl Render for PersistentInputFocusHarness {
    fn render(&mut self, _: &mut gpui::Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focus_view = cx.weak_entity();
        let field_view = cx.weak_entity();
        let mut input =
            super::text_input_base(Theme::light(), "persistent-focus-input", "value", "Value");
        input.debug_selector = Some("persistent-focus-input");
        input.on_focus = Some(Rc::new(move |focused, cx| {
            let _ = focus_view.update(cx, |view, _| view.focus_changes.push(focused));
        }));
        let input = input.persistent_field(self.field.clone(), move |field, focused, cx| {
            let _ = field_view.update(cx, |view, cx| {
                view.field_focus_changes.push(focused);
                view.field = focused.then_some(field);
                cx.notify();
            });
        });
        div()
            .size_full()
            .when(self.visible, |view| view.child(input))
    }
}

#[gpui::test]
fn virtualizing_a_persistent_input_does_not_report_blur_to_either_focus_callback(
    cx: &mut TestAppContext,
) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(320.0), px(180.0)), |window, _| {
        window.activate_window();
        PersistentInputFocusHarness {
            field: None,
            visible: true,
            focus_changes: Vec::new(),
            field_focus_changes: Vec::new(),
        }
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let bounds = visual.debug_bounds("persistent-focus-input").unwrap();
    visual.simulate_click(bounds.center(), Modifiers::default());
    visual.run_until_parked();
    let focus = window
        .update(cx, |view, window, cx| {
            assert_eq!(view.focus_changes, [true]);
            assert_eq!(view.field_focus_changes, [true]);
            let focus = window.focused(cx).unwrap();
            view.visible = false;
            cx.notify();
            focus
        })
        .unwrap();
    visual.run_until_parked();
    assert!(visual.debug_bounds("persistent-focus-input").is_none());
    window
        .update(cx, |view, window, cx| {
            assert_eq!(window.focused(cx), Some(focus.clone()));
            assert_eq!(view.focus_changes, [true]);
            assert_eq!(view.field_focus_changes, [true]);
            view.visible = true;
            cx.notify();
        })
        .unwrap();
    visual.run_until_parked();
    window
        .update(cx, |view, window, cx| {
            assert_eq!(window.focused(cx), Some(focus));
            assert!(!view.focus_changes.contains(&false));
            assert!(!view.field_focus_changes.contains(&false));
            window.blur();
        })
        .unwrap();
    visual.run_until_parked();
    window
        .update(cx, |view, _, _| {
            assert_eq!(view.focus_changes.last(), Some(&false));
            assert_eq!(view.field_focus_changes.last(), Some(&false));
            assert_eq!(
                view.focus_changes
                    .iter()
                    .filter(|focused| !**focused)
                    .count(),
                1
            );
            assert_eq!(
                view.field_focus_changes
                    .iter()
                    .filter(|focused| !**focused)
                    .count(),
                1
            );
            assert!(view.field.is_none());
        })
        .unwrap();
}

#[derive(Clone, Copy)]
enum TextContextMenuHarnessKind {
    Input,
    BodyEditor,
    ResponseEditor,
}

struct TextContextMenuHarness {
    kind: TextContextMenuHarnessKind,
    input: Option<Entity<InputState>>,
}

struct ScrollableInputHarness {
    input: Entity<InputState>,
    list_scroll: ScrollHandle,
    value: SharedString,
}

impl Render for ScrollableInputHarness {
    fn render(&mut self, _window: &mut gpui::Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let mut input = super::text_input_base(
            Theme::light(),
            "scrollable-input",
            self.value.clone(),
            "Value",
        );
        input.debug_selector = Some("scrollable-input");
        input.shared_input = Some(self.input.clone());
        input.list_scroll = Some(self.list_scroll.clone());
        div()
            .id("scrollable-input-list")
            .w(px(240.0))
            .h(px(120.0))
            .overflow_y_scroll()
            .track_scroll(&self.list_scroll)
            .child(input)
            .child(div().h(px(500.0)))
    }
}

#[gpui::test]
fn horizontal_wheel_scrolls_focused_input_without_scrolling_its_list(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(320.0), px(220.0)), |window, cx| {
        ScrollableInputHarness {
            input: cx.new(|cx| InputState::new(window, cx)),
            list_scroll: ScrollHandle::new(),
            value: "x".repeat(400).into(),
        }
    });
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let field = visual
        .debug_bounds("scrollable-input")
        .expect("the overflowing input should render");
    visual.simulate_click(field.center(), Modifiers::default());
    visual.run_until_parked();
    let (input, list_before, text_before) = window
        .update(cx, |view, window, cx| {
            assert!(view.input.read(cx).focus_handle(cx).is_focused(window));
            assert!(view.list_scroll.max_offset().y > px(0.0));
            (
                view.input.clone(),
                view.list_scroll.offset().y,
                view.input.read(cx).scroll_offset().x,
            )
        })
        .unwrap();

    visual.simulate_event(gpui::ScrollWheelEvent {
        position: field.center(),
        delta: gpui::ScrollDelta::Pixels(point(px(-160.0), px(-6.0))),
        modifiers: Modifiers::default(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    visual.run_until_parked();
    window
        .update(cx, |view, _, cx| {
            let text_after = input.read(cx).scroll_offset().x;
            assert!(
                text_after < text_before,
                "horizontal wheel should move overflowing text, before={text_before:?} after={text_after:?}"
            );
            assert_eq!(view.list_scroll.offset().y, list_before);
        })
        .unwrap();
}

#[gpui::test]
fn vertical_trackpad_frames_scroll_the_list_even_when_one_is_mostly_horizontal(
    cx: &mut TestAppContext,
) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(320.0), px(220.0)), |window, cx| {
        ScrollableInputHarness {
            input: cx.new(|cx| InputState::new(window, cx)),
            list_scroll: ScrollHandle::new(),
            value: "x".repeat(400).into(),
        }
    });
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let field = visual
        .debug_bounds("scrollable-input")
        .expect("the overflowing input should render");
    let (input, list_before, text_before) = window
        .update(cx, |view, _, cx| {
            assert!(view.list_scroll.max_offset().y > px(0.0));
            (
                view.input.clone(),
                view.list_scroll.offset().y,
                view.input.read(cx).scroll_offset().x,
            )
        })
        .unwrap();
    visual.simulate_event(gpui::ScrollWheelEvent {
        position: field.center(),
        delta: gpui::ScrollDelta::Pixels(point(px(-80.0), px(-24.0))),
        modifiers: Modifiers::default(),
        touch_phase: gpui::TouchPhase::Started,
    });
    visual.run_until_parked();
    window
        .update(cx, |view, _, cx| {
            assert_eq!(
                view.list_scroll.offset().y,
                list_before + px(-24.0),
                "an unfocused field should give the vertical component to the list"
            );
            assert_eq!(input.read(cx).scroll_offset().x, text_before);
        })
        .unwrap();

    window
        .update(cx, |view, _, cx| {
            view.list_scroll.set_offset(point(px(0.0), px(0.0)));
            cx.notify();
        })
        .unwrap();
    visual.run_until_parked();
    let field = visual
        .debug_bounds("scrollable-input")
        .expect("the field should return to the top of the list");
    visual.simulate_click(field.center(), Modifiers::default());
    visual.run_until_parked();
    let (list_before, text_before) = window
        .update(cx, |view, window, cx| {
            assert!(view.input.read(cx).focus_handle(cx).is_focused(window));
            (
                view.list_scroll.offset().y,
                view.input.read(cx).scroll_offset().x,
            )
        })
        .unwrap();
    for (phase, delta) in [
        (gpui::TouchPhase::Started, point(px(3.0), px(-36.0))),
        (gpui::TouchPhase::Moved, point(px(-48.0), px(8.0))),
    ] {
        visual.simulate_event(gpui::ScrollWheelEvent {
            position: field.center(),
            delta: gpui::ScrollDelta::Pixels(delta),
            modifiers: Modifiers::default(),
            touch_phase: phase,
        });
    }
    visual.run_until_parked();
    window
        .update(cx, |view, _, cx| {
            assert_eq!(
                view.list_scroll.offset().y,
                list_before + px(-28.0),
                "a sideways frame inside a vertical gesture should still scroll the list"
            );
            assert_eq!(
                input.read(cx).scroll_offset().x,
                text_before,
                "sideways drift should not move text during a vertical gesture"
            );
        })
        .unwrap();
}

#[gpui::test]
fn small_horizontal_trackpad_frames_scroll_focused_text(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(320.0), px(220.0)), |window, cx| {
        ScrollableInputHarness {
            input: cx.new(|cx| InputState::new(window, cx)),
            list_scroll: ScrollHandle::new(),
            value: "x".repeat(400).into(),
        }
    });
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let field = visual
        .debug_bounds("scrollable-input")
        .expect("the overflowing input should render");
    visual.simulate_click(field.center(), Modifiers::default());
    visual.run_until_parked();
    let (input, list_before, text_before) = window
        .update(cx, |view, window, cx| {
            assert!(view.input.read(cx).focus_handle(cx).is_focused(window));
            (
                view.input.clone(),
                view.list_scroll.offset().y,
                view.input.read(cx).scroll_offset().x,
            )
        })
        .unwrap();
    for (index, delta_x) in [-2.0_f32, -3.0, -3.0, -4.0].into_iter().enumerate() {
        visual.simulate_event(gpui::ScrollWheelEvent {
            position: field.center(),
            delta: gpui::ScrollDelta::Pixels(point(px(delta_x), px(0.0))),
            modifiers: Modifiers::default(),
            touch_phase: if index == 0 {
                gpui::TouchPhase::Started
            } else {
                gpui::TouchPhase::Moved
            },
        });
    }
    visual.run_until_parked();
    window
        .update(cx, |view, _, cx| {
            assert!(
                input.read(cx).scroll_offset().x < text_before,
                "several small horizontal frames should scroll overflowing text"
            );
            assert_eq!(view.list_scroll.offset().y, list_before);
        })
        .unwrap();
}

#[gpui::test]
fn reversing_at_the_horizontal_edge_moves_text_immediately(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(320.0), px(220.0)), |window, cx| {
        ScrollableInputHarness {
            input: cx.new(|cx| InputState::new(window, cx)),
            list_scroll: ScrollHandle::new(),
            value: "x".repeat(400).into(),
        }
    });
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let field = visual
        .debug_bounds("scrollable-input")
        .expect("the overflowing input should render");
    visual.simulate_click(field.center(), Modifiers::default());
    visual.run_until_parked();
    let (input, list_before) = window
        .update(cx, |view, window, cx| {
            assert!(view.input.read(cx).focus_handle(cx).is_focused(window));
            (view.input.clone(), view.list_scroll.offset().y)
        })
        .unwrap();
    visual.simulate_event(gpui::ScrollWheelEvent {
        position: field.center(),
        delta: gpui::ScrollDelta::Pixels(point(px(-100_000.0), px(0.0))),
        modifiers: Modifiers::default(),
        touch_phase: gpui::TouchPhase::Started,
    });
    let edge = window
        .update(cx, |view, _, cx| {
            assert_eq!(
                view.list_scroll.offset().y,
                list_before,
                "reaching the horizontal edge should not scroll the list"
            );
            input.read(cx).scroll_offset().x
        })
        .unwrap();
    assert!(
        edge < px(0.0),
        "scrolling to the end of an overflowing value should move its text, offset={edge:?}"
    );
    visual.simulate_event(gpui::ScrollWheelEvent {
        position: field.center(),
        delta: gpui::ScrollDelta::Pixels(point(px(40.0), px(0.0))),
        modifiers: Modifiers::default(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    window
        .update(cx, |view, _, cx| {
            let reversed = input.read(cx).scroll_offset().x;
            assert!(
                reversed > edge,
                "reversing during the same gesture should move off the clamped edge, edge={edge:?} reversed={reversed:?}"
            );
            assert_eq!(view.list_scroll.offset().y, list_before);
        })
        .unwrap();
}

#[test]
fn changing_editor_language_refreshes_unchanged_text() {
    let xml: SharedString = r#"<root id="1"/>"#.into();
    assert!(editor_value_needs_refresh(true, &xml, &xml));
    assert!(!editor_value_needs_refresh(false, &xml, &xml));
    let same_ptr = xml.clone();
    assert!(!editor_value_needs_refresh(false, &xml, &same_ptr));
    let same_text: SharedString = r#"<root id="1"/>"#.into();
    assert!(!editor_value_needs_refresh(false, &xml, &same_text));
}

struct EditableEditorHarness {
    value: SharedString,
    readonly: bool,
    soft_wrap: bool,
    next: Entity<InputState>,
    selection: Option<std::ops::Range<usize>>,
}

impl Render for EditableEditorHarness {
    fn render(&mut self, _window: &mut gpui::Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::light();
        let view = cx.weak_entity();
        div()
            .size_full()
            .p(px(20.0))
            .on_action(|_: &TabOrIndent, window, cx| window.focus_next(cx))
            .on_action(|_: &ShiftTabOrOutdent, window, cx| window.focus_prev(cx))
            .on_action(|_: &FocusNextControl, window, cx| window.focus_next(cx))
            .on_action(|_: &FocusPreviousControl, window, cx| window.focus_prev(cx))
            .flex()
            .flex_col()
            .child(
                ProbeEditor {
                    theme,
                    id: "editable-editor-regression".into(),
                    value: self.value.clone(),
                    placeholder: "Body content".into(),
                    decorations: Vec::new(),
                    language: "json".into(),
                    readonly: self.readonly,
                    min_height: Some(120.0),
                    padding: EditorInsets::standard(theme),
                    soft_wrap: self.soft_wrap,
                    text_color: theme.colors.text.primary,
                    scroll_to_range: self.selection.clone(),
                    search_matches: Vec::new(),
                    on_change: Some(Rc::new(move |value, _, cx| {
                        let _ = view.update(cx, |view, cx| {
                            view.value = value;
                            cx.notify();
                        });
                    })),
                    on_mouse_down: None,
                    on_visible_range: None,
                    extra_context_menu_actions: Vec::new(),
                    debug_selector: Some("editable-editor-regression"),
                    variables: None,
                }
                .into_any_element(),
            )
            .child(
                gpui_base::InputBase::new("tab-next-control")
                    .debug_selector(|| "tab-next-control".into())
                    .child(gpui_base::Input::new(&self.next)),
            )
    }
}

#[gpui::test]
fn editable_editor_preserves_caret_and_undo_history_across_controlled_renders(
    cx: &mut TestAppContext,
) {
    cx.update(crate::theme::Theme::init);
    cx.update(|cx| cx.bind_keys([KeyBinding::new("ctrl-z", Undo, None)]));
    let window = cx.open_window(size(px(420.0), px(220.0)), |window, cx| {
        EditableEditorHarness {
            value: SharedString::default(),
            readonly: false,
            soft_wrap: true,
            next: cx.new(|cx| InputState::new(window, cx)),
            selection: None,
        }
    });
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let editor = visual
        .debug_bounds("editable-editor-regression")
        .expect("editable editor should render");
    visual.simulate_click(editor.center(), Modifiers::default());
    visual.run_until_parked();

    cx.simulate_input(window.into(), "abc");
    cx.run_until_parked();
    cx.simulate_input(window.into(), "d");
    cx.run_until_parked();
    assert_eq!(
        window
            .read_with(cx, |view, _| view.value.clone())
            .expect("test window should remain open"),
        "abcd"
    );

    cx.simulate_keystrokes(window.into(), "ctrl-z");
    cx.run_until_parked();
    assert_eq!(
        window
            .read_with(cx, |view, _| view.value.clone())
            .expect("test window should remain open"),
        ""
    );
}

#[gpui::test]
fn editable_editor_dispatches_tab_actions_and_undo_atomically(cx: &mut TestAppContext) {
    cx.update(crate::theme::Theme::init);
    cx.update(|cx| {
        cx.bind_keys([
            KeyBinding::new("tab", TabOrIndent, None),
            KeyBinding::new("shift-tab", ShiftTabOrOutdent, None),
            KeyBinding::new("ctrl-tab", FocusNextControl, None),
            KeyBinding::new("ctrl-shift-tab", FocusPreviousControl, None),
            KeyBinding::new("ctrl-a", SelectAll, None),
            KeyBinding::new("ctrl-z", Undo, None),
        ])
    });
    let window = cx.open_window(size(px(420.0), px(220.0)), |window, cx| {
        EditableEditorHarness {
            value: "😀\n  beta".into(),
            readonly: false,
            soft_wrap: true,
            next: cx.new(|cx| InputState::new(window, cx)),
            selection: None,
        }
    });
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let bounds = visual
        .debug_bounds("editable-editor-regression")
        .expect("editable editor should render");
    visual.simulate_click(bounds.center(), Modifiers::default());
    visual.run_until_parked();
    let editor_focus = window
        .update(cx, |_, window, cx| window.focused(cx).unwrap())
        .unwrap();

    cx.simulate_keystrokes(window.into(), "ctrl-a tab");
    cx.run_until_parked();
    assert_eq!(
        window.read_with(cx, |view, _| view.value.clone()).unwrap(),
        "  😀\n    beta"
    );

    cx.simulate_keystrokes(window.into(), "ctrl-z");
    cx.run_until_parked();
    assert_eq!(
        window.read_with(cx, |view, _| view.value.clone()).unwrap(),
        "😀\n  beta"
    );

    cx.simulate_keystrokes(window.into(), "ctrl-a shift-tab");
    cx.run_until_parked();
    assert_eq!(
        window.read_with(cx, |view, _| view.value.clone()).unwrap(),
        "😀\nbeta"
    );

    window
        .update(cx, |view, _, cx| {
            view.value = "😀\n  beta".into();
            view.selection = Some(view.value.len()..view.value.len());
            cx.notify();
        })
        .unwrap();
    cx.run_until_parked();
    cx.simulate_keystrokes(window.into(), "shift-tab ctrl-z");
    cx.simulate_input(window.into(), "X");
    cx.run_until_parked();
    assert_eq!(
        window.read_with(cx, |view, _| view.value.clone()).unwrap(),
        "😀\n  betaX",
        "undo should restore the collapsed caret after the outdent"
    );

    cx.simulate_keystrokes(window.into(), "ctrl-tab");
    cx.run_until_parked();
    assert_eq!(
        window.read_with(cx, |view, _| view.value.clone()).unwrap(),
        "😀\n  betaX",
        "Ctrl-Tab navigation must not edit the body"
    );

    window
        .update(cx, |view, window, cx| {
            assert!(view.next.read(cx).focus_handle(cx).is_focused(window));
        })
        .unwrap();
    cx.simulate_keystrokes(window.into(), "ctrl-shift-tab");
    cx.run_until_parked();

    let focused_after_ctrl_tab = window
        .update(cx, |view, window, cx| {
            view.soft_wrap = false;
            cx.notify();
            window.focused(cx)
        })
        .unwrap();
    assert_eq!(focused_after_ctrl_tab, Some(editor_focus));
    cx.run_until_parked();

    cx.simulate_keystrokes(window.into(), "ctrl-a tab");
    cx.run_until_parked();
    assert_eq!(
        window.read_with(cx, |view, _| view.value.clone()).unwrap(),
        "  😀\n    betaX",
        "editable editors should indent even when soft wrapping is disabled"
    );
    cx.simulate_keystrokes(window.into(), "ctrl-z");
    cx.run_until_parked();
    window
        .update(cx, |view, _, cx| {
            view.readonly = true;
            cx.notify();
        })
        .unwrap();
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.simulate_click(bounds.center(), Modifiers::default());
    visual.run_until_parked();
    let readonly_focus = window
        .update(cx, |_, window, cx| window.focused(cx))
        .unwrap();
    cx.simulate_keystrokes(window.into(), "tab");
    cx.run_until_parked();
    let next_focus = window
        .update(cx, |_, window, cx| window.focused(cx))
        .unwrap();
    assert_ne!(next_focus, readonly_focus);
    assert_eq!(
        window.read_with(cx, |view, _| view.value.clone()).unwrap(),
        "😀\n  betaX",
        "readonly Tab should navigate without editing"
    );
}

impl Render for TextContextMenuHarness {
    fn render(&mut self, _window: &mut gpui::Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::light();
        let content = match self.kind {
            TextContextMenuHarnessKind::Input => {
                let mut input = super::text_input_base(
                    theme,
                    "context-input",
                    "https://api.example.com",
                    "URL",
                );
                input.debug_selector = Some("context-input");
                input.shared_input = self.input.clone();
                input.into_any_element()
            }
            TextContextMenuHarnessKind::BodyEditor => ProbeEditor {
                theme,
                id: "context-body-editor".into(),
                value: "{\"ok\":true}".into(),
                placeholder: "Body content".into(),
                decorations: Vec::new(),
                language: "json".into(),
                readonly: false,
                min_height: Some(120.0),
                padding: EditorInsets::standard(theme),
                soft_wrap: true,
                text_color: theme.colors.text.primary,
                scroll_to_range: None,
                search_matches: Vec::new(),
                on_change: None,
                on_mouse_down: None,
                on_visible_range: None,
                extra_context_menu_actions: Vec::new(),
                debug_selector: Some("context-body-editor"),
                variables: Some(VariableContext::default()),
            }
            .into_any_element(),
            TextContextMenuHarnessKind::ResponseEditor => ProbeEditor {
                theme,
                id: "context-response-editor".into(),
                value: "{\"ok\":true}".into(),
                placeholder: SharedString::default(),
                decorations: Vec::new(),
                language: "json".into(),
                readonly: true,
                min_height: Some(120.0),
                padding: EditorInsets::response(theme),
                soft_wrap: false,
                text_color: theme.colors.text.primary,
                scroll_to_range: None,
                search_matches: Vec::new(),
                on_change: None,
                on_mouse_down: None,
                on_visible_range: None,
                extra_context_menu_actions: Vec::new(),
                debug_selector: Some("context-response-editor"),
                variables: None,
            }
            .into_any_element(),
        };
        div()
            .size_full()
            .p(px(20.0))
            .child(div().w_full().h(px(140.0)).child(content))
    }
}

fn open_text_context_menu(
    cx: &mut TestAppContext,
    kind: TextContextMenuHarnessKind,
    target: &'static str,
) -> gpui::WindowHandle<TextContextMenuHarness> {
    let window = cx.open_window(size(px(420.0), px(220.0)), |window, cx| {
        TextContextMenuHarness {
            kind,
            input: (matches!(kind, TextContextMenuHarnessKind::Input))
                .then(|| cx.new(|cx| InputState::new(window, cx))),
        }
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let target = visual
        .debug_bounds(target)
        .expect("text context-menu target should render");
    visual.simulate_mouse_down(target.center(), MouseButton::Right, Modifiers::default());
    visual.simulate_mouse_up(target.center(), MouseButton::Right, Modifiers::default());
    visual.run_until_parked();
    cx.run_until_parked();
    window
}

#[gpui::test]
fn editable_input_and_body_editor_show_editing_context_menu(cx: &mut TestAppContext) {
    cx.update(crate::theme::Theme::init);
    cx.update(|cx| {
        cx.bind_keys([
            KeyBinding::new("ctrl-x", Cut, None),
            KeyBinding::new("ctrl-c", Copy, None),
            KeyBinding::new("ctrl-v", Paste, None),
            KeyBinding::new("ctrl-a", SelectAll, None),
        ]);
    });
    for (kind, target) in [
        (TextContextMenuHarnessKind::Input, "context-input"),
        (
            TextContextMenuHarnessKind::BodyEditor,
            "context-body-editor",
        ),
    ] {
        let window = open_text_context_menu(cx, kind, target);
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        assert!(visual.debug_bounds("text-context-menu").is_some());
        assert!(visual.debug_bounds("text-context-cut").is_some());
        assert!(visual.debug_bounds("text-context-copy").is_some());
        assert!(visual.debug_bounds("text-context-paste").is_some());
        assert!(visual.debug_bounds("text-context-select-all").is_some());
        let shortcuts = window
            .update(cx, |_, window, _| {
                [
                    super::shortcut_label_for_action(window, &Cut),
                    super::shortcut_label_for_action(window, &Copy),
                    super::shortcut_label_for_action(window, &Paste),
                    super::shortcut_label_for_action(window, &SelectAll),
                ]
            })
            .expect("text context-menu window should remain open");
        assert!(shortcuts.iter().all(Option::is_some));
    }
}

#[gpui::test]
fn readonly_response_editor_shows_copy_context_menu(cx: &mut TestAppContext) {
    cx.update(crate::theme::Theme::init);
    let window = open_text_context_menu(
        cx,
        TextContextMenuHarnessKind::ResponseEditor,
        "context-response-editor",
    );
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("text-context-menu").is_some());
    assert!(visual.debug_bounds("text-context-copy").is_some());
    assert!(visual.debug_bounds("text-context-select-all").is_some());
    assert!(visual.debug_bounds("text-context-cut").is_none());
    assert!(visual.debug_bounds("text-context-paste").is_none());
}

#[gpui::test]
fn text_context_menu_only_enables_paste_for_text_clipboard_content(cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.write_to_clipboard(ClipboardItem::new_image(&Image::empty()));
        assert!(!clipboard_has_pasteable_text(cx));

        cx.write_to_clipboard(ClipboardItem::new_string("request body".into()));
        assert!(clipboard_has_pasteable_text(cx));
    });
}

#[gpui::test]
fn context_menu_actions_apply_to_the_target_input(cx: &mut TestAppContext) {
    cx.update(crate::theme::Theme::init);
    let window = open_text_context_menu(cx, TextContextMenuHarnessKind::Input, "context-input");
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let select_all = visual
        .debug_bounds("text-context-select-all")
        .expect("Select All should render");
    visual.simulate_click(select_all.center(), Modifiers::default());
    visual.run_until_parked();
    cx.run_until_parked();

    let input = window
        .update(cx, |view, _, _| {
            view.input.clone().expect("input state should exist")
        })
        .expect("test window should remain open");
    assert_eq!(
        input.read_with(cx, |input, _| input.selected_range()),
        0..23
    );

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let target = visual
        .debug_bounds("context-input")
        .expect("input should remain rendered");
    let selected_text = point(target.left() + px(40.0), target.center().y);
    visual.simulate_mouse_down(selected_text, MouseButton::Right, Modifiers::default());
    visual.simulate_mouse_up(selected_text, MouseButton::Right, Modifiers::default());
    visual.run_until_parked();
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let cut = visual
        .debug_bounds("text-context-cut")
        .expect("Cut should render");
    visual.simulate_click(cut.center(), Modifiers::default());
    visual.run_until_parked();
    cx.run_until_parked();
    assert_eq!(input.read_with(cx, |input, _| input.value()), "");
}

impl Render for MenuTestView {
    fn render(&mut self, _window: &mut gpui::Window, cx: &mut Context<Self>) -> impl IntoElement {
        let open_view = cx.weak_entity();
        let activate_view = cx.weak_entity();

        div().size_full().p(px(20.0)).child(
            Popover::new("menu-test-popover")
                .open(self.open)
                .on_open_change(move |open, _, cx| {
                    let _ = open_view.update(cx, |view, cx| {
                        view.open = *open;
                        cx.notify();
                    });
                })
                .trigger(
                    Button::new("menu-test-trigger")
                        .w(px(100.0))
                        .h(px(28.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .debug_selector(|| "menu-test-trigger".into())
                        .child("Open"),
                )
                .content(move |_, _, _| {
                    div()
                        .id("menu-test-popup")
                        .w(px(180.0))
                        .debug_selector(|| "menu-test-popup".into())
                        .child(menu_button(
                            Theme::light(),
                            "menu-test-item",
                            "Workspace",
                            None,
                            move |_, cx| {
                                let _ = activate_view.update(cx, |view, cx| {
                                    view.activations += 1;
                                    view.open = false;
                                    cx.notify();
                                });
                            },
                        ))
                }),
        )
    }
}

#[gpui::test]
fn controlled_popover_menu_item_activates_on_pointer_press(cx: &mut TestAppContext) {
    cx.update(crate::theme::Theme::init);
    let window = cx.open_window(size(px(320.0), px(180.0)), |_, _| MenuTestView {
        open: false,
        activations: 0,
    });
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let trigger = visual
        .debug_bounds("menu-test-trigger")
        .expect("trigger should be rendered");
    visual.simulate_click(trigger.center(), Modifiers::default());
    visual.run_until_parked();
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let popup = visual
        .debug_bounds("menu-test-popup")
        .expect("popup should be rendered");
    visual.simulate_click(popup.center(), Modifiers::default());
    visual.run_until_parked();
    cx.run_until_parked();

    let (open, activations) = window
        .update(cx, |view, _, _| (view.open, view.activations))
        .expect("test window should remain open");
    assert!(!open);
    assert_eq!(activations, 1);
}

struct DropdownHoverLeakView {
    value: Option<&'static str>,
    underlay_hovered: bool,
}

impl Render for DropdownHoverLeakView {
    fn render(&mut self, _window: &mut gpui::Window, cx: &mut Context<Self>) -> impl IntoElement {
        let select_view = cx.weak_entity();
        let underlay_view = cx.weak_entity();

        div()
            .size_full()
            .p(px(12.0))
            .flex()
            .flex_col()
            .child(dropdown(
                Theme::light(),
                "hover-leak-select",
                "Method",
                self.value,
                vec![
                    ("GET", "GET".to_owned()),
                    ("POST", "POST".to_owned()),
                    ("PUT", "PUT".to_owned()),
                    ("PATCH", "PATCH".to_owned()),
                    ("DELETE", "DELETE".to_owned()),
                ],
                120.0,
                move |value, _, cx| {
                    let value = value.copied();
                    let _ = select_view.update(cx, |view, cx| {
                        view.value = value;
                        cx.notify();
                    });
                },
            ))
            .child(
                div()
                    .id("dropdown-underlay")
                    .flex_1()
                    .w_full()
                    .mt(px(8.0))
                    .debug_selector(|| "dropdown-underlay".into())
                    .hover(|underlay| underlay.bg(Theme::light().colors.surfaces.raised))
                    .on_hover(move |hovered, _, cx| {
                        let hovered = *hovered;
                        let _ = underlay_view.update(cx, |view, cx| {
                            view.underlay_hovered = hovered;
                            cx.notify();
                        });
                    })
                    .child("Underlay"),
            )
    }
}

#[gpui::test]
fn dropdown_menu_does_not_hover_elements_underneath(cx: &mut TestAppContext) {
    cx.update(crate::theme::Theme::init);
    let window = cx.open_window(size(px(360.0), px(280.0)), |_, _| DropdownHoverLeakView {
        value: Some("GET"),
        underlay_hovered: false,
    });
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let underlay = visual
        .debug_bounds("dropdown-underlay")
        .expect("underlay should be rendered");
    visual.simulate_mouse_move(underlay.center(), None, Modifiers::default());
    visual.run_until_parked();
    cx.run_until_parked();
    let hovered = window
        .update(cx, |view, _, _| view.underlay_hovered)
        .expect("test window should remain open");
    assert!(hovered, "underlay should hover when the menu is closed");

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let trigger = visual
        .debug_bounds("hover-leak-select-trigger")
        .expect("select trigger should be rendered");
    visual.simulate_click(trigger.center(), Modifiers::default());
    visual.run_until_parked();
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let item = visual
        .debug_bounds("hover-leak-select-item-3")
        .expect("select item over the underlay should be rendered");
    visual.simulate_mouse_move(item.center(), None, Modifiers::default());
    visual.run_until_parked();
    cx.run_until_parked();

    let hovered = window
        .update(cx, |view, _, _| view.underlay_hovered)
        .expect("test window should remain open");
    assert!(
        !hovered,
        "hovering a dropdown item should not hover the element underneath"
    );

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.simulate_click(point(px(340.0), px(260.0)), Modifiers::default());
    visual.run_until_parked();
    assert!(
        visual.debug_bounds("hover-leak-select-item-3").is_none(),
        "clicking outside should dismiss the dropdown"
    );
}

#[gpui::test]
fn dropdown_opens_from_keyboard_focused_trigger(cx: &mut TestAppContext) {
    cx.update(crate::theme::Theme::init);
    let window = cx.open_window(size(px(360.0), px(280.0)), |_, _| DropdownHoverLeakView {
        value: Some("GET"),
        underlay_hovered: false,
    });
    cx.run_until_parked();

    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let trigger = visual
            .debug_bounds("hover-leak-select-trigger")
            .expect("select trigger should render");
        visual.simulate_click(trigger.center(), Modifiers::default());
        visual.run_until_parked();
    }
    cx.simulate_keystrokes(window.into(), "escape");
    cx.run_until_parked();
    cx.simulate_keystrokes(window.into(), "down");
    cx.run_until_parked();
    cx.simulate_keystrokes(window.into(), "down");
    cx.run_until_parked();
    cx.simulate_keystrokes(window.into(), "enter");
    cx.run_until_parked();

    let value = window
        .update(cx, |view, _, _| view.value)
        .expect("test window should remain open");
    assert_eq!(value, Some("POST"));
}

#[gpui::test]
fn dropdown_keyboard_navigation_selects_and_dismisses(cx: &mut TestAppContext) {
    cx.update(crate::theme::Theme::init);
    let window = cx.open_window(size(px(360.0), px(280.0)), |_, _| DropdownHoverLeakView {
        value: Some("GET"),
        underlay_hovered: false,
    });
    cx.run_until_parked();

    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let trigger = visual
            .debug_bounds("hover-leak-select-trigger")
            .expect("select trigger should render");
        visual.simulate_click(trigger.center(), Modifiers::default());
        visual.run_until_parked();
    }

    cx.simulate_keystrokes(window.into(), "down enter");
    cx.run_until_parked();

    let value = window
        .update(cx, |view, _, _| view.value)
        .expect("test window should remain open");
    assert_eq!(value, Some("POST"));
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(
        visual.debug_bounds("hover-leak-select-item-1").is_none(),
        "keyboard selection should dismiss the dropdown"
    );

    cx.simulate_keystrokes(window.into(), "down");
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(
        visual.debug_bounds("hover-leak-select-item-1").is_some(),
        "trigger should stay focused so the next arrow key reopens the menu"
    );
}

struct SplitterHarness {
    presses: Rc<Cell<usize>>,
}

impl Render for SplitterHarness {
    fn render(&mut self, _: &mut gpui::Window, _: &mut Context<Self>) -> impl IntoElement {
        let presses = self.presses.clone();
        div().size_full().p(px(8.0)).child(
            div()
                .id("splitter-pane")
                .debug_selector(|| "splitter-pane".into())
                .size_full()
                .relative()
                .child(
                    pane_splitter(Theme::light(), "test-splitter", Axis::Horizontal)
                        .debug_selector("test-splitter")
                        .on_mouse_down(move |_, _, _| {
                            presses.set(presses.get() + 1);
                        }),
                ),
        )
    }
}

#[gpui::test]
fn pane_splitter_activates_on_pointer_press(cx: &mut TestAppContext) {
    cx.update(crate::theme::Theme::init);
    let presses = Rc::new(Cell::new(0));
    let window = cx.open_window(size(px(240.0), px(80.0)), {
        let presses = presses.clone();
        move |_, _| SplitterHarness { presses }
    });
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let handle = visual
        .debug_bounds("test-splitter")
        .expect("splitter hit target should render");
    visual.simulate_mouse_down(handle.center(), MouseButton::Left, Modifiers::default());
    visual.simulate_mouse_up(handle.center(), MouseButton::Left, Modifiers::default());
    let pane = visual
        .debug_bounds("splitter-pane")
        .expect("splitter parent pane should render");
    assert_eq!(handle.size.width, px(5.0));
    assert!(handle.size.height > px(10.0));
    assert_eq!(handle.center().x, pane.left());
    assert_eq!(presses.get(), 1);
}

struct HiddenLineSplitterHarness {
    presses: Rc<Cell<usize>>,
}

impl Render for HiddenLineSplitterHarness {
    fn render(&mut self, _: &mut gpui::Window, _: &mut Context<Self>) -> impl IntoElement {
        let presses = self.presses.clone();
        div().size_full().p(px(8.0)).child(
            div()
                .id("hidden-splitter-pane")
                .debug_selector(|| "hidden-splitter-pane".into())
                .size_full()
                .relative()
                .child(
                    pane_splitter(Theme::light(), "hidden-splitter", Axis::Horizontal)
                        .show_line(false)
                        .debug_selector("hidden-splitter")
                        .on_mouse_down(move |_, _, _| {
                            presses.set(presses.get() + 1);
                        }),
                ),
        )
    }
}

#[gpui::test]
fn pane_splitter_without_idle_line_still_exposes_a_hit_target(cx: &mut TestAppContext) {
    cx.update(crate::theme::Theme::init);
    let presses = Rc::new(Cell::new(0));
    let window = cx.open_window(size(px(240.0), px(80.0)), {
        let presses = presses.clone();
        move |_, _| HiddenLineSplitterHarness { presses }
    });
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let handle = visual
        .debug_bounds("hidden-splitter")
        .expect("hidden-line splitter should still render a hit target");
    visual.simulate_mouse_down(handle.center(), MouseButton::Left, Modifiers::default());
    visual.simulate_mouse_up(handle.center(), MouseButton::Left, Modifiers::default());
    let pane = visual
        .debug_bounds("hidden-splitter-pane")
        .expect("hidden-line splitter parent pane should render");
    assert_eq!(handle.size.width, px(5.0));
    assert!(handle.size.height > px(10.0));
    assert_eq!(handle.center().x, pane.left());
    assert_eq!(presses.get(), 1);
}

struct DropdownButtonHarness {
    open: bool,
    primary_clicks: usize,
    menu_activations: usize,
}

impl Render for DropdownButtonHarness {
    fn render(&mut self, _window: &mut gpui::Window, cx: &mut Context<Self>) -> impl IntoElement {
        let primary_view = cx.weak_entity();
        let menu_state_view = cx.weak_entity();
        let activate_view = cx.weak_entity();
        let theme = Theme::light();

        div().size_full().p(px(20.0)).child(
            DropdownButton::new(theme, "dropdown-button-action", "Send", move |_, _, cx| {
                let _ = primary_view.update(cx, |view, cx| {
                    view.primary_clicks += 1;
                    cx.notify();
                });
            })
            .menu_trigger("dropdown-button-trigger", "Send options")
            .open(self.open)
            .on_open_change(move |open, _, cx| {
                let _ = menu_state_view.update(cx, |view, cx| {
                    view.open = *open;
                    cx.notify();
                });
            })
            .menu(
                "dropdown-button-menu",
                div()
                    .id("dropdown-button-popup")
                    .w(px(180.0))
                    .debug_selector(|| "dropdown-button-popup".into())
                    .child(menu_button(
                        theme,
                        "dropdown-button-item",
                        "Send and Save Body…",
                        None,
                        move |_, cx| {
                            let _ = activate_view.update(cx, |view, cx| {
                                view.menu_activations += 1;
                                view.open = false;
                                cx.notify();
                            });
                        },
                    )),
            ),
        )
    }
}

#[gpui::test]
fn dropdown_button_primary_action_does_not_open_the_menu(cx: &mut TestAppContext) {
    cx.update(crate::theme::Theme::init);
    let window = cx.open_window(size(px(320.0), px(180.0)), |_, _| DropdownButtonHarness {
        open: false,
        primary_clicks: 0,
        menu_activations: 0,
    });
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let action = visual
        .debug_bounds("dropdown-button-action")
        .expect("primary action should render");
    visual.simulate_click(action.center(), Modifiers::default());
    visual.run_until_parked();
    cx.run_until_parked();

    let (open, primary_clicks) = window
        .update(cx, |view, _, _| (view.open, view.primary_clicks))
        .expect("test window should remain open");
    assert!(!open);
    assert_eq!(primary_clicks, 1);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("dropdown-button-popup").is_none());
}

#[gpui::test]
fn dropdown_button_menu_trigger_opens_and_activates_an_item(cx: &mut TestAppContext) {
    cx.update(crate::theme::Theme::init);
    let window = cx.open_window(size(px(320.0), px(180.0)), |_, _| DropdownButtonHarness {
        open: false,
        primary_clicks: 0,
        menu_activations: 0,
    });
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let trigger = visual
        .debug_bounds("dropdown-button-trigger")
        .expect("menu trigger should render");
    visual.simulate_click(trigger.center(), Modifiers::default());
    visual.run_until_parked();
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let item = visual
        .debug_bounds("dropdown-button-item")
        .expect("menu item should render");
    visual.simulate_click(item.center(), Modifiers::default());
    visual.run_until_parked();
    cx.run_until_parked();

    let (open, primary_clicks, menu_activations) = window
        .update(cx, |view, _, _| {
            (view.open, view.primary_clicks, view.menu_activations)
        })
        .expect("test window should remain open");
    assert!(!open);
    assert_eq!(primary_clicks, 0);
    assert_eq!(menu_activations, 1);
}
