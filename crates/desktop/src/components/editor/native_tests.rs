use super::*;
use gpui_kit::base::input::Undo;
use gpui_kit::{KeyBinding, TestAppContext, VisualTestContext, size};
use std::cell::RefCell;

struct EditorHarness {
    value: SharedString,
    language: SharedString,
    readonly: bool,
    editor: Option<Entity<EditorState>>,
}

impl Render for EditorHarness {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.weak_entity();
        let element = ProbeEditor {
            theme: Theme::light(),
            id: "native-editor-test".into(),
            value: self.value.clone(),
            placeholder: SharedString::default(),
            decorations: Vec::new(),
            language: self.language.clone(),
            readonly: self.readonly,
            disabled_appearance: false,
            min_height: None,
            padding: EditorInsets::standard(Theme::light()),
            soft_wrap: true,
            text_color: Theme::light().colors.text.primary,
            scroll_to_range: None,
            on_change: Some(Rc::new(move |value, _, cx| {
                view.update(cx, |view, cx| {
                    view.value = value;
                    cx.notify();
                })
                .unwrap();
            })),
            on_mouse_down: None,
            on_visible_range: None,
            extra_context_menu_actions: Vec::new(),
            debug_selector: Some("native-editor-test"),
            variables: None,
        }
        .render(window, cx)
        .into_any_element();
        let field: Entity<EditorField> =
            window.use_keyed_state("native-editor-test", cx, |_, _| {
                panic!("ProbeEditor should have installed its state")
            });
        self.editor = Some(field.read(cx).state.clone());
        element
    }
}

fn setup(
    cx: &mut TestAppContext,
    value: &str,
    language: &str,
    readonly: bool,
) -> (gpui_kit::WindowHandle<EditorHarness>, Entity<EditorState>) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(420.0), px(220.0)), |_, _| EditorHarness {
        value: value.to_owned().into(),
        language: language.to_owned().into(),
        readonly,
        editor: None,
    });
    cx.run_until_parked();
    let editor = window
        .update(cx, |view, window, cx| {
            let editor = view.editor.as_ref().unwrap().clone();
            editor.update(cx, |editor, cx| {
                editor.set_selected_range(value.len()..value.len(), cx);
                editor.focus(window, cx);
            });
            editor
        })
        .unwrap();
    cx.run_until_parked();
    (window, editor)
}

fn assert_input(cx: &mut TestAppContext, language: &str, case: [&str; 3]) {
    let [initial, input, expected] = case;
    let (window, editor) = setup(cx, initial, language, false);
    cx.simulate_input(window.into(), input);
    cx.run_until_parked();
    assert_eq!(editor.read_with(cx, |editor, _| editor.value()), expected);
}

#[gpui_kit::test]
fn probe_json_config_enables_expected_pairs(cx: &mut TestAppContext) {
    for (input, expected) in [
        ("{", "{}"),
        ("[", "[]"),
        ("\"", "\"\""),
        ("(", "("),
        ("'", "'"),
        ("`", "`"),
    ] {
        assert_input(cx, "json", ["", input, expected]);
    }
}

#[gpui_kit::test]
fn plain_and_xml_disable_auto_pairs(cx: &mut TestAppContext) {
    for language in ["", "xml"] {
        for input in ["{", "[", "\"", "(", "'", "`"] {
            assert_input(cx, language, ["", input, input]);
        }
    }
}

#[gpui_kit::test]
fn json_context_prevents_duplicate_closing_quote(cx: &mut TestAppContext) {
    assert_input(cx, "json", ["\"hello ", "\"", "\"hello \""]);
    assert_input(cx, "json", ["\"hello ", "{", "\"hello {"]);
}

#[gpui_kit::test]
fn probe_editor_enables_smart_indent_only_for_json(cx: &mut TestAppContext) {
    for (language, expected) in [("", "{\n}"), ("xml", "{\n}"), ("json", "{\n  \n}")] {
        let (window, editor) = setup(cx, "{}", language, false);
        window
            .update(cx, |_, _, cx| {
                editor.update(cx, |editor, cx| {
                    editor.set_selected_range(1..1, cx);
                })
            })
            .unwrap();
        cx.simulate_keystrokes(window.into(), "enter");
        cx.run_until_parked();
        assert_eq!(editor.read_with(cx, |editor, _| editor.value()), expected);
    }
}

#[gpui_kit::test]
fn language_switch_updates_smart_indent_without_reset(cx: &mut TestAppContext) {
    cx.update(|cx| cx.bind_keys([KeyBinding::new("ctrl-z", Undo, None)]));
    let (window, editor) = setup(cx, "{}", "json", false);
    window
        .update(cx, |_, _, cx| {
            editor.update(cx, |editor, cx| editor.set_selected_range(1..1, cx));
        })
        .unwrap();
    for (language, expected) in [
        ("xml", "{\n}"),
        ("json", "{\n  \n}"),
        ("", "{\n}"),
        ("json", "{\n  \n}"),
    ] {
        window
            .update(cx, |view, _, cx| {
                view.language = language.into();
                cx.notify();
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |view, _, _| {
                assert_eq!(view.editor.as_ref().unwrap(), &editor);
            })
            .unwrap();
        editor.read_with(cx, |editor, _| {
            assert_eq!(editor.value(), "{}");
            assert_eq!(editor.selected_range(), 1..1);
        });
        cx.simulate_keystrokes(window.into(), "enter");
        cx.run_until_parked();
        assert_eq!(editor.read_with(cx, |editor, _| editor.value()), expected);
        cx.simulate_keystrokes(window.into(), "ctrl-z");
        cx.run_until_parked();
    }
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.value(), "{}");
        assert_eq!(editor.selected_range(), 1..1);
    });
}

#[gpui_kit::test]
fn probe_search_card_uses_native_session(cx: &mut TestAppContext) {
    cx.update(|cx| cx.bind_keys([KeyBinding::new("ctrl-f", Search, None)]));
    let text = format!("needle\n{}needle", "row\n".repeat(60));
    let (window, editor) = setup(cx, &text, "", true);
    cx.simulate_keystrokes(window.into(), "ctrl-f");
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("editor-search-card").is_some());
    cx.simulate_input(window.into(), "needle");
    cx.run_until_parked();
    let before = editor.read_with(cx, |editor, _| {
        assert!(editor.search_session().is_active());
        assert_eq!(editor.search_session().query, "needle");
        editor.scroll_offset()
    });
    cx.simulate_keystrokes(window.into(), "enter");
    cx.run_until_parked();
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.search_session().matcher.current_match_index(), 1);
        assert_ne!(editor.scroll_offset(), before);
    });
}

#[gpui_kit::test]
fn language_switch_refreshes_syntect_without_reset(cx: &mut TestAppContext) {
    cx.update(|cx| cx.bind_keys([KeyBinding::new("ctrl-z", Undo, None)]));
    let (window, editor) = setup(cx, "", "json", false);
    let languages = Rc::new(RefCell::new(Vec::new()));
    window
        .update(cx, |_, _, cx| {
            editor.update(cx, |editor, cx| {
                let languages = languages.clone();
                let factory = crate::syntax::factory();
                editor.set_highlighter_factory(
                    Rc::new(move |language| {
                        languages.borrow_mut().push(language.to_owned());
                        factory(language)
                    }),
                    cx,
                );
            })
        })
        .unwrap();
    cx.simulate_input(window.into(), "<root/>");
    cx.run_until_parked();
    assert!(languages.borrow().iter().any(|language| language == "json"));
    languages.borrow_mut().clear();
    let caret = editor.read_with(cx, |editor, _| editor.selected_range());
    window
        .update(cx, |view, _, cx| {
            view.language = "xml".into();
            cx.notify();
        })
        .unwrap();
    cx.run_until_parked();
    assert!(languages.borrow().iter().any(|language| language == "xml"));
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.value(), "<root/>");
        assert_eq!(editor.selected_range(), caret);
    });
    cx.simulate_keystrokes(window.into(), "ctrl-z");
    cx.run_until_parked();
    assert_ne!(editor.read_with(cx, |editor, _| editor.value()), "<root/>");
}
