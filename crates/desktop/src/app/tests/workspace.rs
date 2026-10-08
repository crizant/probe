use super::*;
use crate::app::{NewGraphqlRequest, NewRequest, documentation::OverviewTarget};
use probe_core::ItemKind;
use probe_opencollection::ItemLocator;

#[gpui::test]
fn request_creation_dialog_uses_the_selected_protocol(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = bundled_fixture().canonicalize().unwrap();
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    window
        .update(cx, |view, _, _| {
            view.session_store = None;
            view.set_workspace(fixture, workspace);
        })
        .unwrap();
    cx.run_until_parked();
    for (action, protocol) in [
        (
            Box::new(NewRequest) as Box<dyn gpui::Action>,
            probe_core::RequestProtocol::Http,
        ),
        (
            Box::new(NewGraphqlRequest) as Box<dyn gpui::Action>,
            probe_core::RequestProtocol::Graphql,
        ),
    ] {
        window
            .update(cx, |_, window, cx| {
                window.dispatch_action(action, cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |view, _, _| {
                let dialog = view.structure_dialog.as_ref().unwrap();
                assert_eq!(dialog.mode, StructureDialogMode::CreateRequest(protocol));
            })
            .unwrap();
    }
}

#[gpui::test]
fn new_request_tabs_are_in_memory_and_editable(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = bundled_fixture().canonicalize().unwrap();
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture, workspace);
            let original_count = view.loaded_workspace.as_ref().unwrap().requests().len();
            view.new_detached_request(probe_core::RequestProtocol::Http, window, cx);
            let http_key = view.shell.active_tab().unwrap();
            assert_eq!(
                view.active_request().unwrap().method.as_deref(),
                Some("GET")
            );
            assert!(view.request_is_dirty(http_key));
            view.edit_request(
                http_key,
                |request| request.url = Some("https://example.test".to_owned()),
                cx,
            );
            assert_eq!(
                view.active_request().unwrap().url.as_deref(),
                Some("https://example.test")
            );
            view.new_detached_request(probe_core::RequestProtocol::Graphql, window, cx);
            assert!(view.active_request().unwrap().kind.is_graphql());
            assert_eq!(
                view.active_request().unwrap().method.as_deref(),
                Some("POST")
            );
            assert_eq!(
                view.request_editor
                    .section(view.shell.active_tab().unwrap()),
                EditorSection::Path
            );
            assert_eq!(
                view.loaded_workspace.as_ref().unwrap().requests().len(),
                original_count
            );
            assert_eq!(view.dirty_keys().len(), 2);
            view.close_tab_now(http_key, cx);
            assert!(
                view.loaded_workspace
                    .as_ref()
                    .unwrap()
                    .workspace()
                    .request(http_key)
                    .is_none()
            );
            assert_eq!(view.dirty_keys().len(), 1);
        })
        .unwrap();
}

#[gpui::test]
fn request_editor_lists_scroll_when_the_pointer_is_over_a_field(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = bundled_fixture().canonicalize().unwrap();
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture, workspace);
            view.new_detached_request(probe_core::RequestProtocol::Http, window, cx);
            let key = view.shell.active_tab().unwrap();
            view.request_editor.set_section(key, EditorSection::Headers);
            view.edit_request(
                key,
                |request| {
                    for index in 0..40 {
                        request.headers.push(probe_core::Header {
                            name: format!("Header-{index}"),
                            value: format!("value-{index}"),
                            disabled: false,
                        });
                        request.query_parameters.push(QueryParameter {
                            name: format!("query-{index}"),
                            value: format!("value-{index}"),
                            disabled: false,
                        });
                        request.path_parameters.push(QueryParameter {
                            name: format!("path-{index}"),
                            value: format!("value-{index}"),
                            disabled: false,
                        });
                    }
                },
                cx,
            );
        })
        .unwrap();
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    for (section, selector, value_selector) in [
        (
            EditorSection::Headers,
            "header-name-field",
            "header-value-field",
        ),
        (EditorSection::Query, "query-name-row", "query-name-row"),
        (EditorSection::Path, "path-name-row", "path-name-row"),
    ] {
        for value_column in [false, true] {
            window
                .update(cx, |view, _, cx| {
                    let key = view.shell.active_tab().unwrap();
                    view.request_editor.set_section(key, section);
                    view.request_section_scroll
                        .set_offset(point(px(0.0), px(0.0)));
                    cx.notify();
                })
                .unwrap();
            visual.run_until_parked();
            let field = visual
                .debug_bounds(if value_column {
                    value_selector
                } else {
                    selector
                })
                .unwrap();
            let position = field.origin
                + point(
                    field.size.width
                        * if value_column && section != EditorSection::Headers {
                            0.6
                        } else {
                            0.25
                        },
                    field.size.height / 2.0,
                );
            // Each adapter only needs to prove that its field reaches the shared coordinator.
            visual.simulate_event(gpui::ScrollWheelEvent {
                position,
                delta: gpui::ScrollDelta::Pixels(point(px(2.0), px(-8.0))),
                modifiers: Modifiers::default(),
                touch_phase: gpui::TouchPhase::Started,
            });
            assert_eq!(
                window
                    .update(cx, |view, _, _| view.request_section_scroll.offset().y)
                    .unwrap(),
                px(-8.0),
                "scroll wiring for {section:?}, value column: {value_column}"
            );
        }
    }
}

#[gpui::test]
fn key_value_editor_row_controls_update_their_rows_and_restore_focus(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    cx.update(bind_platform_hotkeys);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = bundled_fixture().canonicalize().unwrap();
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture, workspace);
            view.new_detached_request(probe_core::RequestProtocol::Http, window, cx);
            let key = view.shell.active_tab().unwrap();
            view.edit_request(
                key,
                |request| {
                    request.url = Some("https://example.test/:id".to_owned());
                    request.path_parameters.push(QueryParameter {
                        name: "id".into(),
                        value: "123".into(),
                        disabled: false,
                    });
                },
                cx,
            );
            view.request_editor.set_section(key, EditorSection::Query);
        })
        .unwrap();
    cx.run_until_parked();

    // Query exercises every shared callback once; other kinds only need adapter checks.
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let add = visual.debug_bounds("add-query-parameter").unwrap();
    visual.simulate_click(add.center(), Modifiers::default());
    visual.run_until_parked();
    window
        .update(cx, |view, _, _| {
            assert_eq!(view.active_request().unwrap().query_parameters.len(), 1);
        })
        .unwrap();

    // From Add, traverse Remove, Enabled, Value, then Name.
    cx.simulate_keystrokes(window.into(), "shift-tab shift-tab shift-tab shift-tab");
    cx.simulate_input(window.into(), "edited");
    cx.run_until_parked();
    cx.simulate_keystrokes(window.into(), "tab");
    cx.simulate_input(window.into(), "edited-value");
    cx.run_until_parked();
    cx.simulate_keystrokes(window.into(), "tab");
    visual.run_until_parked();
    let keystroke = gpui::Keystroke::parse("space").unwrap();
    visual.simulate_event(gpui::KeyDownEvent {
        keystroke: keystroke.clone(),
        is_held: false,
        prefer_character_input: false,
    });
    visual.simulate_event(gpui::KeyUpEvent { keystroke });
    visual.run_until_parked();
    window
        .update(cx, |view, _, _| {
            let row = &view.active_request().unwrap().query_parameters[0];
            assert_eq!(row.name, "edited");
            assert_eq!(row.value, "edited-value");
            assert!(row.disabled);
        })
        .unwrap();
    let remove = visual.debug_bounds("remove-query-0").unwrap();
    visual.simulate_click(remove.center(), Modifiers::default());
    visual.run_until_parked();
    window
        .update(cx, |view, window, _| {
            assert!(view.focus_handle.is_focused(window));
            assert!(view.active_request().unwrap().query_parameters.is_empty());
        })
        .unwrap();

    // Only check Path-specific routing here; core tests own the URL-sync semantics.
    window
        .update(cx, |view, _, cx| {
            let key = view.shell.active_tab().unwrap();
            view.request_editor.set_section(key, EditorSection::Path);
            cx.notify();
        })
        .unwrap();
    visual.run_until_parked();
    let row = visual.debug_bounds("path-name-row").unwrap();
    visual.simulate_click(
        row.origin + point(row.size.width / 4.0, row.size.height / 2.0),
        Modifiers::default(),
    );
    cx.simulate_keystrokes(
        window.into(),
        if cfg!(target_os = "macos") {
            "cmd-a"
        } else {
            "ctrl-a"
        },
    );
    cx.simulate_input(window.into(), "renamed");
    visual.run_until_parked();
    window
        .update(cx, |view, _, _| {
            assert_eq!(
                view.active_request().unwrap().url.as_deref(),
                Some("https://example.test/:renamed")
            );
        })
        .unwrap();

    // Headers are rendered by the existing scrolling test; Form needs only a smoke check.
    window
        .update(cx, |view, _, cx| {
            let key = view.shell.active_tab().unwrap();
            view.change_body_kind(key, BodyEditorKind::Form, cx);
            view.request_editor.set_section(key, EditorSection::Body);
            cx.notify();
        })
        .unwrap();
    visual.run_until_parked();
    assert!(visual.debug_bounds("add-form-field").is_some());
}

#[gpui::test]
fn saving_detached_request_preserves_edited_fields(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_structure_fixture("save-detached-request");
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), workspace);
            view.new_detached_request(probe_core::RequestProtocol::Http, window, cx);
            let key = view.shell.active_tab().unwrap();
            view.edit_request(
                key,
                |request| {
                    request.method = Some("POST".to_owned());
                    request.url = Some("https://example.test/create".to_owned());
                    request.headers.push(probe_core::Header {
                        name: "X-Test".to_owned(),
                        value: "yes".to_owned(),
                        disabled: false,
                    });
                },
                cx,
            );
            view.persist_detached_request(key, "Created".to_owned(), None, window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    let loaded = probe_opencollection::load_workspace(&fixture).unwrap();
    let created = loaded
        .requests()
        .iter()
        .find_map(|located| {
            let request = loaded.workspace().request(located.key())?;
            (request.metadata.name.as_deref() == Some("Created")).then_some(request)
        })
        .expect("request should be saved");
    assert_eq!(created.method.as_deref(), Some("POST"));
    assert_eq!(created.url.as_deref(), Some("https://example.test/create"));
    assert_eq!(created.headers[0].name, "X-Test");
}

#[gpui::test]
fn saving_detached_request_keeps_the_persisted_sequence(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/opencollection/phase16-unbundled");
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let fixture = std::env::temp_dir().join(format!(
        "probe-desktop-unbundled-{}-{unique}-save-sequence",
        std::process::id()
    ));
    copy_unbundled_fixture(&source, &fixture);
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), workspace);
            view.new_detached_request(probe_core::RequestProtocol::Http, window, cx);
            let key = view.shell.active_tab().unwrap();
            view.edit_request(
                key,
                |request| {
                    request.method = Some("POST".to_owned());
                    request.url = Some("https://example.test/create".to_owned());
                },
                cx,
            );
            view.persist_detached_request(key, "Created".to_owned(), None, window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    let loaded = probe_opencollection::load_workspace(&fixture).unwrap();
    let created = loaded
        .requests()
        .iter()
        .find_map(|located| {
            let request = loaded.workspace().request(located.key())?;
            (request.metadata.name.as_deref() == Some("Created")).then_some(request)
        })
        .expect("request should be saved");
    assert!(created.metadata.sequence.is_some());
    window
        .update(cx, |view, _, _| {
            let saved_key = view
                .shell
                .tabs()
                .find(|key| {
                    view.loaded_workspace
                        .as_ref()
                        .and_then(|loaded| loaded.workspace().request(*key))
                        .and_then(|request| request.metadata.name.clone())
                        .as_deref()
                        == Some("Created")
                })
                .expect("saved request should stay open");
            let saved = view
                .loaded_workspace
                .as_ref()
                .unwrap()
                .workspace()
                .request(saved_key)
                .unwrap();
            assert_eq!(saved.metadata, created.metadata);
            assert!(!view.request_is_dirty(saved_key));
        })
        .unwrap();
    let _ = fs::remove_dir_all(fixture);
}

fn copy_unbundled_fixture(source: &std::path::Path, destination: &std::path::Path) {
    fs::create_dir(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.path().is_dir() {
            copy_unbundled_fixture(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

#[gpui::test]
fn saving_detached_graphql_request_preserves_query(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_structure_fixture("save-detached-graphql-request");
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), workspace);
            view.new_detached_request(probe_core::RequestProtocol::Graphql, window, cx);
            let key = view.shell.active_tab().unwrap();
            view.edit_request(
                key,
                |request| {
                    request.url = Some("https://example.test/graphql".to_owned());
                    request
                        .apply_graphql_update(&probe_core::GraphqlUpdate {
                            query: probe_core::FieldPatch::Set(
                                "query Viewer { viewer { id } }".to_owned(),
                            ),
                            ..probe_core::GraphqlUpdate::default()
                        })
                        .unwrap();
                },
                cx,
            );
            view.persist_detached_request(key, "Viewer".to_owned(), None, window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    let loaded = probe_opencollection::load_workspace(&fixture).unwrap();
    let created = loaded
        .requests()
        .iter()
        .find_map(|located| {
            let request = loaded.workspace().request(located.key())?;
            (request.metadata.name.as_deref() == Some("Viewer")).then_some(request)
        })
        .expect("GraphQL request should be saved");
    assert!(created.kind.is_graphql());
    assert_eq!(
        created
            .selected_graphql()
            .unwrap()
            .unwrap()
            .query
            .as_deref(),
        Some("query Viewer { viewer { id } }")
    );
}

#[gpui::test]
fn saving_background_draft_keeps_the_user_selected_tab(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_structure_fixture("save-background-draft-selection");
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    let existing_selector = workspace.requests()[0].selector().to_owned();
    let existing_key = workspace.requests()[0].key();
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), workspace);
            view.new_detached_request(probe_core::RequestProtocol::Http, window, cx);
            let draft_key = view.shell.active_tab().unwrap();
            view.persist_detached_request(draft_key, "Background".to_owned(), None, window, cx);
            view.select_request(existing_key, cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |view, _, _| {
            let loaded = view.loaded_workspace.as_ref().unwrap();
            assert_eq!(
                view.shell.active_tab(),
                loaded.request_key(&existing_selector)
            );
        })
        .unwrap();
}

#[gpui::test]
fn saving_detached_request_keeps_unrelated_request_dirty(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_structure_fixture("save-detached-keeps-dirty");
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    let existing_selector = workspace.requests()[0].selector().to_owned();
    let existing_key = workspace.requests()[0].key();
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), workspace);
            view.edit_request(
                existing_key,
                |request| {
                    request.url = Some("https://local.example/unsaved".to_owned());
                },
                cx,
            );
            view.new_detached_request(probe_core::RequestProtocol::Http, window, cx);
            let draft_key = view.shell.active_tab().unwrap();
            view.persist_detached_request(draft_key, "Saved Draft".to_owned(), None, window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |view, _, _| {
            let loaded = view.loaded_workspace.as_ref().unwrap();
            let key = loaded.request_key(&existing_selector).unwrap();
            let request = loaded.workspace().request(key).unwrap();
            assert_eq!(
                request.url.as_deref(),
                Some("https://local.example/unsaved")
            );
            assert!(view.persistence.is_dirty(key, request));
        })
        .unwrap();
}

#[gpui::test]
fn editing_detached_request_during_save_keeps_the_edit_dirty(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_structure_fixture("save-detached-edit-in-flight");
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), workspace);
            view.new_detached_request(probe_core::RequestProtocol::Http, window, cx);
            let key = view.shell.active_tab().unwrap();
            view.edit_request(
                key,
                |request| request.url = Some("https://example.test/saved".to_owned()),
                cx,
            );
            view.persist_detached_request(key, "In Flight".to_owned(), None, window, cx);
            view.edit_request(
                key,
                |request| request.url = Some("https://example.test/edited".to_owned()),
                cx,
            );
        })
        .unwrap();
    cx.run_until_parked();

    window
        .update(cx, |view, _, _| {
            let key = view.shell.active_tab().unwrap();
            assert!(!view.detached_requests.contains(&key));
            let loaded = view.loaded_workspace.as_ref().unwrap();
            assert!(loaded.request_selector(key).is_some());
            let request = loaded.workspace().request(key).unwrap();
            assert_eq!(request.metadata.name.as_deref(), Some("In Flight"));
            assert_eq!(request.url.as_deref(), Some("https://example.test/edited"));
            assert!(view.persistence.is_dirty(key, request));
        })
        .unwrap();
    let disk = probe_opencollection::load_workspace(&fixture).unwrap();
    let persisted = disk
        .requests()
        .iter()
        .find_map(|located| {
            let request = disk.workspace().request(located.key())?;
            (request.metadata.name.as_deref() == Some("In Flight")).then_some(request)
        })
        .unwrap();
    assert_eq!(persisted.url.as_deref(), Some("https://example.test/saved"));
    fs::remove_file(fixture).unwrap();
}

#[gpui::test]
fn unchanged_active_folder_survives_reconciliation_in_mixed_tab_order(cx: &mut TestAppContext) {
    let (window, [request, _, folder]) = mixed_tab_window(cx);
    window
        .update(cx, |view, _, cx| {
            view.shell.move_tab(folder, request, true);
            let loaded = view.loaded_workspace.as_ref().unwrap();
            let expected: Vec<_> = view
                .shell
                .open_tabs()
                .iter()
                .map(|tab| crate::session::TabLocator::capture(*tab, loaded).unwrap())
                .collect();
            let fresh = probe_opencollection::load_workspace(view.workspace_path.as_ref().unwrap())
                .unwrap();
            let crate::synchronization::ReconcileResult::Applied(reconciled) =
                crate::synchronization::reconcile(
                    &view.local_request_states(),
                    fresh,
                    &BTreeMap::new(),
                )
            else {
                panic!("unchanged workspace must reconcile");
            };
            assert!(!reconciled.selector_remaps.contains_key("items/1"));
            view.apply_reconciled_workspace(*reconciled, cx);
            let loaded = view.loaded_workspace.as_ref().unwrap();
            let actual: Vec<_> = view
                .shell
                .open_tabs()
                .iter()
                .map(|tab| crate::session::TabLocator::capture(*tab, loaded).unwrap())
                .collect();
            assert_eq!(actual, expected);
            assert_eq!(
                view.selected_tree_item,
                loaded.item_key(ItemKind::Folder, "items/1")
            );
            assert_eq!(
                crate::session::TabLocator::capture(view.shell.active_open_tab().unwrap(), loaded),
                Some(crate::session::TabLocator::Folder("items/1".into()))
            );
        })
        .unwrap();
}

#[gpui::test]
fn close_other_tabs_keeps_a_detached_tab_after_key_remap(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_structure_fixture("keep-detached-after-save");
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), workspace);
            view.new_detached_request(probe_core::RequestProtocol::Http, window, cx);
            let keep = view.shell.active_tab().unwrap();
            view.new_detached_request(probe_core::RequestProtocol::Http, window, cx);
            let save = view.shell.active_tab().unwrap();
            view.pending_close = Some(PendingClose::OtherTabs { keep: keep.into() });
            view.persist_detached_request(save, "Saved".to_owned(), None, window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |view, _, _| {
            assert!(view.pending_close.is_none());
            assert_eq!(view.shell.tabs().count(), 1);
            assert!(
                view.detached_requests
                    .contains(&view.shell.active_tab().unwrap())
            );
        })
        .unwrap();
}

#[gpui::test]
fn save_dialog_survives_workspace_reload(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_structure_fixture("save-dialog-reload");
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), workspace);
            view.new_detached_request(probe_core::RequestProtocol::Http, window, cx);
            let old_key = view.shell.active_tab().unwrap();
            view.open_save_detached_request_dialog(old_key, window, cx);
            let mut fresh = probe_opencollection::load_workspace(&fixture).unwrap();
            fresh
                .apply_structure(probe_opencollection::StructureOperation::CreateRequest {
                    parent: None,
                    index: None,
                    name: "External".to_owned(),
                    method: Some("GET".to_owned()),
                    url: None,
                    protocol: probe_core::RequestProtocol::Http,
                    graphql: None,
                    update: None,
                })
                .unwrap();
            let baselines = fresh
                .requests()
                .iter()
                .filter_map(|located| {
                    fresh
                        .workspace()
                        .request(located.key())
                        .cloned()
                        .map(|request| (located.key(), request))
                })
                .collect();
            view.install_reloaded_workspace(fresh, baselines, &BTreeMap::new(), &BTreeMap::new());
            let dialog = view.structure_dialog.as_mut().unwrap();
            let StructureDialogMode::SaveDetachedRequest { key } = dialog.mode else {
                panic!("expected save dialog")
            };
            assert_ne!(key, old_key);
            assert!(view.detached_requests.contains(&key));
            dialog.name = "Reloaded Draft".to_owned();
            view.submit_structure_dialog(window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    let reloaded = probe_opencollection::load_workspace(&fixture).unwrap();
    assert!(reloaded.requests().iter().any(|located| {
        reloaded
            .workspace()
            .request(located.key())
            .unwrap()
            .metadata
            .name
            .as_deref()
            == Some("Reloaded Draft")
    }));
}

#[gpui::test]
fn save_dialog_records_the_selected_parent(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_structure_fixture("save-dialog-parent");
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture, workspace);
            view.new_detached_request(probe_core::RequestProtocol::Http, window, cx);
            let key = view.shell.active_tab().unwrap();
            let folder = view
                .loaded_workspace
                .as_ref()
                .unwrap()
                .folder_key("items/1")
                .unwrap();
            view.select_tree_item(WorkspaceItemRef::Folder(folder), cx);
            view.open_save_detached_request_dialog(key, window, cx);
            let dialog = view.structure_dialog.as_ref().unwrap();
            let StructureDialogMode::SaveDetachedRequest { key: dialog_key } = dialog.mode else {
                panic!("expected save dialog");
            };
            assert_eq!(dialog_key, key);
            assert_eq!(dialog.parent, "items/1");
            assert!(view.detached_requests.contains(&key));
            let dialog = view.structure_dialog.as_mut().unwrap();
            dialog.name = "Renamed".to_owned();
            dialog.expanded_folders.insert("items/1".to_owned());
            view.open_save_detached_request_dialog(key, window, cx);
            let dialog = view.structure_dialog.as_ref().unwrap();
            assert_eq!(dialog.name, "Renamed");
            assert_eq!(dialog.parent, "items/1");
            assert!(dialog.expanded_folders.contains("items/1"));
        })
        .unwrap();
}

#[gpui::test]
fn creating_a_folder_from_the_save_dialog_keeps_the_draft_and_saves_into_it(
    cx: &mut TestAppContext,
) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_structure_fixture("save-dialog-new-folder");
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), workspace);
            view.new_detached_request(probe_core::RequestProtocol::Http, window, cx);
            let key = view.shell.active_tab().unwrap();
            view.edit_request(
                key,
                |request| request.url = Some("https://example.test/placed".to_owned()),
                cx,
            );
            let folder = view
                .loaded_workspace
                .as_ref()
                .unwrap()
                .folder_key("items/1")
                .unwrap();
            view.select_tree_item(WorkspaceItemRef::Folder(folder), cx);
            view.open_save_detached_request_dialog(key, window, cx);
            let dialog = view.structure_dialog.as_mut().unwrap();
            assert!(matches!(
                dialog.mode,
                StructureDialogMode::SaveDetachedRequest { .. }
            ));
            assert_eq!(dialog.parent, "items/1");
            dialog.name = "Placed".to_owned();
            dialog.new_folder_name = Some("  ".to_owned());
            view.create_folder_from_save_dialog(window, cx);
            assert!(has_active_toast(
                view,
                ToastIntent::Error,
                "Folder name is required."
            ));
            assert!(matches!(
                view.structure_dialog.as_ref().unwrap().mode,
                StructureDialogMode::SaveDetachedRequest { .. }
            ));
            view.structure_dialog.as_mut().unwrap().new_folder_name = Some("Inbox".to_owned());
            view.create_folder_from_save_dialog(window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |view, window, cx| {
            assert!(view.structure_task.is_none(), "{:?}", toast_debug(view));
            let dialog = view.structure_dialog.as_ref().unwrap();
            let StructureDialogMode::SaveDetachedRequest { key } = dialog.mode else {
                panic!("save dialog should stay open");
            };
            assert_eq!(dialog.name, "Placed");
            assert!(view.detached_requests.contains(&key));
            assert!(view.shell.open_tabs().contains(&key.into()));
            assert_eq!(
                view.loaded_workspace
                    .as_ref()
                    .unwrap()
                    .workspace()
                    .request(key)
                    .unwrap()
                    .url
                    .as_deref(),
                Some("https://example.test/placed")
            );
            let inbox = folder_selector_named(view, "Inbox").expect("folder should be created");
            assert_eq!(dialog.parent, inbox);
            view.submit_structure_dialog(window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    let loaded = probe_opencollection::load_workspace(&fixture).unwrap();
    let placed = loaded
        .requests()
        .iter()
        .find(|located| {
            loaded
                .workspace()
                .request(located.key())
                .unwrap()
                .metadata
                .name
                .as_deref()
                == Some("Placed")
        })
        .expect("request should be saved");
    let parent = loaded
        .workspace()
        .request_ancestor_folders(placed.key())
        .and_then(|ancestors| ancestors.last().copied())
        .expect("saved request should be inside the new folder");
    assert_eq!(
        loaded
            .workspace()
            .folder(parent)
            .unwrap()
            .metadata
            .name
            .as_deref(),
        Some("Inbox")
    );
}

#[gpui::test]
fn enter_in_the_save_folder_field_creates_the_folder_without_saving(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    cx.update(bind_platform_hotkeys);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_structure_fixture("save-folder-enter");
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture, workspace);
            view.new_detached_request(probe_core::RequestProtocol::Http, window, cx);
            let key = view.shell.active_tab().unwrap();
            view.open_save_detached_request_dialog(key, window, cx);
            let dialog = view.structure_dialog.as_mut().unwrap();
            dialog.name = "Placed".to_owned();
            dialog.new_folder_name = Some("Inbox".to_owned());
            cx.notify();
        })
        .unwrap();
    cx.run_until_parked();
    cx.simulate_keystrokes(window.into(), "enter");
    cx.run_until_parked();

    window
        .update(cx, |view, _, _| {
            assert!(view.structure_task.is_none(), "{:?}", toast_debug(view));
            let dialog = view
                .structure_dialog
                .as_ref()
                .expect("save dialog should stay open");
            let StructureDialogMode::SaveDetachedRequest { key } = dialog.mode else {
                panic!("save dialog should stay open");
            };
            assert_eq!(dialog.name, "Placed");
            assert!(view.detached_requests.contains(&key));
            assert!(view.shell.open_tabs().contains(&key.into()));
            let inbox = folder_selector_named(view, "Inbox").expect("folder should be created");
            assert_eq!(dialog.parent, inbox);
            assert!(dialog.new_folder_name.is_none());
        })
        .unwrap();
}

#[gpui::test]
fn new_request_tab_accepts_save_shortcut(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    cx.update(bind_platform_hotkeys);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = bundled_fixture().canonicalize().unwrap();
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            view.set_workspace(fixture, workspace);
            cx.notify();
        })
        .unwrap();
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let add = visual
        .debug_bounds("request-tab-add-trigger")
        .expect("new request control should render");
    visual.simulate_click(add.center(), Modifiers::default());
    visual.run_until_parked();
    let http = visual
        .debug_bounds("request-tab-new-http")
        .expect("HTTP menu item should render");
    visual.simulate_click(http.center(), Modifiers::default());
    visual.run_until_parked();
    cx.run_until_parked();

    window
        .update(cx, |view, window, cx| {
            let key = view.shell.active_tab().expect("new tab should be active");
            assert!(view.detached_requests.contains(&key));
            assert_eq!(window.focused(cx), Some(view.focus_handle.clone()));
        })
        .unwrap();
    cx.simulate_keystrokes(window.into(), super::save_shortcut());
    cx.run_until_parked();
    window
        .update(cx, |view, _, _| {
            assert!(matches!(
                view.structure_dialog.as_ref().map(|dialog| &dialog.mode),
                Some(StructureDialogMode::SaveDetachedRequest { .. })
            ));
        })
        .unwrap();
}

#[gpui::test]
fn enter_on_a_save_destination_selects_that_folder(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    cx.update(bind_platform_hotkeys);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_structure_fixture("save-destination-enter");
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    let folder = workspace.folder_key("items/1").unwrap();
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture, workspace);
            view.new_detached_request(probe_core::RequestProtocol::Http, window, cx);
            let key = view.shell.active_tab().unwrap();
            view.select_tree_item(WorkspaceItemRef::Folder(folder), cx);
            view.open_save_detached_request_dialog(key, window, cx);
            let dialog = view.structure_dialog.as_mut().unwrap();
            dialog.name = "Placed".to_owned();
            assert_eq!(dialog.parent, "items/1");
            cx.notify();
        })
        .unwrap();
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let row = visual
        .debug_bounds("save-destination-items/1")
        .expect("folder row should render");
    visual.simulate_click(row.center(), Modifiers::default());
    visual.run_until_parked();
    cx.run_until_parked();
    window
        .update(cx, |view, _, cx| {
            let dialog = view.structure_dialog.as_mut().unwrap();
            assert_eq!(dialog.parent, "items/1");
            dialog.parent.clear();
            cx.notify();
        })
        .unwrap();
    cx.simulate_keystrokes(window.into(), "enter");
    cx.run_until_parked();

    window
        .update(cx, |view, _, _| {
            let dialog = view
                .structure_dialog
                .as_ref()
                .expect("save dialog should stay open");
            let StructureDialogMode::SaveDetachedRequest { key } = dialog.mode else {
                panic!("save dialog should stay open");
            };
            assert_eq!(dialog.name, "Placed");
            assert_eq!(dialog.parent, "items/1");
            assert!(view.detached_requests.contains(&key));
            assert!(view.shell.open_tabs().contains(&key.into()));
        })
        .unwrap();
}

#[gpui::test]
fn creating_a_folder_keeps_open_tab_order(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_structure_fixture("save-folder-tab-order");
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    let persisted = workspace.request_key("items/0").unwrap();
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture, workspace);
            view.select_request(persisted, cx);
            view.new_detached_request(probe_core::RequestProtocol::Http, window, cx);
            assert_eq!(view.shell.tabs().count(), 2);
            assert_eq!(view.shell.tabs().next().unwrap(), persisted);
            assert_eq!(
                view.shell.active_tab(),
                Some(view.shell.tabs().nth(1).unwrap())
            );
            let detached = view.shell.tabs().nth(1).unwrap();
            view.open_save_detached_request_dialog(detached, window, cx);
            let dialog = view.structure_dialog.as_mut().unwrap();
            dialog.name = "Placed".to_owned();
            dialog.new_folder_name = Some("Inbox".to_owned());
            view.shell.activate(persisted.into());
            assert_eq!(view.shell.active_tab(), Some(persisted));
            view.create_folder_from_save_dialog(window, cx);
        })
        .unwrap();
    cx.run_until_parked();

    window
        .update(cx, |view, _, _| {
            assert!(view.structure_task.is_none(), "{:?}", toast_debug(view));
            let dialog = view
                .structure_dialog
                .as_ref()
                .expect("save dialog should stay open");
            let StructureDialogMode::SaveDetachedRequest { key } = dialog.mode else {
                panic!("save dialog should stay open");
            };
            assert_eq!(dialog.name, "Placed");
            assert_eq!(view.shell.tabs().count(), 2);
            assert_eq!(view.shell.tabs().nth(1).unwrap(), key);
            assert_eq!(
                view.shell.active_tab(),
                Some(view.shell.tabs().next().unwrap())
            );
            assert!(view.detached_requests.contains(&key));
            assert!(
                !view
                    .detached_requests
                    .contains(&view.shell.tabs().next().unwrap())
            );
            let loaded = view.loaded_workspace.as_ref().unwrap();
            assert_eq!(
                loaded
                    .workspace()
                    .request(view.shell.tabs().next().unwrap())
                    .unwrap()
                    .metadata
                    .name
                    .as_deref(),
                Some("Alpha")
            );
        })
        .unwrap();
}

#[gpui::test]
fn save_dialog_stays_open_when_a_save_is_already_running(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_structure_fixture("save-dialog-busy");
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    let persisted = workspace.request_key("items/0").unwrap();
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), workspace);
            view.select_request(persisted, cx);
            view.edit_request(
                persisted,
                |request| request.url = Some("https://example.test/busy".to_owned()),
                cx,
            );
            view.save_active_editor(window, cx);
            assert!(view.request_save_task.is_some());
            view.new_detached_request(probe_core::RequestProtocol::Http, window, cx);
            let key = view.shell.active_tab().unwrap();
            view.open_save_detached_request_dialog(key, window, cx);
            view.structure_dialog.as_mut().unwrap().name = "Placed".to_owned();
            view.submit_structure_dialog(window, cx);
            let dialog = view
                .structure_dialog
                .as_ref()
                .expect("save dialog should stay open");
            let StructureDialogMode::SaveDetachedRequest { key: dialog_key } = dialog.mode else {
                panic!("save dialog should stay open");
            };
            assert_eq!(dialog_key, key);
            assert_eq!(dialog.name, "Placed");
            assert!(view.detached_requests.contains(&key));
            assert!(view.structure_task.is_none());
            assert!(has_active_toast(
                view,
                ToastIntent::Warning,
                "Wait for the current save to finish."
            ));
        })
        .unwrap();
    cx.run_until_parked();

    let loaded = probe_opencollection::load_workspace(&fixture).unwrap();
    assert!(loaded.requests().iter().all(|located| {
        loaded
            .workspace()
            .request(located.key())
            .unwrap()
            .metadata
            .name
            .as_deref()
            != Some("Placed")
    }));
    window
        .update(cx, |view, _, _| {
            let dialog = view
                .structure_dialog
                .as_ref()
                .expect("save dialog should stay open");
            assert_eq!(dialog.name, "Placed");
            assert!(matches!(
                dialog.mode,
                StructureDialogMode::SaveDetachedRequest { .. }
            ));
            let StructureDialogMode::SaveDetachedRequest { key } = dialog.mode else {
                panic!("save dialog should stay open");
            };
            assert!(view.detached_requests.contains(&key));
        })
        .unwrap();
}

#[gpui::test]
fn missing_save_destination_keeps_the_save_dialog(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_structure_fixture("save-dialog-missing-parent");
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture, workspace);
            view.new_detached_request(probe_core::RequestProtocol::Http, window, cx);
            let key = view.shell.active_tab().unwrap();
            view.open_save_detached_request_dialog(key, window, cx);
            let dialog = view.structure_dialog.as_mut().unwrap();
            dialog.name = "Kept".to_owned();
            dialog.parent = "not-a-folder".to_owned();
            dialog.expanded_folders.insert("items/1".to_owned());
            view.remap_structure_dialog(&BTreeMap::new());
            let dialog = view
                .structure_dialog
                .as_ref()
                .expect("save dialog should stay open");
            let StructureDialogMode::SaveDetachedRequest { key: dialog_key } = dialog.mode else {
                panic!("save dialog should stay open");
            };
            assert_eq!(dialog_key, key);
            assert_eq!(dialog.name, "Kept");
            assert!(dialog.parent.is_empty());
            assert!(dialog.expanded_folders.contains("items/1"));
        })
        .unwrap();
}

#[gpui::test]
fn move_dialog_keeps_a_destination_parent(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_structure_fixture("move-dialog-parent");
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    let request = workspace.request_key("items/0").unwrap();
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), workspace);
            view.select_request(request, cx);
            view.open_move_dialog(window, cx);
            let dialog = view.structure_dialog.as_ref().unwrap();
            assert!(matches!(dialog.mode, StructureDialogMode::Move { .. }));
            assert_eq!(dialog.parent, "");
            view.structure_dialog.as_mut().unwrap().parent = "items/1".to_owned();
            view.submit_structure_dialog(window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    let loaded = probe_opencollection::load_workspace(&fixture).unwrap();
    let moved = loaded
        .requests()
        .iter()
        .find(|located| {
            loaded
                .workspace()
                .request(located.key())
                .unwrap()
                .metadata
                .name
                .as_deref()
                == Some("Alpha")
        })
        .expect("moved request should remain");
    let parent = loaded
        .workspace()
        .request_ancestor_folders(moved.key())
        .and_then(|ancestors| ancestors.last().copied())
        .expect("request should leave the collection root");
    assert_eq!(
        loaded
            .workspace()
            .folder(parent)
            .unwrap()
            .metadata
            .name
            .as_deref(),
        Some("Folder")
    );
}

#[gpui::test]
fn request_tab_add_button_stays_with_the_open_tabs(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = bundled_fixture().canonicalize().unwrap();
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    let request = workspace.requests()[0].key();
    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            view.set_workspace(fixture, workspace);
            view.select_request(request, cx);
            cx.notify();
        })
        .unwrap();
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let tab = visual
        .debug_bounds("request-tab-0")
        .expect("open request should render a tab");
    let add = visual
        .debug_bounds("request-tab-add-trigger")
        .expect("new request control should stay in the tab bar");
    let environment = visual
        .debug_bounds("request-environment-trigger")
        .expect("environment dropdown should stay at the end of the tab bar");
    let tab_gap = f32::from(add.left()) - f32::from(tab.right());
    let trailing = f32::from(environment.left()) - f32::from(add.right());
    assert!(f32::from(add.left()) >= f32::from(tab.left()));
    assert!(trailing > tab_gap);
}

fn folder_selector_named(view: &ProbeApp, name: &str) -> Option<String> {
    let loaded = view.loaded_workspace.as_ref()?;
    loaded.folders().iter().find_map(|located| {
        let folder = loaded.workspace().folder(located.key())?;
        (folder.metadata.name.as_deref() == Some(name)).then(|| located.selector().to_owned())
    })
}

#[gpui::test]
fn unified_session_tabs_take_precedence_over_legacy_fields(cx: &mut TestAppContext) {
    let (window, [request, _, folder]) = mixed_tab_window(cx);
    window
        .update(cx, |view, _, cx| {
            view.shell.move_tab(folder, request, true);
            view.capture_session();
            let order = view.shell.open_tabs().to_vec();
            let loaded = view.loaded_workspace.as_ref().unwrap();
            let crate::session::TabLocator::Request(selector) =
                crate::session::TabLocator::capture(request, loaded).unwrap()
            else {
                unreachable!()
            };
            let saved = view
                .session
                .workspaces
                .get_mut(view.workspace_path.as_ref().unwrap())
                .unwrap();
            // Stale compatibility fields must not change current unified state.
            saved.open_tabs = vec![selector.clone()];
            saved.active_tab = Some(selector);
            view.shell.reset_for_workspace();
            view.restore_shell_state(cx);
            assert_eq!(view.shell.open_tabs(), order);
            assert_eq!(view.shell.active_open_tab(), Some(folder));
            let saved = view
                .session
                .workspaces
                .get_mut(view.workspace_path.as_ref().unwrap())
                .unwrap();
            saved.ordered_tabs = Some(Vec::new());
            saved.active_open_tab = None;
            view.restore_shell_state(cx);
            assert!(view.shell.open_tabs().is_empty());
            assert_eq!(view.shell.active_open_tab(), None);
        })
        .unwrap();
}

#[gpui::test]
fn reconciliation_preserves_interleaved_detached_tab_and_its_selection(cx: &mut TestAppContext) {
    let (window, [request, _, _]) = mixed_tab_window(cx);
    window
        .update(cx, |view, window, cx| {
            view.new_detached_request(probe_core::RequestProtocol::Http, window, cx);
            let old_detached = view.shell.active_open_tab().unwrap();
            view.shell.move_tab(old_detached, request, false);
            let loaded = view.loaded_workspace.as_ref().unwrap();
            let expected: Vec<_> = view
                .shell
                .open_tabs()
                .iter()
                .map(|tab| crate::session::TabLocator::capture(*tab, loaded))
                .collect();
            let fresh = probe_opencollection::load_workspace(view.workspace_path.as_ref().unwrap())
                .unwrap();
            view.reconcile_filesystem_workspace(fresh, BTreeMap::new(), window, cx);
            let loaded = view.loaded_workspace.as_ref().unwrap();
            let actual: Vec<_> = view
                .shell
                .open_tabs()
                .iter()
                .map(|tab| crate::session::TabLocator::capture(*tab, loaded))
                .collect();
            assert_eq!(actual, expected);
            let crate::shell::OpenTab::Request(active) = view.shell.active_open_tab().unwrap()
            else {
                panic!("detached request must stay selected");
            };
            assert!(view.detached_requests.contains(&active));
            assert_eq!(view.shell.open_tabs()[1], active.into());
            assert_ne!(view.shell.active_open_tab(), Some(old_detached));
        })
        .unwrap();
}

#[gpui::test]
fn close_other_tabs_preserves_the_kept_overview_draft(cx: &mut TestAppContext) {
    let (window, [_, collection, _]) = mixed_tab_window(cx);
    window
        .update(cx, |view, window, cx| {
            view.edit_overview(
                OverviewTarget::Collection,
                true,
                "Unsaved collection docs".into(),
                cx,
            );
            view.request_close_other_tabs(collection, window, cx);
            assert!(view.application_dialog.is_none());
            assert_eq!(view.shell.open_tabs(), &[collection]);
            assert_eq!(view.shell.active_open_tab(), Some(collection));
            assert!(view.has_dirty_overviews());
        })
        .unwrap();
}

#[gpui::test]
fn close_other_tabs_prompts_for_a_draft_on_another_overview(cx: &mut TestAppContext) {
    let (window, tabs @ [_, _, folder]) = mixed_tab_window(cx);
    window
        .update(cx, |view, window, cx| {
            view.edit_overview(
                OverviewTarget::Collection,
                true,
                "Unsaved collection docs".into(),
                cx,
            );
            view.request_close_other_tabs(folder, window, cx);
            let Some(ApplicationDialog::Unsaved {
                pending,
                documentation,
                ..
            }) = view.application_dialog.as_ref()
            else {
                panic!("dirty overview must prompt before closing");
            };
            assert!(*documentation);
            assert_eq!(
                view.pending_overview_targets(pending),
                vec![OverviewTarget::Collection]
            );
            assert_eq!(view.shell.open_tabs(), tabs);
        })
        .unwrap();
}

#[gpui::test]
fn switching_workspaces_restores_each_tabs_active_tab_and_folders(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let a = bundled_fixture().canonicalize().unwrap();
    let b = nested_fixture().canonicalize().unwrap();
    let load = |path: &PathBuf| probe_opencollection::load_workspace(path).unwrap();
    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            view.set_workspace(a.clone(), load(&a));
            let loaded = view.loaded_workspace.as_ref().unwrap();
            let a_tabs: Vec<_> = loaded
                .requests()
                .iter()
                .take(2)
                .map(|item| item.key())
                .collect();
            let a_folder = loaded.folders()[0].key();
            view.shell.open_request(a_tabs[0]);
            view.shell.open_request(a_tabs[1]);
            view.shell.activate(a_tabs[1].into());
            view.shell.collapse_folder(a_folder);

            view.set_workspace(b.clone(), load(&b));
            view.restore_shell_state(cx);
            assert!(view.shell.tabs().next().is_none());
            let loaded = view.loaded_workspace.as_ref().unwrap();
            let b_tab = loaded.requests()[0].key();
            let b_folder = loaded.folders()[0].key();
            view.shell.open_request(b_tab);
            view.shell.collapse_folder(b_folder);

            view.set_workspace(a.clone(), load(&a));
            view.restore_shell_state(cx);
            let loaded = view.loaded_workspace.as_ref().unwrap();
            let restored_a: Vec<_> = loaded
                .requests()
                .iter()
                .take(2)
                .map(|item| item.key())
                .collect();
            assert_eq!(view.shell.tabs().collect::<Vec<_>>(), restored_a);
            assert_eq!(view.shell.active_tab(), Some(restored_a[1]));
            assert!(!view.shell.folder_is_expanded(loaded.folders()[0].key()));

            view.set_workspace(b.clone(), load(&b));
            view.restore_shell_state(cx);
            let loaded = view.loaded_workspace.as_ref().unwrap();
            assert_eq!(
                view.shell.tabs().collect::<Vec<_>>(),
                &[loaded.requests()[0].key()]
            );
            assert_eq!(view.shell.active_tab(), Some(loaded.requests()[0].key()));
            assert!(!view.shell.folder_is_expanded(loaded.folders()[0].key()));
            view.capture_session();
            assert_eq!(view.session.active_collection.as_ref(), Some(&b));
        })
        .unwrap();
}

#[gpui::test]
fn loading_existing_workspaces_restores_tabs_after_switching(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let a = bundled_fixture().canonicalize().unwrap();
    let b = nested_fixture().canonicalize().unwrap();
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.load_workspace_path(a.clone(), None, window, cx);
        })
        .unwrap();
    cx.run_until_parked();

    window
        .update(cx, |view, window, cx| {
            assert_eq!(view.workspace_path.as_ref(), Some(&a));
            let requests = view.loaded_workspace.as_ref().unwrap().requests();
            let (first, second) = (requests[0].key(), requests[1].key());
            view.select_request(first, cx);
            view.select_request(second, cx);
            view.select_request(first, cx);
            view.load_workspace_path(b.clone(), None, window, cx);
        })
        .unwrap();
    cx.run_until_parked();

    window
        .update(cx, |view, window, cx| {
            assert_eq!(view.workspace_path.as_ref(), Some(&b));
            assert!(view.shell.tabs().next().is_none());
            let requests = view.loaded_workspace.as_ref().unwrap().requests();
            let (first, second) = (requests[0].key(), requests[1].key());
            view.select_request(first, cx);
            view.select_request(second, cx);
            view.load_workspace_path(a.clone(), None, window, cx);
        })
        .unwrap();
    cx.run_until_parked();

    window
        .update(cx, |view, window, cx| {
            assert_eq!(view.workspace_path.as_ref(), Some(&a));
            let requests = view.loaded_workspace.as_ref().unwrap().requests();
            let (first, second) = (requests[0].key(), requests[1].key());
            assert_eq!(view.shell.tabs().collect::<Vec<_>>(), &[first, second]);
            assert_eq!(view.shell.active_tab(), Some(first));
            view.load_workspace_path(b.clone(), None, window, cx);
        })
        .unwrap();
    cx.run_until_parked();

    window
        .update(cx, |view, _, _| {
            assert_eq!(view.workspace_path.as_ref(), Some(&b));
            let requests = view.loaded_workspace.as_ref().unwrap().requests();
            let (first, second) = (requests[0].key(), requests[1].key());
            assert_eq!(view.shell.tabs().collect::<Vec<_>>(), &[first, second]);
            assert_eq!(view.shell.active_tab(), Some(second));
        })
        .unwrap();
}

#[gpui::test]
fn add_menu_renders_compact_request_and_folder_markers(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = bundled_fixture()
        .canonicalize()
        .expect("fixture should exist");
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            view.set_workspace(fixture, workspace);
            view.transient.structure_add_menu_open = true;
            cx.notify();
        })
        .unwrap();
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.update(|window, cx| window.simulate_next_frame(cx));
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(
        visual
            .debug_bounds("tree-new-http-request-leading")
            .is_some()
    );
    assert!(
        visual
            .debug_bounds("tree-new-graphql-request-leading")
            .is_some()
    );
    assert!(visual.debug_bounds("tree-new-folder-leading").is_some());
}

#[gpui::test]
fn dismissing_transient_surfaces_closes_the_request_execution_menu(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });

    window
        .update(cx, |view, _, _| {
            view.transient.request_execution_menu_open = true;
            view.dismiss_transient_surfaces();
            assert!(!view.transient.request_execution_menu_open);
        })
        .unwrap();
}

#[gpui::test]
fn structural_rename_keeps_open_tab_and_dirty_draft(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_structure_fixture("rename-open-tab");
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    let request = workspace.request_key("items/0").unwrap();
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), workspace);
            view.select_request(request, cx);
            view.edit_request(
                request,
                |request| request.url = Some("https://local.example/dirty".to_owned()),
                cx,
            );
            view.apply_structure(
                probe_opencollection::StructureOperation::Rename {
                    target: ItemLocator::new(ItemKind::Request, "items/0"),
                    name: "Renamed Alpha".to_owned(),
                },
                window,
                cx,
            );
        })
        .unwrap();
    cx.run_until_parked();

    window
        .update(cx, |view, _, _| {
            assert!(view.structure_task.is_none(), "{:?}", toast_debug(view));
            let loaded = view.loaded_workspace.as_ref().unwrap();
            let renamed = loaded.request_key("items/0").unwrap();
            let request = loaded.workspace().request(renamed).unwrap();
            assert_eq!(request.metadata.name.as_deref(), Some("Renamed Alpha"));
            assert_eq!(request.url.as_deref(), Some("https://local.example/dirty"));
            assert!(view.persistence.is_dirty(renamed, request));
            assert_eq!(view.shell.active_tab(), Some(renamed));
            assert_eq!(view.shell.tabs().collect::<Vec<_>>(), &[renamed]);
        })
        .unwrap();

    fs::remove_file(fixture).unwrap();
}

#[gpui::test]
fn structural_move_remaps_tabs_and_preserves_dirty_drafts(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_structure_fixture("move");
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    let request = workspace.request_key("items/0").unwrap();
    let folder = workspace.folder_key("items/1").unwrap();
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), workspace);
            view.shell
                .open_overview(crate::shell::OverviewTab::Collection);
            view.shell
                .open_overview(crate::shell::OverviewTab::Folder(folder));
            view.select_request(request, cx);
            view.shell.collapse_folder(folder);
            view.edit_request(
                request,
                |request| request.url = Some("https://local.example/dirty".to_owned()),
                cx,
            );
            view.apply_structure(
                probe_opencollection::StructureOperation::Move {
                    target: ItemLocator::new(ItemKind::Request, "items/0"),
                    parent: Some("items/1".to_owned()),
                    index: Some(1),
                },
                window,
                cx,
            );
        })
        .unwrap();
    cx.run_until_parked();

    window
        .update(cx, |view, _, _| {
            assert!(view.structure_task.is_none(), "{:?}", toast_debug(view));
            let loaded = view.loaded_workspace.as_ref().unwrap();
            let moved = loaded.request_key("items/0/items/1").unwrap();
            let request = loaded.workspace().request(moved).unwrap();
            assert_eq!(request.url.as_deref(), Some("https://local.example/dirty"));
            assert!(view.persistence.is_dirty(moved, request));
            assert_eq!(view.shell.active_tab(), Some(moved));
            assert!(view.shell.open_tabs().contains(&moved.into()));
            let remapped_folder = loaded.folder_key("items/0").unwrap();
            assert!(!view.shell.folder_is_expanded(remapped_folder));
            assert_eq!(
                view.shell.overview_tabs().collect::<Vec<_>>(),
                &[
                    crate::shell::OverviewTab::Collection,
                    crate::shell::OverviewTab::Folder(remapped_folder),
                ]
            );
        })
        .unwrap();

    let disk = probe_opencollection::load_workspace(&fixture).unwrap();
    let persisted = disk
        .workspace()
        .request(disk.request_key("items/0/items/1").unwrap())
        .unwrap();
    assert_ne!(
        persisted.url.as_deref(),
        Some("https://local.example/dirty"),
        "structural moves must not silently save an unrelated dirty draft"
    );
    fs::remove_file(fixture).unwrap();
}

#[gpui::test]
fn creating_root_request_without_selection_selects_opens_and_reveals_it(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_large_fixture("create-root-request-selection");
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), workspace);
            view.selected_tree_item = None;
            view.apply_structure(
                probe_opencollection::StructureOperation::CreateRequest {
                    parent: None,
                    index: None,
                    name: "Created Root".to_owned(),
                    method: Some("GET".to_owned()),
                    url: None,
                    protocol: probe_core::RequestProtocol::Http,
                    graphql: None,
                    update: None,
                },
                window,
                cx,
            );
        })
        .unwrap();
    cx.run_until_parked();

    let created_selector = window
        .update(cx, |view, _, _| {
            assert!(view.structure_task.is_none(), "{:?}", toast_debug(view));
            let loaded = view.loaded_workspace.as_ref().unwrap();
            let created = loaded
                .requests()
                .iter()
                .find_map(|located| {
                    let request = loaded.workspace().request(located.key())?;
                    (request.metadata.name.as_deref() == Some("Created Root"))
                        .then(|| located.key())
                })
                .expect("created request should exist");
            assert_eq!(
                view.selected_tree_item,
                Some(WorkspaceItemRef::Request(created))
            );
            assert_eq!(view.shell.active_tab(), Some(created));
            loaded.request_selector(created).unwrap().to_owned()
        })
        .unwrap();
    assert_eq!(created_selector, "items/1001");
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual
        .debug_bounds("tree-row-items/1001")
        .expect("created request should be revealed in the tree");

    fs::remove_file(fixture).unwrap();
}

#[gpui::test]
fn creating_request_in_selected_folder_selects_child_and_expands_parent(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_bundled_fixture("create-folder-child-selection");
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    let folder = workspace.folder_key("items/0").unwrap();
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), workspace);
            view.select_tree_item(WorkspaceItemRef::Folder(folder), cx);
            view.shell.collapse_folder(folder);
            view.rebuild_visible_tree_rows();
            view.apply_structure(
                probe_opencollection::StructureOperation::CreateRequest {
                    parent: Some("items/0".to_owned()),
                    index: None,
                    name: "Created Child".to_owned(),
                    method: Some("GET".to_owned()),
                    url: None,
                    protocol: probe_core::RequestProtocol::Http,
                    graphql: None,
                    update: None,
                },
                window,
                cx,
            );
        })
        .unwrap();
    cx.run_until_parked();

    window
        .update(cx, |view, _, _| {
            assert!(view.structure_task.is_none(), "{:?}", toast_debug(view));
            let loaded = view.loaded_workspace.as_ref().unwrap();
            let created = loaded.request_key("items/0/items/1").unwrap();
            let folder = loaded.folder_key("items/0").unwrap();
            assert_eq!(
                view.selected_tree_item,
                Some(WorkspaceItemRef::Request(created))
            );
            assert_eq!(view.shell.active_tab(), Some(created));
            assert!(view.shell.folder_is_expanded(folder));
            assert!(
                view.visible_tree_rows
                    .iter()
                    .any(|row| row.item == WorkspaceItemRef::Request(created)),
                "created child should be visible after expanding its parent"
            );
        })
        .unwrap();

    fs::remove_file(fixture).unwrap();
}

fn simulate_tree_drag(
    visual: &mut VisualTestContext,
    from: gpui::Bounds<gpui::Pixels>,
    to: gpui::Point<gpui::Pixels>,
) {
    visual.simulate_mouse_down(from.center(), MouseButton::Left, Modifiers::default());
    visual.simulate_mouse_move(
        point(from.center().x + px(8.0), from.center().y + px(8.0)),
        Some(MouseButton::Left),
        Modifiers::default(),
    );
    visual.simulate_mouse_move(to, Some(MouseButton::Left), Modifiers::default());
    visual.simulate_mouse_up(to, MouseButton::Left, Modifiers::default());
}

#[gpui::test]
fn tree_drag_moves_a_request_into_a_folder(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_structure_fixture("tree-drag-move");
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), workspace);
            cx.notify();
        })
        .unwrap();
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let source = visual
        .debug_bounds("tree-row-items/0")
        .expect("request row should render");
    let folder = visual
        .debug_bounds("tree-row-items/1")
        .expect("folder row should render");
    simulate_tree_drag(&mut visual, source, folder.center());
    visual.run_until_parked();
    cx.run_until_parked();

    window
        .update(cx, |view, _, _| {
            assert!(view.structure_task.is_none(), "{:?}", toast_debug(view));
            let loaded = view.loaded_workspace.as_ref().unwrap();
            assert!(loaded.request_key("items/0/items/1").is_some());
            assert!(loaded.folder_key("items/0").is_some());
            assert!(loaded.request_key("items/0").is_none());
        })
        .unwrap();

    let disk = probe_opencollection::load_workspace(&fixture).unwrap();
    assert!(disk.request_key("items/0/items/1").is_some());
    assert!(disk.folder_key("items/0").is_some());
    fs::remove_file(fixture).unwrap();
}

#[gpui::test]
fn tree_drag_reorders_a_folder_before_its_sibling(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_structure_fixture("tree-drag-reorder");
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), workspace);
            cx.notify();
        })
        .unwrap();
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let folder = visual
        .debug_bounds("tree-row-items/1")
        .expect("folder row should render");
    let request = visual
        .debug_bounds("tree-row-items/0")
        .expect("request row should render");
    simulate_tree_drag(
        &mut visual,
        folder,
        point(request.center().x, request.top() + px(2.0)),
    );
    visual.run_until_parked();
    cx.run_until_parked();

    window
        .update(cx, |view, _, _| {
            assert!(view.structure_task.is_none(), "{:?}", toast_debug(view));
            let loaded = view.loaded_workspace.as_ref().unwrap();
            let root = loaded.workspace().root_items();
            assert!(matches!(root[0], probe_core::WorkspaceItemRef::Folder(_)));
            assert!(matches!(root[1], probe_core::WorkspaceItemRef::Request(_)));
        })
        .unwrap();

    let disk = probe_opencollection::load_workspace(&fixture).unwrap();
    assert!(disk.folder_key("items/0").is_some());
    assert!(disk.request_key("items/1").is_some());
    fs::remove_file(fixture).unwrap();
}

#[gpui::test]
fn tree_drag_moves_a_nested_request_to_root_end(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_structure_fixture("tree-drag-root-end");
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), workspace);
            cx.notify();
        })
        .unwrap();
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let nested = visual
        .debug_bounds("tree-row-items/1/items/0")
        .expect("nested request row should render");
    simulate_tree_drag(
        &mut visual,
        nested,
        point(nested.center().x, nested.bottom() + px(48.0)),
    );
    visual.run_until_parked();
    cx.run_until_parked();

    window
        .update(cx, |view, _, _| {
            assert!(view.structure_task.is_none(), "{:?}", toast_debug(view));
            let loaded = view.loaded_workspace.as_ref().unwrap();
            let root = loaded.workspace().root_items();
            assert_eq!(root.len(), 3);
            assert!(matches!(root[2], probe_core::WorkspaceItemRef::Request(_)));
            assert!(loaded.request_key("items/2").is_some());
            let folder = loaded.folder_key("items/1").unwrap();
            assert!(
                loaded
                    .workspace()
                    .folder(folder)
                    .unwrap()
                    .children
                    .is_empty()
            );
        })
        .unwrap();

    let disk = probe_opencollection::load_workspace(&fixture).unwrap();
    assert!(disk.request_key("items/2").is_some());
    fs::remove_file(fixture).unwrap();
}

#[gpui::test]
fn tree_drag_rejects_dropping_a_folder_into_itself(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_structure_fixture("tree-drag-invalid");
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), workspace);
            cx.notify();
        })
        .unwrap();
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let folder = visual
        .debug_bounds("tree-row-items/1")
        .expect("folder row should render");
    simulate_tree_drag(&mut visual, folder, folder.center());
    visual.run_until_parked();
    cx.run_until_parked();

    window
        .update(cx, |view, _, _| {
            assert!(view.structure_task.is_none(), "{:?}", toast_debug(view));
            let loaded = view.loaded_workspace.as_ref().unwrap();
            assert!(loaded.request_key("items/0").is_some());
            assert!(loaded.folder_key("items/1").is_some());
            assert!(view.toasts.is_empty(), "{:?}", toast_debug(view));
        })
        .unwrap();
    fs::remove_file(fixture).unwrap();
}

#[gpui::test]
fn failed_structure_edit_keeps_the_previous_workspace(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_structure_fixture("tree-drag-conflict");
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), workspace);
            cx.notify();
        })
        .unwrap();
    cx.run_until_parked();
    fs::write(
        &fixture,
        "opencollection: 1.0.0\ninfo:\n  name: changed\nbundled: true\nitems: []\n",
    )
    .unwrap();

    window
        .update(cx, |view, window, cx| {
            let folder = view
                .loaded_workspace
                .as_ref()
                .unwrap()
                .folder_key("items/1")
                .unwrap();
            view.select_tree_item(WorkspaceItemRef::Folder(folder), cx);
            view.reorder_selected(-1, window, cx);
        })
        .unwrap();
    cx.run_until_parked();

    window
        .update(cx, |view, _, _| {
            assert!(view.structure_task.is_none());
            assert!(
                view.toasts.iter().any(|(_, toast, _)| {
                    toast.intent == ToastIntent::Error
                        && toast
                            .message
                            .contains("Could not edit collection structure")
                        && toast.message.contains("externally modified")
                }),
                "{:?}",
                toast_debug(view)
            );
            let loaded = view.loaded_workspace.as_ref().unwrap();
            assert!(loaded.request_key("items/0").is_some());
            assert!(loaded.folder_key("items/1").is_some());
        })
        .unwrap();
    fs::remove_file(fixture).unwrap();
}

#[gpui::test]
fn request_save_runs_in_background_and_clears_dirty_state(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_bundled_fixture("save");
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    let key = workspace.requests()[0].key();
    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), workspace);
            view.select_request(key, cx);
            view.edit_request(
                key,
                |request| request.url = Some("https://saved.example/pets".to_owned()),
                cx,
            );
            assert!(
                view.persistence.is_dirty(
                    key,
                    view.loaded_workspace
                        .as_ref()
                        .unwrap()
                        .workspace()
                        .request(key)
                        .unwrap()
                )
            );
        })
        .unwrap();
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let save = visual
        .debug_bounds("editor-save")
        .expect("dirty request should show its save icon");
    let breadcrumb = visual
        .debug_bounds("request-breadcrumb")
        .expect("request breadcrumb should render");
    assert_eq!(
        save.right(),
        breadcrumb.right(),
        "save icon should be anchored to the breadcrumb's right edge"
    );
    visual.simulate_click(save.center(), Modifiers::default());
    visual.run_until_parked();
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let clean_save = visual
        .debug_bounds("editor-save")
        .expect("save icon should remain visible when the request is clean");
    let breadcrumb = visual
        .debug_bounds("request-breadcrumb")
        .expect("request breadcrumb should remain visible");
    assert_eq!(clean_save.right(), breadcrumb.right());

    let (dirty, message) = window
        .update(cx, |view, _, _| {
            let request = view
                .loaded_workspace
                .as_ref()
                .unwrap()
                .workspace()
                .request(key)
                .unwrap();
            (view.persistence.is_dirty(key, request), toast_debug(view))
        })
        .unwrap();
    assert!(!dirty, "save failed: {message:?}");
    let reloaded = probe_opencollection::load_workspace(&fixture).unwrap();
    assert_eq!(
        reloaded
            .workspace()
            .request(reloaded.requests()[0].key())
            .unwrap()
            .url
            .as_deref(),
        Some("https://saved.example/pets")
    );
    fs::remove_file(fixture).unwrap();
}

#[gpui::test]
fn save_shortcut_after_clicking_remove_query_row_persists_removal(cx: &mut TestAppContext) {
    assert_save_shortcut_after_clicking_remove_row_persists_removal(
        cx,
        "remove-query-row-shortcut-save",
        EditorSection::Query,
        "add-query-parameter",
        "remove-query-1",
        |request| {
            assert_eq!(request.query_parameters.len(), 1);
            assert_eq!(request.query_parameters[0].name, "limit");
        },
    );
}

#[gpui::test]
fn save_shortcut_after_clicking_remove_header_row_persists_removal(cx: &mut TestAppContext) {
    assert_save_shortcut_after_clicking_remove_row_persists_removal(
        cx,
        "remove-header-row-shortcut-save",
        EditorSection::Headers,
        "add-header",
        "remove-header-2",
        |request| {
            assert_eq!(request.headers.len(), 2);
            assert_eq!(request.headers[0].name, "Accept");
            assert_eq!(request.headers[1].name, "X-Debug");
        },
    );
}

#[gpui::test]
fn workspace_reload_preserves_running_request_execution(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_environment_fixture("reload-running-execution");
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), workspace);

            let old = view.loaded_workspace.as_ref().unwrap();
            let old_key = old.requests()[0].key();
            let (sender, mut receiver) = oneshot::channel();
            view.execution.begin(old_key, sender);

            let fresh = probe_opencollection::load_workspace(&fixture).unwrap();
            let selector_remaps = old
                .requests()
                .iter()
                .map(|located| (located.selector().to_owned(), located.selector().to_owned()))
                .collect::<BTreeMap<_, _>>();
            let key_remaps = request_key_remaps(old, &fresh, &selector_remaps);
            let baselines = fresh
                .requests()
                .iter()
                .filter_map(|located| {
                    fresh
                        .workspace()
                        .request(located.key())
                        .cloned()
                        .map(|request| (located.key(), request))
                })
                .collect::<Vec<_>>();
            let new_key = key_remaps[&old_key];

            view.install_reloaded_workspace(fresh, baselines, &key_remaps, &BTreeMap::new());

            assert!(receiver.try_recv().is_err());
            assert!(matches!(
                view.execution.response(new_key),
                Some(crate::execution::ResponseState::Running { .. })
            ));
            cx.notify();
        })
        .unwrap();

    fs::remove_file(fixture).unwrap();
}

#[gpui::test]
fn saving_after_removing_empty_query_parameter_during_in_flight_save_clears_dirty_state(
    cx: &mut TestAppContext,
) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_bundled_fixture("save-empty-query-removal");
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    let key = workspace.requests()[0].key();
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), workspace);
            view.select_request(key, cx);
            view.edit_request(
                key,
                |request| {
                    request.query_parameters.push(QueryParameter {
                        name: String::new(),
                        value: String::new(),
                        disabled: false,
                    });
                },
                cx,
            );
            view.save_active_editor(window, cx);
            view.edit_request(
                key,
                |request| {
                    request.query_parameters.retain(|parameter| {
                        !parameter.name.is_empty() || !parameter.value.is_empty()
                    });
                },
                cx,
            );
            assert!(
                view.persistence.is_dirty(
                    key,
                    view.loaded_workspace
                        .as_ref()
                        .unwrap()
                        .workspace()
                        .request(key)
                        .unwrap()
                ),
                "removing a saved empty parameter should make the request dirty before save"
            );
            view.save_active_editor(window, cx);
        })
        .unwrap();
    cx.run_until_parked();

    let (dirty, message) = window
        .update(cx, |view, _, _| {
            let request = view
                .loaded_workspace
                .as_ref()
                .unwrap()
                .workspace()
                .request(key)
                .unwrap();
            (view.persistence.is_dirty(key, request), toast_debug(view))
        })
        .unwrap();
    assert!(!dirty, "save failed: {message:?}");
    let reloaded = probe_opencollection::load_workspace(&fixture).unwrap();
    let request = reloaded
        .workspace()
        .request(reloaded.requests()[0].key())
        .unwrap();
    assert_eq!(request.query_parameters.len(), 1);
    assert_eq!(request.query_parameters[0].name, "limit");
    fs::remove_file(fixture).unwrap();
}

#[gpui::test]
fn discarding_a_dirty_tab_restores_the_workspace_request(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = bundled_fixture()
        .canonicalize()
        .expect("fixture should exist");
    let workspace = probe_opencollection::load_workspace(&fixture).expect("fixture should load");
    let key = workspace.requests()[0].key();
    let original_url = workspace
        .workspace()
        .request(key)
        .and_then(|request| request.url.clone());

    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            view.set_workspace(fixture, workspace);
            view.select_request(key, cx);
            view.edit_request(
                key,
                |request| request.url = Some("https://discarded.example".to_owned()),
                cx,
            );
            assert_eq!(view.dirty_keys(), vec![key]);
            let (cancellation, _) = oneshot::channel();
            let generation = view.execution.begin(key, cancellation);
            view.complete_execution(
                key,
                generation,
                Ok(HttpResponse {
                    status: 200,
                    initial_url: "https://example.com/start".to_owned(),
                    url_changed: false,
                    reason: "OK".to_owned(),
                    url: "https://discarded.example".to_owned(),
                    duration: Duration::from_millis(12),
                    size: 2,
                    headers: Vec::new(),
                    body: b"ok".to_vec(),
                    body_complete: true,
                    body_file: None,
                    body_retention_error: None,
                }),
                None,
                cx,
            );
            assert!(view.execution.response(key).is_some());
            assert!(view.response_viewer.document(key).is_some());

            view.discard_dirty_requests(&[key]);
            view.close_tab_now(key, cx);
            view.select_request(key, cx);

            let request = view
                .loaded_workspace
                .as_ref()
                .unwrap()
                .workspace()
                .request(key)
                .unwrap();
            assert_eq!(request.url, original_url);
            assert!(view.dirty_keys().is_empty());
            assert!(view.execution.response(key).is_none());
            assert!(view.response_viewer.document(key).is_none());
        })
        .unwrap();
}

#[gpui::test]
fn unsaved_changes_use_the_custom_dialog_and_cancel_preserves_the_tab(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_bundled_fixture("enter-create-environment");
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    let key = workspace.requests()[0].key();
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture, workspace);
            view.select_request(key, cx);
            view.edit_request(
                key,
                |request| request.url = Some("https://unsaved.example".to_owned()),
                cx,
            );
            view.request_close_tab(key, window, cx);
            assert!(matches!(
                view.application_dialog,
                Some(ApplicationDialog::Unsaved { .. })
            ));
        })
        .unwrap();
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let cancel = visual
        .debug_bounds("application-dialog-cancel")
        .expect("custom dialog Cancel action should be rendered");
    visual.simulate_click(cancel.center(), Modifiers::default());
    visual.run_until_parked();
    cx.run_until_parked();

    window
        .update(cx, |view, _, _| {
            assert!(view.application_dialog.is_none());
            assert!(view.shell.open_tabs().contains(&key.into()));
            assert!(view.request_is_dirty(key));
        })
        .unwrap();
}

#[gpui::test]
fn enter_triggers_application_dialog_primary_action(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    cx.update(bind_platform_hotkeys);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });

    window
        .update(cx, |view, window, cx| {
            view.show_application_dialog(ApplicationDialog::About, window, cx);
            assert!(matches!(
                view.application_dialog,
                Some(ApplicationDialog::About)
            ));
        })
        .unwrap();
    cx.run_until_parked();

    cx.simulate_keystrokes(window.into(), "enter");
    cx.run_until_parked();

    window
        .update(cx, |view, _, _| {
            assert!(view.application_dialog.is_none());
        })
        .unwrap();
}

#[gpui::test]
fn enter_triggers_create_environment_dialog_primary_action(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    cx.update(bind_platform_hotkeys);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_bundled_fixture("enter-create-environment");
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();

    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture, workspace);
            view.open_create_environment_dialog(window, cx);
            if let Some(name) = view.create_environment_dialog.as_mut() {
                *name = "Staging".to_owned();
            }
            assert!(view.create_environment_dialog.is_some());
        })
        .unwrap();
    cx.run_until_parked();

    cx.simulate_keystrokes(window.into(), "enter");
    cx.run_until_parked();

    window
        .update(cx, |view, _, _| {
            assert!(view.create_environment_dialog.is_none());
            assert!(
                view.loaded_workspace
                    .as_ref()
                    .unwrap()
                    .workspace()
                    .environments()
                    .iter()
                    .any(|environment| environment.name == "Staging")
            );
        })
        .unwrap();
}

#[gpui::test]
fn destructive_shortcut_triggers_application_dialog_destructive_action(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    cx.update(bind_platform_hotkeys);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_bundled_fixture("destructive-shortcut");
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    let key = workspace.requests()[0].key();

    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture, workspace);
            view.select_request(key, cx);
            view.edit_request(
                key,
                |request| request.url = Some("https://discard.example".to_owned()),
                cx,
            );
            view.request_close_tab(key, window, cx);
            assert!(matches!(
                view.application_dialog,
                Some(ApplicationDialog::Unsaved { .. })
            ));
        })
        .unwrap();
    cx.run_until_parked();

    cx.simulate_keystrokes(window.into(), destructive_dialog_shortcut());
    cx.run_until_parked();

    window
        .update(cx, |view, _, _| {
            assert!(view.application_dialog.is_none());
            assert!(!view.shell.open_tabs().contains(&key.into()));
            assert!(!view.request_is_dirty(key));
        })
        .unwrap();
}

#[gpui::test]
fn application_dialogs_queue_without_repeating_the_same_filesystem_conflict(
    cx: &mut TestAppContext,
) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let conflict_path = PathBuf::from("collection.yml");

    window
        .update(cx, |view, window, cx| {
            view.show_application_dialog(
                ApplicationDialog::Delete {
                    kind: ItemKind::Request,
                    selector: "products/list".to_owned(),
                    name: "List products".to_owned(),
                    detail: "This cannot be undone.".to_owned(),
                },
                window,
                cx,
            );
            for detail in ["First conflict", "Repeated conflict"] {
                view.show_application_dialog(
                    ApplicationDialog::FilesystemConflict {
                        path: Some(conflict_path.clone()),
                        detail: detail.to_owned(),
                    },
                    window,
                    cx,
                );
            }

            assert!(matches!(
                view.application_dialog,
                Some(ApplicationDialog::Delete { .. })
            ));
            assert_eq!(view.pending_application_dialogs.len(), 1);

            view.handle_application_dialog_action(ApplicationDialogAction::Cancel, window, cx);

            assert!(matches!(
                view.application_dialog,
                Some(ApplicationDialog::FilesystemConflict { .. })
            ));
            assert!(view.pending_application_dialogs.is_empty());
        })
        .unwrap();
}

#[gpui::test]
fn filesystem_reload_merges_against_disk_baselines_and_use_disk_discards_edits(
    cx: &mut TestAppContext,
) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_bundled_fixture("filesystem-reload");
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    let selector = workspace.requests()[0].selector().to_owned();
    let key = workspace.requests()[0].key();
    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), workspace);
            view.edit_request(
                key,
                |request| request.url = Some("https://local.example/draft".to_owned()),
                cx,
            );
        })
        .unwrap();

    let source = fs::read_to_string(&fixture).unwrap();
    fs::write(&fixture, source.replacen("method: GET", "method: PATCH", 1)).unwrap();
    let fresh = probe_opencollection::load_workspace(&fixture).unwrap();
    window
        .update(cx, |view, window, cx| {
            view.reconcile_filesystem_workspace(fresh, BTreeMap::new(), window, cx);
            let loaded = view.loaded_workspace.as_ref().unwrap();
            let key = loaded.request_key(&selector).unwrap();
            let request = loaded.workspace().request(key).unwrap();
            assert_eq!(request.method.as_deref(), Some("PATCH"));
            assert_eq!(request.url.as_deref(), Some("https://local.example/draft"));
            let baseline = view.persistence.saved_request(key).unwrap();
            assert_eq!(baseline.method.as_deref(), Some("PATCH"));
            assert_eq!(
                baseline.url.as_deref(),
                Some("https://api.example.com/pets")
            );
            assert!(view.persistence.is_dirty(key, request));
            view.edit_request(key, |request| request.method = Some("POST".to_owned()), cx);
        })
        .unwrap();

    let source = fs::read_to_string(&fixture).unwrap();
    fs::write(&fixture, source.replacen("method: PATCH", "method: PUT", 1)).unwrap();
    let fresh = probe_opencollection::load_workspace(&fixture).unwrap();
    window
        .update(cx, |view, window, cx| {
            view.reconcile_filesystem_workspace(fresh, BTreeMap::new(), window, cx);
            assert!(matches!(
                view.application_dialog,
                Some(ApplicationDialog::FilesystemConflict { .. })
            ));
            view.handle_application_dialog_action(ApplicationDialogAction::UseDisk, window, cx);
        })
        .unwrap();
    cx.run_until_parked();

    window
        .update(cx, |view, _, _| {
            let loaded = view.loaded_workspace.as_ref().unwrap();
            let key = loaded.request_key(&selector).unwrap();
            let request = loaded.workspace().request(key).unwrap();
            assert_eq!(request.method.as_deref(), Some("PUT"));
            assert_eq!(request.url.as_deref(), Some("https://api.example.com/pets"));
            assert!(!view.persistence.is_dirty(key, request));
        })
        .unwrap();
    fs::remove_file(fixture).unwrap();
}

#[gpui::test]
fn save_conflict_keeps_the_request_dirty_and_visible(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_bundled_fixture("conflict");
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    let key = workspace.requests()[0].key();
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), workspace);
            view.select_request(key, cx);
            view.edit_request(
                key,
                |request| request.url = Some("https://local.example".to_owned()),
                cx,
            );
            let mut external = fs::read_to_string(&fixture).unwrap();
            external.push_str("external: true\n");
            fs::write(&fixture, external).unwrap();
            view.save_active_editor(window, cx);
        })
        .unwrap();
    cx.run_until_parked();

    window
        .update(cx, |view, _, _| {
            let request = view
                .loaded_workspace
                .as_ref()
                .unwrap()
                .workspace()
                .request(key)
                .unwrap();
            assert!(view.persistence.is_dirty(key, request));
            assert!(view.toasts.iter().any(|(_, toast, _)| {
                toast.intent == ToastIntent::Error && toast.message.contains("externally modified")
            }));
            assert_eq!(request.url.as_deref(), Some("https://local.example"));
        })
        .unwrap();
    fs::remove_file(fixture).unwrap();
}

#[gpui::test]
fn recent_collection_in_sidebar_loads_the_workspace(cx: &mut TestAppContext) {
    cx.update(Theme::init);
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
            (view.workspace_path.clone(), view.loading, toast_debug(view))
        })
        .expect("test window should remain open");
    assert_eq!(
        actual.as_deref(),
        Some(expected.as_path()),
        "loading={loading}, message={message:?}"
    );
}

#[gpui::test]
fn recent_collection_can_be_removed_from_the_sidebar(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = bundled_fixture();
    let next = PathBuf::from("/tmp/next-collection.yml");
    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            view.session.recent_collections = vec![fixture.clone(), next.clone()];
            cx.notify();
        })
        .expect("test window should be open");
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let remove = visual
        .debug_bounds("recent-collection-remove-0")
        .expect("recent collection remove button should be rendered");
    visual.simulate_click(remove.center(), Modifiers::default());
    visual.run_until_parked();

    window
        .update(cx, |view, _, _| {
            assert_eq!(view.session.recent_collections, vec![next]);
            assert!(view.loaded_workspace.is_none());
        })
        .expect("test window should remain open");

    window
        .update(cx, |view, window, _| {
            let next_focus = view
                .transient
                .recent_collection_focus_handles
                .borrow()
                .get(&PathBuf::from("/tmp/next-collection.yml"))
                .cloned()
                .expect("next recent collection should have a focus handle");
            assert!(next_focus.is_focused(window));
        })
        .expect("next recent collection should receive focus");
}

#[gpui::test]
fn removing_the_last_recent_collection_restores_sidebar_focus(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            view.session.recent_collections = vec![bundled_fixture()];
            cx.notify();
        })
        .expect("test window should be open");
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let remove = visual
        .debug_bounds("recent-collection-remove-0")
        .expect("recent collection remove button should be rendered");
    visual.simulate_click(remove.center(), Modifiers::default());
    visual.run_until_parked();

    window
        .update(cx, |view, window, _| {
            assert!(view.session.recent_collections.is_empty());
            assert!(
                view.transient
                    .sidebar_import_trigger_focus
                    .is_focused(window)
            );
        })
        .expect("sidebar import trigger should receive focus");
}

#[gpui::test]
fn recent_collection_focus_handles_are_pruned(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let stale = bundled_fixture();
    let current = PathBuf::from("/tmp/current-collection.yml");
    window
        .update(cx, |view, _, cx| {
            view.session.recent_collections = vec![stale.clone()];
            cx.notify();
        })
        .expect("test window should be open");
    cx.run_until_parked();

    window
        .update(cx, |view, _, cx| {
            assert!(
                view.transient
                    .recent_collection_focus_handles
                    .borrow()
                    .contains_key(&stale)
            );
            view.session.recent_collections = vec![current.clone()];
            cx.notify();
        })
        .expect("test window should remain open");
    cx.run_until_parked();

    window
        .update(cx, |view, _, _| {
            let focus_handles = view.transient.recent_collection_focus_handles.borrow();
            assert!(!focus_handles.contains_key(&stale));
            assert!(focus_handles.contains_key(&current));
        })
        .expect("stale focus handles should be pruned");
}

#[gpui::test]
fn empty_sidebar_new_collection_creates_and_loads_a_workspace(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            cx.notify();
        })
        .expect("test window should be open");
    cx.run_until_parked();

    let destination_dir = std::env::temp_dir().join(format!(
        "probe-desktop-new-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&destination_dir).unwrap();
    let destination = destination_dir.join("pets.yml");
    let canonical_destination = destination_dir.canonicalize().unwrap().join("pets.yml");
    window
        .update(cx, |view, _, _| {
            view.session.workspaces.insert(
                canonical_destination.clone(),
                crate::session::WorkspaceSessionState {
                    ordered_tabs: None,
                    active_open_tab: None,
                    open_tabs: vec!["items/0".to_owned()],
                    active_tab: Some("items/0".to_owned()),
                    collapsed_folders: vec!["items/0".to_owned()],
                },
            );
        })
        .unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let button = visual
        .debug_bounds("sidebar-new-collection")
        .expect("new collection button should be rendered");
    visual.simulate_click(button.center(), Modifiers::default());
    visual.run_until_parked();
    cx.run_until_parked();

    assert!(cx.did_prompt_for_new_path());
    cx.simulate_new_path_selection({
        let destination = destination.clone();
        move |_| Some(destination)
    });
    cx.run_until_parked();

    let expected = destination
        .canonicalize()
        .expect("created collection should exist");
    let (actual, name, requests, tabs, remembered, message) = window
        .update(cx, |view, _, _| {
            (
                view.workspace_path.clone(),
                view.loaded_workspace
                    .as_ref()
                    .and_then(|loaded| loaded.workspace().metadata().name.clone()),
                view.loaded_workspace
                    .as_ref()
                    .map(|loaded| loaded.workspace().request_count()),
                view.shell.tabs().count(),
                view.session.workspaces.get(&canonical_destination).cloned(),
                toast_debug(view),
            )
        })
        .expect("test window should remain open");
    assert_eq!(
        actual.as_deref(),
        Some(expected.as_path()),
        "message={message:?}"
    );
    assert_eq!(name.as_deref(), Some("pets"));
    assert_eq!(requests, Some(0));
    assert_eq!(tabs, 0);
    assert_eq!(
        remembered,
        Some(crate::session::WorkspaceSessionState {
            ordered_tabs: Some(Vec::new()),
            ..Default::default()
        })
    );
    fs::remove_dir_all(destination_dir).unwrap();
}

#[gpui::test]
fn empty_sidebar_import_menu_lists_postman_and_yaak(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            cx.notify();
        })
        .expect("test window should be open");
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let trigger = visual
        .debug_bounds("sidebar-import-from")
        .expect("empty sidebar should include Import From");
    visual.simulate_click(trigger.center(), Modifiers::default());
    visual.run_until_parked();
    cx.run_until_parked();

    visual
        .debug_bounds("sidebar-import-postman")
        .expect("provider menu should include Postman");
    visual
        .debug_bounds("sidebar-import-yaak")
        .expect("provider menu should include Yaak");
}

#[gpui::test]
fn workspace_switcher_includes_new_collection(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            cx.notify();
        })
        .expect("test window should be open");
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let trigger = visual
        .debug_bounds("workspace-switcher-trigger")
        .expect("workspace switcher trigger should render");
    visual.simulate_click(trigger.center(), Modifiers::default());
    visual.run_until_parked();
    cx.run_until_parked();

    visual
        .debug_bounds("workspace-switcher-new")
        .expect("workspace switcher should include New Collection");
    visual
        .debug_bounds("workspace-switcher-open")
        .expect("workspace switcher should include Open Collection");
    let import = visual
        .debug_bounds("workspace-switcher-import-from")
        .expect("workspace switcher should include Import From");
    visual.simulate_click(import.center(), Modifiers::default());
    visual.run_until_parked();
    cx.run_until_parked();

    let submenu = visual
        .debug_bounds("workspace-switcher-import-popup")
        .expect("workspace switcher import popup should render");
    let postman = visual
        .debug_bounds("workspace-switcher-import-postman")
        .expect("workspace switcher import menu should include Postman");
    visual
        .debug_bounds("workspace-switcher-import-yaak")
        .expect("workspace switcher import menu should include Yaak");
    assert!(
        submenu.center().x > import.center().x,
        "import submenu should open beside its trigger: trigger={import:?}, submenu={submenu:?}"
    );
    assert_eq!(
        submenu.left(),
        import.right(),
        "the submenu should meet the trigger edge while its surfaces overlap"
    );
    assert_eq!(
        postman.top(),
        import.top(),
        "the first submenu row should align with its trigger row"
    );
}

#[gpui::test]
fn large_sidebar_virtualizes_rows_and_reveals_the_restored_request(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = large_fixture()
        .canonicalize()
        .expect("fixture should exist");
    let workspace =
        probe_opencollection::load_workspace(&fixture).expect("large fixture should load");
    let first = workspace
        .requests()
        .first()
        .expect("request should exist")
        .key();
    let last = workspace
        .requests()
        .last()
        .expect("request should exist")
        .key();
    let last_selector = workspace
        .request_selector(last)
        .expect("request should have a selector")
        .to_owned();
    let first_selector = workspace
        .request_selector(first)
        .expect("request should have a selector")
        .to_owned();
    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            view.set_workspace(fixture, workspace);
            let workspace = view
                .session
                .workspaces
                .entry(view.workspace_path.clone().unwrap())
                .or_default();
            workspace.open_tabs = vec![first_selector, last_selector.clone()];
            workspace.active_tab = Some(last_selector);
            view.restore_shell_state(cx);
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
    window
        .update(cx, |view, _, _| {
            assert_eq!(
                view.selected_tree_item,
                Some(WorkspaceItemRef::Request(last))
            );
            assert!(
                tree_item_is_in_view(view, WorkspaceItemRef::Request(last)),
                "active request should be inside the sidebar viewport"
            );
        })
        .expect("test window should remain open");

    window
        .update(cx, |view, _, cx| view.close_tab_now(last, cx))
        .expect("test window should remain open");
    cx.run_until_parked();

    window
        .update(cx, |view, _, _| {
            assert_eq!(view.shell.active_tab(), Some(first));
            assert_eq!(
                view.selected_tree_item,
                Some(WorkspaceItemRef::Request(first))
            );
            assert!(
                tree_item_is_in_view(view, WorkspaceItemRef::Request(first)),
                "closing the active tab should reveal its selected neighbor"
            );
        })
        .expect("test window should remain open");
}

#[gpui::test]
fn sidebar_folder_selection_and_search_reveal_behavior(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = nested_fixture()
        .canonicalize()
        .expect("fixture should exist");
    let workspace = probe_opencollection::load_workspace(&fixture).expect("fixture should load");
    let folder = workspace
        .folder_key("items/1")
        .expect("folder should exist");
    let nested = workspace
        .request_key("items/1/items/0")
        .expect("nested request should exist");
    let root = workspace
        .request_key("items/0")
        .expect("root request should exist");
    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            view.set_workspace(fixture, workspace);
            view.select_request(root, cx);
            view.shell.collapse_folder(folder);
            view.rebuild_visible_tree_rows();
            cx.notify();
        })
        .expect("test window should be open");
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual
        .debug_bounds("tree-search")
        .expect("sidebar search input should render");

    let folder_row = visual
        .debug_bounds("tree-row-items/1")
        .expect("collapsed folder row should render");
    visual.simulate_click(folder_row.center(), Modifiers::default());
    visual.run_until_parked();
    window
        .update(cx, |view, _, _| {
            assert_eq!(
                view.selected_tree_item,
                Some(WorkspaceItemRef::Folder(folder))
            );
            assert!(
                !view.shell.folder_is_expanded(folder),
                "opening an overview must preserve folder expansion"
            );
        })
        .expect("test window should remain open");

    let disclosure = visual.debug_bounds("folder-disclosure-items/1").unwrap();
    visual.simulate_click(disclosure.center(), Modifiers::default());
    visual.run_until_parked();
    window
        .update(cx, |view, _, _| {
            assert!(view.shell.folder_is_expanded(folder));
            assert_eq!(
                view.shell.active_overview(),
                Some(crate::shell::OverviewTab::Folder(folder))
            );
            assert_eq!(
                view.shell.overview_tabs().collect::<Vec<_>>(),
                &[crate::shell::OverviewTab::Folder(folder)]
            );
        })
        .unwrap();

    window
        .update(cx, |view, _, cx| view.select_request(root, cx))
        .expect("test window should remain open");
    visual.run_until_parked();
    let folder_row = visual
        .debug_bounds("tree-row-items/1")
        .expect("expanded folder row should render");
    visual.simulate_click(folder_row.center(), Modifiers::default());
    visual.run_until_parked();
    window
        .update(cx, |view, _, _| {
            assert_eq!(
                view.selected_tree_item,
                Some(WorkspaceItemRef::Folder(folder))
            );
            assert!(
                view.shell.folder_is_expanded(folder),
                "selecting an expanded folder must not collapse it"
            );
        })
        .expect("test window should remain open");

    // Opening the same folder again focuses the existing overview tab.
    visual.simulate_click(folder_row.center(), Modifiers::default());
    visual.run_until_parked();
    window
        .update(cx, |view, _, _| {
            assert_eq!(view.shell.overview_tabs().count(), 1);
            assert!(view.shell.folder_is_expanded(folder));
        })
        .unwrap();
    let header = visual.debug_bounds("collection-overview-header").unwrap();
    visual.simulate_click(header.center(), Modifiers::default());
    visual.simulate_click(header.center(), Modifiers::default());
    visual.run_until_parked();
    visual
        .debug_bounds("documentation-overview")
        .expect("overview content must render");
    window
        .update(cx, |view, _, cx| {
            assert_eq!(view.shell.overview_tabs().count(), 2);
            assert_eq!(
                view.shell.active_overview(),
                Some(crate::shell::OverviewTab::Collection)
            );
            assert!(view.active_request().is_none());
            view.select_request(root, cx);
            assert_eq!(view.shell.active_tab(), Some(root));
            assert_eq!(view.shell.active_overview(), None);
        })
        .unwrap();

    window
        .update(cx, |view, _, cx| {
            view.shell.collapse_folder(folder);
            view.rebuild_visible_tree_rows();
            cx.notify();
        })
        .expect("test window should be open");
    let collapsed_names = window
        .update(cx, |view, _, _| visible_tree_names(view))
        .expect("test window should remain open");
    assert_eq!(collapsed_names, ["Alpha", "Folder"]);

    window
        .update(cx, |view, _, cx| {
            view.select_request(nested, cx);
        })
        .expect("test window should remain open");
    let expanded_names = window
        .update(cx, |view, _, _| visible_tree_names(view))
        .expect("test window should remain open");
    assert_eq!(expanded_names, ["Alpha", "Folder", "Nested"]);

    window
        .update(cx, |view, _, cx| {
            view.shell.collapse_folder(folder);
            view.set_tree_search("alpha".to_owned(), cx);
            view.select_request(nested, cx);
        })
        .expect("test window should remain open");
    let (filtered_names, folder_expanded) = window
        .update(cx, |view, _, _| {
            (
                visible_tree_names(view),
                view.shell.folder_is_expanded(folder),
            )
        })
        .expect("test window should remain open");
    assert_eq!(filtered_names, ["Alpha"]);
    assert!(
        !folder_expanded,
        "revealing a filtered-out request should preserve collapsed folders"
    );

    window
        .update(cx, |view, _, cx| {
            view.set_tree_search("nstd".to_owned(), cx);
        })
        .expect("test window should remain open");
    cx.run_until_parked();

    let (names, folder_expanded) = window
        .update(cx, |view, _, _| {
            (
                visible_tree_names(view),
                view.shell.folder_is_expanded(folder),
            )
        })
        .expect("test window should remain open");
    assert!(folder_expanded, "matching request should expand its folder");
    assert_eq!(names, ["Folder", "Nested"]);

    window
        .update(cx, |view, _, cx| {
            view.set_tree_search("fldr".to_owned(), cx);
        })
        .expect("test window should remain open");
    cx.run_until_parked();

    let folder_only = window
        .update(cx, |view, _, _| visible_tree_names(view))
        .expect("test window should remain open");
    assert_eq!(folder_only, ["Folder", "Nested"]);
}

#[gpui::test]
fn restored_active_tab_highlights_matching_sidebar_request(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = bundled_fixture()
        .canonicalize()
        .expect("fixture should exist");
    let workspace = probe_opencollection::load_workspace(&fixture).expect("fixture should load");
    let first = workspace.requests()[0].key();
    let second = workspace.requests()[1].key();
    let first_selector = workspace
        .request_selector(first)
        .expect("first request should have a selector")
        .to_owned();
    let second_selector = workspace
        .request_selector(second)
        .expect("second request should have a selector")
        .to_owned();

    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            view.set_workspace(fixture, workspace);
            let workspace = view
                .session
                .workspaces
                .entry(view.workspace_path.clone().unwrap())
                .or_default();
            workspace.open_tabs = vec![first_selector, second_selector.clone()];
            workspace.active_tab = Some(second_selector);
            view.restore_shell_state(cx);
            cx.notify();
        })
        .expect("test window should be open");
    cx.run_until_parked();

    window
        .update(cx, |view, _, _| {
            assert_eq!(view.shell.active_tab(), Some(second));
            assert_eq!(
                view.selected_tree_item,
                Some(WorkspaceItemRef::Request(second))
            );
        })
        .expect("test window should remain open");

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual
        .debug_bounds("request-tree-label")
        .expect("active sidebar request label should render");
}

#[gpui::test]
fn workspace_reload_preserves_request_section_scroll_owner_after_multiple_remaps(
    cx: &mut TestAppContext,
) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = writable_bundled_fixture("reload-scroll-owner-multi");
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            view.set_workspace(fixture.clone(), workspace);

            let old = view.loaded_workspace.as_ref().unwrap();
            let old_key = old.requests()[0].key();
            view.select_request(old_key, cx);
            view.request_editor
                .set_section(old_key, EditorSection::Headers);

            view.request_section_scroll_owner
                .set(Some((old_key, EditorSection::Headers)));
        })
        .unwrap();

    for iteration in 1..=3 {
        window
            .update(cx, |view, _, _| {
                let old = view.loaded_workspace.as_ref().unwrap();
                let old_key = view.shell.active_tab().unwrap();
                let fresh = probe_opencollection::load_workspace(&fixture).unwrap();
                let selector_remaps = old
                    .requests()
                    .iter()
                    .map(|located| (located.selector().to_owned(), located.selector().to_owned()))
                    .collect::<BTreeMap<_, _>>();
                let key_remaps = request_key_remaps(old, &fresh, &selector_remaps);
                let baselines = fresh
                    .requests()
                    .iter()
                    .filter_map(|located| {
                        fresh
                            .workspace()
                            .request(located.key())
                            .cloned()
                            .map(|request| (located.key(), request))
                    })
                    .collect::<Vec<_>>();
                let new_key = key_remaps[&old_key];

                view.install_reloaded_workspace(fresh, baselines, &key_remaps, &BTreeMap::new());

                let scroll_owner = view.request_section_scroll_owner.get();
                assert_eq!(
                    scroll_owner,
                    Some((new_key, EditorSection::Headers)),
                    "scroll owner should be remapped after reload {iteration}"
                );
            })
            .unwrap();
    }

    fs::remove_file(fixture).unwrap();
}

fn writable_documentation_fixture(name: &str) -> PathBuf {
    let path = writable_structure_fixture(name);
    fs::write(
        &path,
        include_str!("../../../../../tests/fixtures/opencollection/documentation.yml"),
    )
    .unwrap();
    path
}

#[gpui::test]
fn committed_documentation_save_keeps_drafts_until_recovery_integrates(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    for (folder, reload_fails, later_edit) in [
        (false, true, false),
        (false, false, false),
        (false, false, true),
        (true, false, false),
    ] {
        let path = writable_documentation_fixture(&format!(
            "documentation-recovery-{folder}-{reload_fails}-{later_edit}"
        ));
        let workspace = probe_opencollection::load_workspace(&path).unwrap();
        let replacement = probe_opencollection::load_workspace(&path).unwrap();
        let target = if folder {
            OverviewTarget::Folder("items/0".to_owned())
        } else {
            OverviewTarget::Collection
        };
        let recovery_path = if reload_fails {
            path.with_extension("missing.yml")
        } else {
            path.clone()
        };
        let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
            ProbeApp::new(window, cx)
        });
        let (previous, original) = window
            .update(cx, |view, window, cx| {
                view.session_store = None;
                view.set_workspace(recovery_path, workspace);
                let tab = if folder {
                    crate::shell::OverviewTab::Folder(
                        view.loaded_workspace
                            .as_ref()
                            .unwrap()
                            .folder_key("items/0")
                            .unwrap(),
                    )
                } else {
                    crate::shell::OverviewTab::Collection
                };
                view.shell.open_overview(tab);
                view.edit_overview(target.clone(), true, "Committed documentation".into(), cx);
                let original = view.overview_drafts[&target].original.clone();
                view.save_active_editor(window, cx);
                assert!(view.documentation_save_task.is_some());
                // Keep the prepared baseline alive so the write commits, but make
                // completion encounter a different loaded repository baseline.
                let previous = view.loaded_workspace.replace(replacement);
                if later_edit {
                    view.edit_overview(target.clone(), true, "Later documentation".into(), cx);
                }
                assert!(view.overview_drafts[&target].is_dirty());
                (previous, original)
            })
            .unwrap();
        cx.run_until_parked();
        let disk = probe_opencollection::load_workspace(&path).unwrap();
        let disk_docs = if folder {
            disk.workspace()
                .folder(disk.folder_key("items/0").unwrap())
                .unwrap()
                .docs
                .as_ref()
        } else {
            disk.workspace().metadata().docs.as_ref()
        };
        assert_eq!(
            crate::app::documentation::documentation_text(disk_docs),
            Some("Committed documentation"),
            "the save must have reached disk"
        );
        window
            .update(cx, |view, window, cx| {
                assert!(!view.loading);
                assert!(view.documentation_save_task.is_none());
                let draft = &view.overview_drafts[&target];
                assert_eq!(
                    crate::app::documentation::documentation_text(draft.current.docs.as_ref()),
                    Some(if later_edit {
                        "Later documentation"
                    } else {
                        "Committed documentation"
                    })
                );
                if reload_fails {
                    assert_eq!(
                        draft.original, original,
                        "failed recovery must retain the original baseline"
                    );
                    assert!(draft.is_dirty());
                    assert!(
                        toast_debug(view).iter().any(|message| message.contains(
                            "Save reached disk, but the collection could not be reloaded"
                        ))
                    );
                    assert!(
                        !view.request_close_window(window, cx),
                        "failed recovery must retain unsaved-change protection"
                    );
                } else {
                    assert_eq!(draft.original, view.overview_content(&target).unwrap());
                    assert_eq!(
                        draft.is_dirty(),
                        later_edit,
                        "successful recovery may only clear the submitted edits"
                    );
                }
            })
            .unwrap();
        drop(previous);
        fs::remove_file(path).unwrap();
    }
}

#[gpui::test]
fn documentation_editors_save_preserve_media_types_and_keep_later_edits_dirty(
    cx: &mut TestAppContext,
) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1000.0), px(1000.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    cx.update(bind_platform_hotkeys);
    let path = writable_documentation_fixture("documentation-editors");
    let workspace = probe_opencollection::load_workspace(&path).unwrap();
    let folder = workspace.folder_key("items/0").unwrap();
    let request = workspace.request_key("items/0/items/0").unwrap();
    let original_source = fs::read_to_string(&path).unwrap();
    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            view.set_workspace(path.clone(), workspace);
            view.shell
                .open_overview(crate::shell::OverviewTab::Collection);
            cx.notify();
        })
        .unwrap();
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let save = visual
        .debug_bounds("editor-save")
        .expect("overview uses the editor save icon");
    visual.simulate_click(save.center(), Modifiers::default());
    visual.run_until_parked();
    window
        .update(cx, |view, _, _| {
            assert!(!view.has_dirty_overviews());
            assert!(
                view.documentation_save_task.is_none(),
                "clean save icons must be disabled"
            );
        })
        .unwrap();
    for (selector, text) in [
        ("documentation-first-editor", "Edited summary"),
        ("documentation-docs-editor", "Edited collection guide"),
    ] {
        let field = visual.debug_bounds(selector).unwrap();
        visual.simulate_click(
            field.origin + point(px(20.0), px(20.0)),
            Modifiers::default(),
        );
        visual.simulate_keystrokes(if cfg!(target_os = "macos") {
            "cmd-a"
        } else {
            "ctrl-a"
        });
        visual.simulate_input(text);
        visual.run_until_parked();
    }
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        original_source,
        "typing must not write files"
    );
    window
        .update(cx, |view, window, cx| {
            assert!(view.has_dirty_overviews());
            view.save_active_editor(window, cx);
            assert!(view.documentation_save_task.is_some());
            view.edit_overview(
                OverviewTarget::Collection,
                true,
                "Newer collection guide".into(),
                cx,
            );
        })
        .unwrap();
    visual.run_until_parked();
    let saved = probe_opencollection::load_workspace(&path).unwrap();
    assert_eq!(
        saved.workspace().metadata().docs,
        Some(probe_core::Documentation::Content {
            content: "Edited collection guide".into(),
            media_type: "text/markdown".into()
        })
    );
    window
        .update(cx, |view, window, cx| {
            assert!(
                view.has_dirty_overviews(),
                "edits made during a save must remain dirty"
            );
            view.save_active_editor(window, cx);
        })
        .unwrap();
    visual.run_until_parked();
    window
        .update(cx, |view, _, cx| {
            assert!(!view.has_dirty_overviews());
            view.shell
                .open_overview(crate::shell::OverviewTab::Folder(folder));
            cx.notify();
        })
        .unwrap();
    visual.run_until_parked();
    for (selector, text) in [
        ("documentation-first-editor", "Edited folder description"),
        ("documentation-docs-editor", "Edited folder docs"),
    ] {
        let field = visual.debug_bounds(selector).unwrap();
        visual.simulate_click(
            field.origin + point(px(20.0), px(20.0)),
            Modifiers::default(),
        );
        visual.simulate_keystrokes(if cfg!(target_os = "macos") {
            "cmd-a"
        } else {
            "ctrl-a"
        });
        visual.simulate_input(text);
        visual.run_until_parked();
    }
    let save = visual.debug_bounds("editor-save").unwrap();
    visual.simulate_click(save.center(), Modifiers::default());
    visual.run_until_parked();
    window
        .update(cx, |view, _, cx| {
            assert!(!view.has_dirty_overviews(), "{:?}", toast_debug(view));
            view.select_request(request, cx);
            view.request_editor
                .set_section(request, EditorSection::Docs);
            cx.notify();
        })
        .unwrap();
    visual.run_until_parked();
    for (selector, text) in [
        ("documentation-first-editor", "Edited request description"),
        ("documentation-docs-editor", "Edited request docs"),
    ] {
        let field = visual.debug_bounds(selector).unwrap();
        visual.simulate_click(
            field.origin + point(px(20.0), px(20.0)),
            Modifiers::default(),
        );
        visual.simulate_keystrokes(if cfg!(target_os = "macos") {
            "cmd-a"
        } else {
            "ctrl-a"
        });
        visual.simulate_input(text);
        visual.run_until_parked();
    }
    window
        .update(cx, |view, _, _| assert!(view.request_is_dirty(request)))
        .unwrap();
    visual.simulate_keystrokes(super::save_shortcut());
    visual.run_until_parked();
    window
        .update(cx, |view, _, _| assert!(!view.request_is_dirty(request)))
        .unwrap();
    fs::remove_file(path).unwrap();
}

#[gpui::test]
fn documentation_close_prompts_and_save_failures_preserve_drafts(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let path = writable_documentation_fixture("documentation-close");
    let workspace = probe_opencollection::load_workspace(&path).unwrap();
    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(path.clone(), workspace);
            let tab = crate::shell::OverviewTab::Collection;
            view.shell.open_overview(tab);
            view.edit_overview(
                OverviewTarget::Collection,
                false,
                "Local summary".into(),
                cx,
            );
            view.request_close_overview(tab, window, cx);
            assert!(matches!(
                view.application_dialog,
                Some(ApplicationDialog::Unsaved { .. })
            ));
            view.handle_application_dialog_action(ApplicationDialogAction::Cancel, window, cx);
            assert!(view.has_dirty_overviews());
            assert_eq!(view.shell.active_overview(), Some(tab));
            view.request_close_overview(tab, window, cx);
            view.handle_application_dialog_action(ApplicationDialogAction::Save, window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |view, window, cx| {
            assert!(!view.has_dirty_overviews(), "{:?}", toast_debug(view));
            assert!(view.shell.overview_tabs().next().is_none());
            view.shell
                .open_overview(crate::shell::OverviewTab::Collection);
            view.edit_overview(OverviewTarget::Collection, true, "Unsaved docs".into(), cx);
            let external = format!("{}external: retained\n", fs::read_to_string(&path).unwrap());
            fs::write(&path, external).unwrap();
            view.save_active_editor(window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |view, window, cx| {
            assert!(view.has_dirty_overviews());
            assert!(view.documentation_save_task.is_none());
            assert!(
                toast_debug(view)
                    .iter()
                    .any(|message| message.contains("Could not save documentation"))
            );
            let external = fs::read_to_string(&path)
                .unwrap()
                .replace("content: Collection guide", "content: External guide");
            fs::write(&path, &external).unwrap();
            let fresh = probe_opencollection::load_workspace(&path).unwrap();
            view.reconcile_filesystem_workspace(fresh, BTreeMap::new(), window, cx);
            view.save_active_editor(window, cx);
            assert!(
                view.documentation_save_task.is_none(),
                "overlapping disk edits must be rejected before a write"
            );
            assert!(view.has_dirty_overviews());
            assert!(
                toast_debug(view)
                    .iter()
                    .any(|message| message.contains("Documentation changed on disk"))
            );
            assert_eq!(fs::read_to_string(&path).unwrap(), external);
            assert!(!view.request_close_window(window, cx));
            assert!(matches!(
                view.application_dialog,
                Some(ApplicationDialog::Unsaved { .. })
            ));
            view.handle_application_dialog_action(ApplicationDialogAction::Cancel, window, cx);
            view.request_close_workspace(window, cx);
            view.handle_application_dialog_action(ApplicationDialogAction::Discard, window, cx);
            assert!(view.loaded_workspace.is_none());
            assert!(!view.has_dirty_overviews());
        })
        .unwrap();
    assert!(
        fs::read_to_string(&path)
            .unwrap()
            .contains("external: retained")
    );
    fs::remove_file(path).unwrap();
}

fn wait_for_shortcut_request(
    window: gpui::WindowHandle<ProbeApp>,
    key: probe_core::RequestKey,
    cx: &mut TestAppContext,
) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        cx.run_until_parked();
        let finished = window
            .update(cx, |view, _, _| {
                view.execution
                    .response(key)
                    .is_some_and(|state| !state.is_running())
            })
            .unwrap();
        if finished {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "shortcut request did not finish"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[gpui::test]
fn send_request_shortcut_sends_the_active_request(cx: &mut TestAppContext) {
    // Real HTTP runs on Tokio; permit external wakes before dispatching Send.
    cx.executor().allow_parking();
    cx.update(Theme::init);
    cx.update(bind_platform_hotkeys);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = bundled_fixture().canonicalize().unwrap();
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    let request_key = workspace.requests()[0].key();

    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            view.set_workspace(fixture, workspace);
            view.select_request(request_key, cx);
            view.edit_request(
                request_key,
                |request| request.url = Some("http://127.0.0.1:1/test".into()),
                cx,
            );
            assert_eq!(view.shell.active_tab(), Some(request_key));
            assert!(view.execution.response(request_key).is_none());
        })
        .unwrap();
    cx.run_until_parked();

    cx.simulate_keystrokes(window.into(), super::send_shortcut());
    wait_for_shortcut_request(window, request_key, cx);

    window
        .update(cx, |view, _, _| {
            assert!(matches!(
                view.execution.response(request_key),
                Some(crate::execution::ResponseState::Failed(_))
            ));
        })
        .unwrap();
    cx.run_until_parked();
}

#[gpui::test]
fn send_request_shortcut_works_when_input_focused(cx: &mut TestAppContext) {
    // Real HTTP runs on Tokio; permit external wakes before dispatching Send.
    cx.executor().allow_parking();
    cx.update(Theme::init);
    cx.update(bind_platform_hotkeys);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = bundled_fixture().canonicalize().unwrap();
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    let request_key = workspace.requests()[0].key();

    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            view.set_workspace(fixture, workspace);
            view.select_request(request_key, cx);
            view.edit_request(
                request_key,
                |request| request.url = Some("http://127.0.0.1:1/test".into()),
                cx,
            );
        })
        .unwrap();
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let url_input = visual
        .debug_bounds("request-url-input")
        .expect("url input should exist");
    visual.simulate_click(url_input.center(), Modifiers::default());
    visual.run_until_parked();

    visual.simulate_keystrokes(super::send_shortcut());
    drop(visual);
    wait_for_shortcut_request(window, request_key, cx);

    window
        .update(cx, |view, _, _| {
            assert!(matches!(
                view.execution.response(request_key),
                Some(crate::execution::ResponseState::Failed(_))
            ));
        })
        .unwrap();
    cx.run_until_parked();
}

#[gpui::test]
fn send_request_shortcut_does_nothing_on_overview_tab(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    cx.update(bind_platform_hotkeys);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = bundled_fixture().canonicalize().unwrap();
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    let requests = workspace.requests().to_vec();

    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            view.set_workspace(fixture, workspace);
            view.select_open_tab(crate::shell::OverviewTab::Collection.into(), cx);
            assert!(view.shell.active_tab().is_none());
        })
        .unwrap();
    cx.run_until_parked();

    cx.simulate_keystrokes(window.into(), super::send_shortcut());
    cx.run_until_parked();

    window
        .update(cx, |view, _, _| {
            for located in &requests {
                assert!(view.execution.response(located.key()).is_none());
            }
        })
        .unwrap();
}

#[gpui::test]
fn send_request_shortcut_does_nothing_when_dialog_is_open(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    cx.update(bind_platform_hotkeys);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = bundled_fixture().canonicalize().unwrap();
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    let request_key = workspace.requests()[0].key();

    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture, workspace);
            view.select_request(request_key, cx);
            window.dispatch_action(Box::new(NewRequest), cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |view, _, _| {
            assert!(view.structure_dialog.is_some());
        })
        .unwrap();
    cx.run_until_parked();

    cx.simulate_keystrokes(window.into(), super::send_shortcut());
    cx.run_until_parked();

    window
        .update(cx, |view, _, _| {
            assert!(view.execution.response(request_key).is_none());
        })
        .unwrap();
}

#[gpui::test]
fn send_request_shortcut_does_nothing_when_create_environment_dialog_is_open(
    cx: &mut TestAppContext,
) {
    cx.update(Theme::init);
    cx.update(bind_platform_hotkeys);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = bundled_fixture().canonicalize().unwrap();
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    let request_key = workspace.requests()[0].key();

    window
        .update(cx, |view, window, cx| {
            view.session_store = None;
            view.set_workspace(fixture, workspace);
            view.select_request(request_key, cx);
            view.open_create_environment_dialog(window, cx);
            assert!(view.create_environment_dialog.is_some());
        })
        .unwrap();
    cx.run_until_parked();

    cx.simulate_keystrokes(window.into(), super::send_shortcut());
    cx.run_until_parked();

    window
        .update(cx, |view, _, _| {
            assert!(view.execution.response(request_key).is_none());
        })
        .unwrap();
}

#[gpui::test]
fn send_request_shortcut_does_not_duplicate_when_already_running(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    cx.update(bind_platform_hotkeys);
    let window = cx.open_window(size(px(900.0), px(640.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = bundled_fixture().canonicalize().unwrap();
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    let request_key = workspace.requests()[0].key();
    let (cancellation_sender, mut cancellation_receiver) = oneshot::channel();

    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            view.set_workspace(fixture, workspace);
            view.select_request(request_key, cx);
            view.edit_request(
                request_key,
                |request| request.url = Some("http://127.0.0.1:1/test".into()),
                cx,
            );
            view.execution.begin(request_key, cancellation_sender);
            assert!(
                view.execution
                    .response(request_key)
                    .is_some_and(crate::execution::ResponseState::is_running)
            );
        })
        .unwrap();
    cx.run_until_parked();

    cx.simulate_keystrokes(window.into(), super::send_shortcut());
    cx.run_until_parked();

    window
        .update(cx, |view, _, _| {
            assert!(
                view.execution
                    .response(request_key)
                    .is_some_and(crate::execution::ResponseState::is_running)
            );
        })
        .unwrap();
    assert_eq!(
        cancellation_receiver.try_recv(),
        Err(oneshot::error::TryRecvError::Empty),
        "the shortcut must leave the original execution active"
    );
    window
        .update(cx, |view, _, cx| view.cancel_request(request_key, cx))
        .unwrap();
    assert_eq!(cancellation_receiver.try_recv(), Ok(()));
}

#[gpui::test]
fn send_request_shortcut_from_body_editor_preserves_multiline_body(cx: &mut TestAppContext) {
    let body_text = |view: &ProbeApp| {
        let Some(probe_core::RequestBody::Single(probe_core::Body::Raw(raw))) =
            view.active_request().unwrap().http_body()
        else {
            panic!("expected a raw body");
        };
        raw.data.clone()
    };
    // Real HTTP runs on Tokio; permit external wakes before dispatching Send.
    cx.executor().allow_parking();
    cx.update(Theme::init);
    cx.update(bind_platform_hotkeys);
    let window = cx.open_window(size(px(900.0), px(640.0)), ProbeApp::new);
    let fixture = bundled_fixture().canonicalize().unwrap();
    let workspace = probe_opencollection::load_workspace(&fixture).unwrap();
    let request_key = workspace.requests()[0].key();
    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            view.set_workspace(fixture, workspace);
            view.select_request(request_key, cx);
            view.request_editor
                .set_section(request_key, EditorSection::Body);
            view.change_body_kind(request_key, BodyEditorKind::Text, cx);
            view.edit_request(
                request_key,
                |request| {
                    request.url = Some("http://127.0.0.1:1/test".into());
                },
                cx,
            );
        })
        .unwrap();
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let editor = visual.debug_bounds("request-body-editor").unwrap();
    visual.simulate_click(
        editor.origin + point(px(20.0), px(20.0)),
        Modifiers::default(),
    );
    visual.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-a"
    } else {
        "ctrl-a"
    });
    visual.simulate_input("first line");
    visual.simulate_keystrokes("enter");
    visual.simulate_input("second line");
    visual.run_until_parked();
    window
        .update(cx, |view, _, _| {
            assert_eq!(body_text(view), "first line\nsecond line");
            assert!(view.execution.response(request_key).is_none());
        })
        .unwrap();
    visual.simulate_keystrokes(super::send_shortcut());
    drop(visual);
    wait_for_shortcut_request(window, request_key, cx);
    window
        .update(cx, |view, _, _| {
            assert!(matches!(
                view.execution.response(request_key),
                Some(crate::execution::ResponseState::Failed(_))
            ));
            assert_eq!(body_text(view), "first line\nsecond line");
        })
        .unwrap();
    cx.run_until_parked();
}
