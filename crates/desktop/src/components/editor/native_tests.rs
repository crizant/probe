use super::*;
use gpui_kit::base::input::{Redo, Undo};
use gpui_kit::{KeyBinding, TestAppContext, VisualTestContext, size};
use std::cell::RefCell;

struct EditorHarness {
    value: SharedString,
    language: SharedString,
    readonly: bool,
    editor: Rc<RefCell<Option<Entity<EditorState>>>>,
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
        *self.editor.borrow_mut() = Some(field.read(cx).state.clone());
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
    cx.update(|cx| {
        cx.bind_keys([
            KeyBinding::new("ctrl-z", Undo, None),
            KeyBinding::new("ctrl-shift-z", Redo, None),
            KeyBinding::new("ctrl-f", Search, None),
        ])
    });
    let window = cx.open_window(size(px(420.0), px(220.0)), |_, _| EditorHarness {
        value: value.to_owned().into(),
        language: language.to_owned().into(),
        readonly,
        editor: Rc::default(),
    });
    cx.run_until_parked();
    let editor = window
        .update(cx, |view, window, cx| {
            let editor = view.editor.borrow().as_ref().unwrap().clone();
            editor.update(cx, |editor, cx| editor.focus(window, cx));
            editor
        })
        .unwrap();
    cx.run_until_parked();
    (window, editor)
}

#[gpui_kit::test]
fn json_pairs_track_caret_closers_deletion_and_undo(cx: &mut TestAppContext) {
    let (window, editor) = setup(cx, "", "json", false);
    for (opener, closer, pair) in [("{", "}", "{}"), ("[", "]", "[]"), ("\"", "\"", "\"\"")] {
        cx.simulate_input(window.into(), opener);
        cx.run_until_parked();
        editor.read_with(cx, |editor, _| {
            assert_eq!(editor.value(), pair);
            assert_eq!(editor.selected_range(), 1..1);
        });
        cx.simulate_keystrokes(window.into(), "ctrl-z");
        cx.run_until_parked();
        assert_eq!(editor.read_with(cx, |editor, _| editor.value()), "");
        cx.simulate_keystrokes(window.into(), "ctrl-shift-z");
        cx.run_until_parked();
        editor.read_with(cx, |editor, _| {
            assert_eq!(editor.value(), pair);
            assert_eq!(editor.selected_range(), 1..1);
        });
        cx.simulate_input(window.into(), closer);
        cx.run_until_parked();
        editor.read_with(cx, |editor, _| {
            assert_eq!(
                editor.value(),
                pair,
                "the tracked closer is overtyped without inserting text"
            );
            assert_eq!(editor.selected_range(), 2..2);
        });
        cx.simulate_keystrokes(window.into(), "left backspace");
        cx.run_until_parked();
        assert_eq!(editor.read_with(cx, |editor, _| editor.value()), "");
        cx.simulate_keystrokes(window.into(), "ctrl-z");
        cx.run_until_parked();
        assert_eq!(editor.read_with(cx, |editor, _| editor.value()), pair);
        cx.simulate_keystrokes(window.into(), "ctrl-shift-z");
        cx.run_until_parked();
        assert_eq!(editor.read_with(cx, |editor, _| editor.value()), "");
    }
}

#[gpui_kit::test]
fn language_changes_keep_pair_tracking_and_exclude_non_json_pairs(cx: &mut TestAppContext) {
    let (window, editor) = setup(cx, "", "json", false);
    cx.simulate_input(window.into(), "[");
    cx.run_until_parked();
    for language in ["xml", "", "json"] {
        window
            .update(cx, |view, _, cx| {
                view.language = language.into();
                cx.notify();
            })
            .unwrap();
        cx.run_until_parked();
        assert_eq!(editor.read_with(cx, |editor, _| editor.value()), "[]");
    }
    // Move the tracked pair through an edit before it, then overtype its closer.
    window
        .update(cx, |_, _, cx| {
            editor.update(cx, |editor, cx| editor.set_selected_range(0..0, cx))
        })
        .unwrap();
    cx.simulate_input(window.into(), "a");
    cx.run_until_parked();
    window
        .update(cx, |_, _, cx| {
            editor.update(cx, |editor, cx| editor.set_selected_range(2..2, cx))
        })
        .unwrap();
    cx.simulate_input(window.into(), "]");
    cx.run_until_parked();
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.value(), "a[]");
        assert_eq!(editor.selected_range(), 3..3);
    });
    for language in ["json", "", "xml", "json", ""] {
        for opener in ["{", "[", "\"", "'", "(", "`"] {
            window
                .update(cx, |view, _, cx| {
                    view.language = language.into();
                    view.value = "".into();
                    cx.notify();
                })
                .unwrap();
            cx.run_until_parked();
            cx.simulate_input(window.into(), opener);
            cx.run_until_parked();
            let expected = match (language, opener) {
                ("json", "{") => "{}",
                ("json", "[") => "[]",
                ("json", "\"") => "\"\"",
                _ => opener,
            };
            assert_eq!(
                editor.read_with(cx, |editor, _| editor.value()),
                expected,
                "language {language:?}, opener {opener:?}"
            );
        }
    }
}

#[gpui_kit::test]
fn custom_search_card_keeps_native_session_active_and_navigates(cx: &mut TestAppContext) {
    let query = format!("token across a wrapped line {}", "x".repeat(120));
    let text = format!("{query}\n{}{query}", "row\n".repeat(5000));
    let (window, editor) = setup(cx, &text, "", true);
    cx.simulate_keystrokes(window.into(), "ctrl-f");
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("editor-search-card").is_some());
    cx.simulate_input(window.into(), &query);
    cx.run_until_parked();
    editor.read_with(cx, |editor, _| {
        let session = editor.search_session();
        assert!(session.is_active());
        assert!(!session.open, "Probe retains its own card chrome");
        assert_eq!(session.matcher.label(), "1/2");
        assert_eq!(session.matcher.matched_ranges()[0], 0..query.len());
    });
    cx.simulate_keystrokes(window.into(), "enter");
    cx.run_until_parked();
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.search_session().matcher.label(), "2/2");
        let visible = editor.visible_row_range().unwrap();
        assert!(
            visible.start > 4900,
            "native navigation reveals the distant occurrence"
        );
        assert!(visible.len() < 100, "large response layout remains bounded");
        assert_eq!(
            editor.value(),
            text,
            "readonly search never edits the document"
        );
    });
    window
        .update(cx, |_, _, cx| {
            editor.update(cx, |editor, cx| {
                editor.previous_search_match(cx);
            })
        })
        .unwrap();
    cx.run_until_parked();
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.search_session().matcher.label()),
        "1/2"
    );
    cx.simulate_keystrokes(window.into(), "escape");
    cx.run_until_parked();
    editor.read_with(cx, |editor, _| {
        assert!(!editor.search_session().is_active())
    });
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("editor-search-card").is_none());
}

// Observe the real Syntect adapter rather than a mock parser or visual constants.
struct ObservedHighlighter {
    inner: Box<dyn gpui_kit::base::input::InputHighlighter>,
    updates: Rc<RefCell<Vec<SharedString>>>,
}
impl gpui_kit::base::input::InputHighlighter for ObservedHighlighter {
    fn language(&self) -> SharedString {
        self.inner.language()
    }
    fn update(
        &mut self,
        edit: Option<gpui_kit::base::input::InputEdit>,
        text: &gpui_kit::base::input::Rope,
        folding: bool,
        window: &mut Window,
        cx: &mut Context<EditorState>,
    ) {
        self.inner.update(edit, text, folding, window, cx);
        self.updates.borrow_mut().push(self.inner.language());
    }
    fn styles(
        &self,
        range: &Range<usize>,
        resolver: &dyn gpui_kit::base::input::HighlightStyleResolver,
    ) -> Vec<(Range<usize>, HighlightStyle)> {
        self.inner.styles(range, resolver)
    }
    fn fold_ranges(
        &self,
        text: &gpui_kit::base::input::Rope,
    ) -> Vec<gpui_kit::base::input::FoldRange> {
        self.inner.fold_ranges(text)
    }
}

#[gpui_kit::test]
fn unchanged_text_switches_syntect_language_without_resetting_history(cx: &mut TestAppContext) {
    let (window, editor) = setup(cx, "", "json", false);
    let updates = Rc::new(RefCell::new(Vec::new()));
    window
        .update(cx, |_, _, cx| {
            editor.update(cx, |editor, cx| {
                let updates = updates.clone();
                let factory = crate::syntax::factory();
                editor.set_highlighter_factory(
                    Rc::new(move |language| {
                        factory(language).map(|inner| {
                            Box::new(ObservedHighlighter {
                                inner,
                                updates: updates.clone(),
                            }) as Box<_>
                        })
                    }),
                    cx,
                );
            })
        })
        .unwrap();
    cx.simulate_input(window.into(), "<root id=\"1\"/>");
    cx.run_until_parked();
    assert!(updates.borrow().iter().any(|language| language == "json"));
    updates.borrow_mut().clear();
    let caret = editor.read_with(cx, |editor, _| editor.selected_range());
    window
        .update(cx, |view, _, cx| {
            view.language = "xml".into();
            cx.notify();
        })
        .unwrap();
    cx.run_until_parked();
    assert!(
        updates.borrow().iter().any(|language| language == "xml"),
        "native set_highlighter reparses unchanged bytes"
    );
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.selected_range()),
        caret
    );
    cx.simulate_keystrokes(window.into(), "ctrl-z");
    cx.run_until_parked();
    assert_ne!(
        editor.read_with(cx, |editor, _| editor.value()),
        "<root id=\"1\"/>",
        "language changes preserve the ability to undo the last native edit"
    );
    cx.simulate_keystrokes(window.into(), "ctrl-shift-z");
    cx.run_until_parked();
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.value()),
        "<root id=\"1\"/>"
    );
}
