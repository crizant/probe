use super::*;

#[gpui::test]
fn folder_breadcrumbs_navigate_to_reusable_overviews_and_preserve_drafts(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/opencollection/breadcrumbs.yml")
        .canonicalize()
        .expect("fixture should exist");
    let workspace = probe_opencollection::load_workspace(&fixture).expect("fixture should load");
    let parent = workspace.folder_key("items/0").unwrap();
    let child = workspace.folder_key("items/0/items/0").unwrap();
    let request = workspace.request_key("items/0/items/0/items/0").unwrap();
    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            view.set_workspace(fixture, workspace);
            view.select_request(request, cx);
        })
        .unwrap();
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let segment = visual.debug_bounds("request-breadcrumb-folder-1").unwrap();
    visual.simulate_click(segment.center(), Modifiers::default());
    visual.run_until_parked();
    window
        .update(cx, |view, _, cx| {
            assert_eq!(
                view.shell.active_overview(),
                Some(crate::shell::OverviewTab::Folder(child))
            );
            assert_eq!(
                view.selected_tree_item,
                Some(WorkspaceItemRef::Folder(child))
            );
            let target = view
                .overview_target(crate::shell::OverviewTab::Folder(child))
                .unwrap();
            view.edit_overview(target, true, "Unsaved child documentation".into(), cx);
        })
        .unwrap();
    visual.run_until_parked();
    assert!(visual.debug_bounds("folder-breadcrumb-folder-1").is_some());
    let segment = visual.debug_bounds("folder-breadcrumb-folder-0").unwrap();
    visual.simulate_click(segment.center(), Modifiers::default());
    visual.run_until_parked();
    window
        .update(cx, |view, _, cx| {
            assert_eq!(
                view.shell.active_overview(),
                Some(crate::shell::OverviewTab::Folder(parent))
            );
            assert_eq!(
                view.selected_tree_item,
                Some(WorkspaceItemRef::Folder(parent))
            );
            assert_eq!(view.shell.overview_tabs().len(), 2);
            assert!(view.has_dirty_overviews());
            view.select_request(request, cx);
        })
        .unwrap();
    visual.run_until_parked();
    let segment = visual.debug_bounds("request-breadcrumb-folder-1").unwrap();
    visual.simulate_click(segment.center(), Modifiers::default());
    visual.run_until_parked();
    let segment = visual.debug_bounds("folder-breadcrumb-folder-1").unwrap();
    visual.simulate_click(segment.center(), Modifiers::default());
    visual.run_until_parked();
    window
        .update(cx, |view, _, _| {
            assert_eq!(
                view.shell.active_overview(),
                Some(crate::shell::OverviewTab::Folder(child))
            );
            assert_eq!(view.shell.overview_tabs().len(), 2);
            let target = view
                .overview_target(crate::shell::OverviewTab::Folder(child))
                .unwrap();
            assert_eq!(
                view.overview_drafts[&target].current.docs,
                Some(probe_core::Documentation::Text(
                    "Unsaved child documentation".into()
                ))
            );
        })
        .unwrap();
}

#[gpui::test]
fn request_editor_sections_render_for_an_open_request(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = bundled_fixture()
        .canonicalize()
        .expect("fixture should exist");
    let workspace = probe_opencollection::load_workspace(&fixture).expect("fixture should load");
    let request_key = workspace.requests()[0].key();
    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            view.set_workspace(fixture, workspace);
            view.select_request(request_key, cx);
        })
        .expect("test window should be open");

    for section in EditorSection::ALL_HTTP {
        window
            .update(cx, |view, _, cx| {
                view.request_editor.set_section(request_key, section);
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
            assert!(visual.debug_bounds("request-breadcrumb").is_some());
            assert!(visual.debug_bounds("request-breadcrumb-folder-0").is_some());
            assert!(visual.debug_bounds("request-breadcrumb-request").is_some());
            assert!(visual.debug_bounds("request-protocol-label").is_some());
            assert!(visual.debug_bounds("request-breadcrumb-protocol").is_none());
            assert!(
                visual
                    .debug_bounds("request-protocol-label")
                    .unwrap()
                    .right()
                    <= visual
                        .debug_bounds("request-breadcrumb-folder-0")
                        .unwrap()
                        .left(),
                "the protocol label should precede the breadcrumb"
            );
            assert!(visual.debug_bounds("request-method-trigger").is_some());
            assert!(visual.debug_bounds("request-environment-trigger").is_some());
            if section == EditorSection::Body {
                assert!(
                    visual.debug_bounds("request-body-editor").is_some(),
                    "JSON body editor should render"
                );
            }
        }
    }
}

#[gpui::test]
fn request_send_menu_offers_streaming_the_response_to_a_file(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = bundled_fixture()
        .canonicalize()
        .expect("fixture should exist");
    let source = format!(
        "{}\nconfig:\n  environments:\n    - name: curl-export\n      variables:\n        - name: host\n          value: https://example.com\n",
        fs::read_to_string(&fixture).unwrap()
    );
    let workspace = probe_opencollection::load_workspace_from_str(&source).unwrap();
    let body_path = crate::filesystem::workspace_base_directory(&fixture)
        .unwrap()
        .join("current-draft-body.bin");

    let request_key = workspace.requests()[0].key();
    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            view.set_workspace(fixture, workspace);
            view.select_request(request_key, cx);
            view.select_environment(Some("curl-export".into()), cx);
            view.edit_request(
                request_key,
                |request| {
                    request.url = Some("{{host}}/current-draft".into());
                    request.kind = probe_core::RequestKind::Http {
                        body: Some(probe_core::RequestBody::Single(probe_core::Body::File(
                            vec![probe_core::FileReference {
                                file_path: "current-draft-body.bin".into(),
                                content_type: "application/octet-stream".into(),
                                selected: true,
                            }],
                        ))),
                    };
                },
                cx,
            );
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(String::new()));
        })
        .expect("test window should be open");
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let trigger = visual
        .debug_bounds("request-execution-menu-trigger")
        .expect("send options should render");
    visual.simulate_click(trigger.center(), Modifiers::default());
    visual.run_until_parked();
    let copy_item = visual
        .debug_bounds("request-copy-as-curl")
        .expect("HTTP requests should offer Copy as cURL");
    visual.simulate_click(copy_item.center(), Modifiers::default());
    drop(visual);
    cx.executor().allow_parking();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let command = loop {
        cx.run_until_parked();
        if let Some(command) = cx
            .update(|cx| cx.read_from_clipboard().and_then(|item| item.text()))
            .filter(|text| text.starts_with("curl "))
        {
            break command;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "clipboard export timed out: {:?}",
            window.update(cx, |view, _, _| toast_debug(view)).unwrap()
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(command.starts_with("curl "));
    assert!(command.contains("https://example.com/current-draft"));
    let body_argument = format!("--data-binary '@{}'", body_path.display());
    assert!(command.contains(&body_argument));
    window
        .update(cx, |view, _, _| {
            assert!(!view.transient.request_execution_menu_open);
            assert!(view.execution.response(request_key).is_none());
        })
        .unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let trigger = visual
        .debug_bounds("request-execution-menu-trigger")
        .unwrap();
    visual.simulate_click(trigger.center(), Modifiers::default());
    visual.run_until_parked();
    let menu_item = visual
        .debug_bounds("request-send-and-save")
        .expect("send and save action should render");
    visual.simulate_click(menu_item.center(), Modifiers::default());
    visual.run_until_parked();

    assert!(cx.did_prompt_for_new_path());
    cx.simulate_new_path_selection(|_| None);
    cx.run_until_parked();
    window
        .update(cx, |view, _, cx| {
            view.edit_request(
                request_key,
                |request| {
                    request.method = Some("POST".into());
                    request.kind = probe_core::RequestKind::Graphql { body: None };
                },
                cx,
            );
        })
        .unwrap();
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let trigger = visual
        .debug_bounds("request-execution-menu-trigger")
        .unwrap();
    visual.simulate_click(trigger.center(), Modifiers::default());
    visual.run_until_parked();
    assert!(visual.debug_bounds("request-send-and-save").is_some());
    let copy_item = visual
        .debug_bounds("request-copy-as-curl")
        .expect("GraphQL requests should offer Copy as cURL");
    visual.simulate_click(copy_item.center(), Modifiers::default());
    drop(visual);
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        cx.run_until_parked();
        if cx
            .update(|cx| cx.read_from_clipboard().and_then(|item| item.text()))
            .is_some_and(|text| text.contains("--data-raw '{}'"))
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "GraphQL clipboard export timed out: clipboard={:?}, toast={:?}",
            cx.update(|cx| cx.read_from_clipboard().and_then(|item| item.text())),
            window.update(cx, |view, _, _| toast_debug(view)).unwrap()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    window
        .update(cx, |view, _, _| {
            assert!(!view.transient.request_execution_menu_open);
            assert!(view.execution.response(request_key).is_none());
        })
        .unwrap();
}

#[gpui::test]
fn response_progress_renders_after_headers_arrive(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = bundled_fixture()
        .canonicalize()
        .expect("fixture should exist");
    let workspace = probe_opencollection::load_workspace(&fixture).expect("fixture should load");
    let request_key = workspace.requests()[0].key();
    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            view.set_workspace(fixture, workspace);
            view.select_request(request_key, cx);
            let (cancellation, _) = tokio::sync::oneshot::channel();
            let generation = view.execution.begin(request_key, cancellation);
            view.execution.report_progress(
                request_key,
                generation,
                probe_http::HttpProgress::ResponseStarted {
                    status: 200,
                    reason: "OK".to_owned(),
                    content_length: Some(120 * 1024 * 1024),
                },
            );
            view.execution.report_progress(
                request_key,
                generation,
                probe_http::HttpProgress::BodyReceived {
                    bytes: 38 * 1024 * 1024,
                },
            );
            cx.notify();
        })
        .expect("test window should be open");
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("response-status-code").is_some());
    assert!(visual.debug_bounds("response-metadata").is_some());
    assert!(visual.debug_bounds("response-receiving-body").is_some());
}

#[gpui::test]
fn completed_response_renders_pretty_raw_headers_and_search(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = bundled_fixture()
        .canonicalize()
        .expect("fixture should exist");
    let workspace = probe_opencollection::load_workspace(&fixture).expect("fixture should load");
    let request_key = workspace.requests()[0].key();
    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            view.set_workspace(fixture, workspace);
            view.select_request(request_key, cx);
            let (cancellation, _) = tokio::sync::oneshot::channel();
            let generation = view.execution.begin(request_key, cancellation);
            let body = br#"{"createdAt":1787482800,"ok":true}"#.to_vec();
            view.complete_execution(
                request_key,
                generation,
                Ok(HttpResponse {
                    status: 201,
                    reason: "Created".to_owned(),
                    url: "https://api.example.test/users".to_owned(),
                    duration: Duration::from_millis(42),
                    size: body.len(),
                    headers: vec![ResponseHeader {
                        name: "content-type".to_owned(),
                        value: "application/json".to_owned(),
                    }],
                    body,
                    body_complete: true,
                    body_file: None,
                    body_retention_error: None,
                }),
                None,
                cx,
            );
            cx.notify();
        })
        .expect("test window should be open");
    cx.run_until_parked();

    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        assert!(visual.debug_bounds("response-status").is_some());
        assert!(visual.debug_bounds("response-status-code").is_some());
        assert!(visual.debug_bounds("response-metadata").is_some());
        assert!(visual.debug_bounds("response-save-body").is_some());
        assert!(visual.debug_bounds("response-tab-pretty").is_some());
        assert!(visual.debug_bounds("response-tab-raw").is_some());
        assert!(visual.debug_bounds("response-tab-headers").is_some());
        assert!(visual.debug_bounds("response-resize-handle").is_some());
        assert!(visual.debug_bounds("sidebar-resize-handle").is_some());
        assert!(visual.debug_bounds("response-raw-view-text").is_none());
        assert!(visual.debug_bounds("response-raw-view-base64").is_none());
        assert!(visual.debug_bounds("response-search").is_none());
        assert!(visual.debug_bounds("editor-search-card").is_none());
        assert!(visual.debug_bounds("response-body").is_some());
        assert!(visual.debug_bounds("response-headers").is_none());
    }

    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let save = visual
            .debug_bounds("response-save-body")
            .expect("completed response should offer saving its body");
        visual.simulate_click(save.center(), Modifiers::default());
        visual.run_until_parked();
    }
    assert!(cx.did_prompt_for_new_path());
    cx.simulate_new_path_selection(|_| None);
    cx.run_until_parked();

    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let body_bounds = visual
            .debug_bounds("response-body")
            .expect("response body should render");
        visual.simulate_click(body_bounds.center(), Modifiers::default());
    }
    cx.simulate_keystrokes(window.into(), find_shortcut());
    cx.run_until_parked();
    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        assert!(visual.debug_bounds("editor-search-card").is_some());
        assert!(visual.debug_bounds("editor-search-input").is_some());
        assert!(visual.debug_bounds("response-search").is_none());
    }
    cx.simulate_keystrokes(window.into(), "o k");
    cx.run_until_parked();
    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        assert!(visual.debug_bounds("editor-search-card").is_some());
        assert!(visual.debug_bounds("response-body").is_some());
    }
    cx.simulate_keystrokes(window.into(), "escape");
    cx.run_until_parked();
    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        assert!(visual.debug_bounds("editor-search-card").is_none());
    }

    window
        .update(cx, |view, _, cx| {
            view.set_response_tab(ResponseViewerTab::Raw, cx);
        })
        .expect("test window should remain open");
    cx.run_until_parked();
    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        assert!(visual.debug_bounds("response-raw-view-text").is_some());
        let base64 = visual
            .debug_bounds("response-raw-view-base64")
            .expect("raw Base64 sub-tab should render");
        visual.simulate_click(base64.center(), Modifiers::default());
    }
    cx.run_until_parked();
    window
        .update(cx, |view, _, _| {
            assert_eq!(
                view.response_viewer.raw_view(request_key),
                RawBodyView::Base64
            );
        })
        .expect("test window should remain open");
    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        assert!(visual.debug_bounds("response-body").is_some());
    }

    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        assert!(visual.debug_bounds("response-raw-view-hex").is_some());
        let hex = visual
            .debug_bounds("response-raw-view-hex")
            .expect("raw Hex sub-tab should render");
        visual.simulate_click(hex.center(), Modifiers::default());
    }
    cx.run_until_parked();
    window
        .update(cx, |view, _, _| {
            assert_eq!(view.response_viewer.raw_view(request_key), RawBodyView::Hex);
            let text = view.response_viewer.visible_text(request_key);
            assert!(text.contains("7b 22 63"));
        })
        .expect("test window should remain open");
    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        assert!(visual.debug_bounds("response-body").is_some());
    }

    window
        .update(cx, |view, _, cx| {
            view.response_viewer
                .set_tab(request_key, ResponseViewerTab::Headers);
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
            view.response_viewer
                .set_tab(request_key, ResponseViewerTab::Pretty);
            cx.notify();
        })
        .expect("test window should remain open");
    cx.run_until_parked();

    window
        .update(cx, |view, _, cx| {
            view.response_viewer
                .set_tab(request_key, ResponseViewerTab::Inspect);
            cx.notify();
        })
        .expect("test window should remain open");
    cx.run_until_parked();
    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        assert!(visual.debug_bounds("response-inspector-divider").is_some());
        let reveal = visual
            .debug_bounds("response-inspector-reveal-pretty")
            .expect("selected inspection should expose a reveal button");
        visual.simulate_mouse_down(reveal.center(), MouseButton::Left, Modifiers::default());
        visual.simulate_mouse_up(reveal.center(), MouseButton::Left, Modifiers::default());
    }
    cx.run_until_parked();
    window
        .update(cx, |view, _, _| {
            assert_eq!(
                view.response_viewer.tab(request_key),
                ResponseViewerTab::Pretty
            );
            assert_eq!(
                view.pretty_reveal.get(),
                Some(PrettyRevealState {
                    selection: InspectSelection::Timestamp(0),
                    scroll_pending: false,
                })
            );
        })
        .expect("test window should remain open");
    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let body = visual
            .debug_bounds("response-body")
            .expect("Pretty response body should render after reveal");
        assert!(
            visual
                .debug_bounds("response-inspector-reveal-pretty")
                .is_none()
        );
        visual.simulate_mouse_down(body.center(), MouseButton::Left, Modifiers::default());
        visual.simulate_mouse_up(body.center(), MouseButton::Left, Modifiers::default());
    }
    cx.run_until_parked();
    window
        .update(cx, |view, _, _| {
            assert!(view.pretty_reveal.get().is_none());
        })
        .expect("test window should remain open");
}

#[gpui::test]
fn image_response_replaces_pretty_with_scrollable_preview(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = bundled_fixture()
        .canonicalize()
        .expect("fixture should exist");
    let workspace = probe_opencollection::load_workspace(&fixture).expect("fixture should load");
    let request_key = workspace.requests()[0].key();
    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            view.set_workspace(fixture, workspace);
            view.select_request(request_key, cx);
            let (cancellation, _) = tokio::sync::oneshot::channel();
            let generation = view.execution.begin(request_key, cancellation);
            let width = 16_u32;
            let height = 1_600_u32;
            let row_bytes = (width * 3).div_ceil(4) * 4;
            let pixel_bytes = row_bytes * height;
            let file_size = 54 + pixel_bytes;
            let mut body = Vec::with_capacity(file_size as usize);
            body.extend_from_slice(b"BM");
            body.extend_from_slice(&file_size.to_le_bytes());
            body.extend_from_slice(&[0; 4]);
            body.extend_from_slice(&54_u32.to_le_bytes());
            body.extend_from_slice(&40_u32.to_le_bytes());
            body.extend_from_slice(&(width as i32).to_le_bytes());
            body.extend_from_slice(&(height as i32).to_le_bytes());
            body.extend_from_slice(&1_u16.to_le_bytes());
            body.extend_from_slice(&24_u16.to_le_bytes());
            body.extend_from_slice(&0_u32.to_le_bytes());
            body.extend_from_slice(&pixel_bytes.to_le_bytes());
            body.extend_from_slice(&[0; 16]);
            body.resize(file_size as usize, 0x7f);
            view.complete_execution(
                request_key,
                generation,
                Ok(HttpResponse {
                    status: 200,
                    reason: "OK".to_owned(),
                    url: "https://api.example.test/avatar".to_owned(),
                    duration: Duration::from_millis(12),
                    size: body.len(),
                    headers: vec![ResponseHeader {
                        name: "content-type".to_owned(),
                        value: "Application/Octet-Stream; charset=binary".to_owned(),
                    }],
                    body,
                    body_complete: true,
                    body_file: None,
                    body_retention_error: None,
                }),
                None,
                cx,
            );
            cx.notify();
        })
        .expect("test window should be open");
    cx.run_until_parked();

    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        assert!(visual.debug_bounds("response-tab-preview").is_some());
        assert!(visual.debug_bounds("response-tab-pretty").is_none());
        assert!(visual.debug_bounds("response-tab-raw").is_some());
        assert!(visual.debug_bounds("response-tab-headers").is_some());
        assert!(visual.debug_bounds("response-image-preview").is_some());
    }
    cx.run_until_parked();
    window
        .update(cx, |_, _, cx| cx.notify())
        .expect("test window should remain open");
    cx.run_until_parked();

    let (preview_bounds, image_before) = {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let preview = visual
            .debug_bounds("response-image-preview")
            .expect("image preview should render");
        let image = visual
            .debug_bounds("response-preview-image")
            .expect("preview image should render");
        assert!(
            image.size.height > preview.size.height,
            "long image should overflow preview: image={image:?}, preview={preview:?}"
        );
        (preview, image)
    };
    window
        .update(cx, |view, _, _| {
            assert!(
                view.response_viewer
                    .image_scroll(request_key)
                    .expect("image scroll handle")
                    .max_offset()
                    .y
                    > px(0.0)
            );
        })
        .expect("test window should remain open");

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.simulate_event(gpui::ScrollWheelEvent {
        position: preview_bounds.center(),
        delta: gpui::ScrollDelta::Pixels(point(px(0.0), px(-120.0))),
        modifiers: Modifiers::default(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    let image_after = visual
        .debug_bounds("response-preview-image")
        .expect("preview image should remain rendered after scrolling");
    assert!(image_after.origin.y < image_before.origin.y);
}

#[gpui::test]
fn xml_response_inspects_values_and_keeps_syntax_after_visiting_raw(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = bundled_fixture()
        .canonicalize()
        .expect("fixture should exist");
    let workspace = probe_opencollection::load_workspace(&fixture).expect("fixture should load");
    let request_key = workspace.requests()[0].key();
    window
        .update(cx, |view, _, cx| {
            view.session_store = None;
            view.set_workspace(fixture, workspace);
            view.select_request(request_key, cx);
            let (cancellation, _) = tokio::sync::oneshot::channel();
            let generation = view.execution.begin(request_key, cancellation);
            let body = br#"<root createdAt="1787482800"><item/></root>"#.to_vec();
            view.complete_execution(
                request_key,
                generation,
                Ok(HttpResponse {
                    status: 200,
                    reason: "OK".to_owned(),
                    url: "https://api.example.test/data.xml".to_owned(),
                    duration: Duration::from_millis(12),
                    size: body.len(),
                    headers: vec![ResponseHeader {
                        name: "content-type".to_owned(),
                        value: "application/xml".to_owned(),
                    }],
                    body,
                    body_complete: true,
                    body_file: None,
                    body_retention_error: None,
                }),
                None,
                cx,
            );
            cx.notify();
        })
        .expect("test window should be open");
    cx.run_until_parked();

    window
        .update(cx, |view, _, cx| {
            assert_eq!(
                view.response_viewer
                    .document(request_key)
                    .expect("response document")
                    .syntax
                    .language(),
                "xml"
            );
            let document = view
                .response_viewer
                .document(request_key)
                .expect("response document");
            assert_eq!(document.inspection.timestamps.len(), 1);
            assert_eq!(document.inspection.timestamps[0].path, "/root/@createdAt");
            assert_eq!(document.inspection_ranges.len(), 1);
            assert_eq!(
                &document.pretty_text[document.inspection_ranges[0].range.clone()],
                "1787482800"
            );
            view.response_viewer
                .set_tab(request_key, ResponseViewerTab::Raw);
            cx.notify();
        })
        .expect("test window should remain open");
    cx.run_until_parked();

    window
        .update(cx, |view, _, cx| {
            view.response_viewer
                .set_tab(request_key, ResponseViewerTab::Pretty);
            cx.notify();
        })
        .expect("test window should remain open");
    cx.run_until_parked();

    window
        .update(cx, |view, _, _| {
            assert_eq!(
                view.response_viewer.tab(request_key),
                ResponseViewerTab::Pretty
            );
            assert_eq!(
                view.response_viewer
                    .document(request_key)
                    .expect("response document")
                    .syntax
                    .language(),
                "xml"
            );
        })
        .expect("test window should remain open");
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("response-body").is_some());
}

#[gpui::test]
fn large_response_body_only_renders_visible_rows(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let window = cx.open_window(size(px(1180.0), px(780.0)), |window, cx| {
        ProbeApp::new(window, cx)
    });
    let fixture = bundled_fixture()
        .canonicalize()
        .expect("fixture should exist");
    let workspace = probe_opencollection::load_workspace(&fixture).expect("fixture should load");
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
                    body_file: None,
                    body_retention_error: None,
                }),
                None,
                cx,
            );
            view.response_viewer
                .set_tab(request_key, ResponseViewerTab::Raw);
            cx.notify();
        })
        .expect("test window should be open");
    cx.run_until_parked();
    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.update(|window, cx| {
            window.simulate_next_frame(cx);
            window.simulate_next_frame(cx);
        });
    }
    cx.run_until_parked();

    let (total_rows, rendered_rows) = window
        .update(cx, |view, _, _| {
            (
                view.response_viewer.visible_line_count(request_key),
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
