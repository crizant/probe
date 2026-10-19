use std::fs;

use probe_core::{Collection, CollectionItem, Documentation, FieldPatch, RequestUpdate};
use probe_opencollection::{
    create_bundled_workspace_from_collection, load_workspace, load_workspace_from_str, parse,
};

mod support;
use support::{copy_directory, fixture, temporary_path};

#[test]
fn projects_http_and_graphql_assertions_without_interpreting_fields() {
    let parsed = parse(&fs::read_to_string(fixture("assertions-bundled.yml")).unwrap()).unwrap();
    let CollectionItem::Request(http) = &parsed.collection().items[0] else {
        panic!()
    };
    let assertions = http.runtime.assertions.as_ref().unwrap();
    assert_eq!(assertions.len(), 4);
    assert_eq!(assertions[0].expression, "response.status");
    assert_eq!(assertions[0].operator, "equals");
    assert_eq!(assertions[0].value.as_deref(), Some("200"));
    assert_eq!(assertions[0].disabled, Some(false));
    assert_eq!(
        assertions[0].description,
        Some(Documentation::Text("Response status should be 200".into()))
    );
    assert_eq!(assertions[1].expression, "response.body.users.length");
    assert_eq!(assertions[1].operator, "greaterThan");
    assert_eq!(assertions[1].value.as_deref(), Some("0"));
    assert_eq!(assertions[2].expression, " response.body.name ");
    assert_eq!(assertions[2].value.as_deref(), Some(""));
    assert_eq!(assertions[2].disabled, Some(true));
    assert_eq!(
        assertions[2].description,
        Some(Documentation::Content {
            content: "**An empty name**".into(),
            media_type: "text/markdown".into()
        })
    );
    assert_eq!(assertions[3].value, None);
    assert_eq!(assertions[3].disabled, None);
    assert_eq!(assertions[3].description, Some(Documentation::Null));
    let CollectionItem::Request(graphql) = &parsed.collection().items[1] else {
        panic!()
    };
    let assertion = &graphql.runtime.assertions.as_ref().unwrap()[0];
    assert_eq!(assertion.value.as_deref(), Some("{{minimumUsers}}"));
    assert_eq!(assertion.disabled, Some(true));
    assert_eq!(
        parse(&parsed.to_yaml().unwrap()).unwrap().collection(),
        parsed.collection()
    );
}

#[test]
fn bundled_and_unbundled_unrelated_edits_and_domain_copy_preserve_assertions() {
    for bundled in [true, false] {
        let path = temporary_path(if bundled {
            "assertions.yml"
        } else {
            "assertions"
        });
        if bundled {
            fs::copy(fixture("assertions-bundled.yml"), &path).unwrap();
        } else {
            copy_directory(&fixture("assertions-unbundled"), &path);
        }
        let mut loaded = load_workspace(&path).unwrap();
        let selectors: Vec<_> = loaded
            .requests()
            .iter()
            .map(|request| request.selector().to_owned())
            .collect();
        for selector in &selectors {
            let key = loaded.request_key(selector).unwrap();
            let before = loaded.workspace().request(key).unwrap().clone();
            let mut edited = before.clone();
            edited.url = Some("https://example.com/changed".into());
            let update = RequestUpdate::between(Some(&before), &edited).unwrap();
            if before.kind.is_graphql() {
                *loaded.request_mut(key).unwrap() = edited.clone();
                let saved = loaded
                    .prepare_request_save(selector, update)
                    .unwrap()
                    .execute()
                    .unwrap();
                loaded.complete_request_save(saved).unwrap();
            } else {
                loaded.update_request(selector, &update).unwrap();
            }
            let reloaded = load_workspace(&path).unwrap();
            let key = reloaded.request_key(selector).unwrap();
            assert_eq!(reloaded.workspace().request(key), Some(&edited));
        }
        // Copy through the domain into a fresh file, without retained source YAML.
        let collection = Collection {
            items: loaded
                .requests()
                .iter()
                .map(|request| {
                    CollectionItem::Request(
                        loaded.workspace().request(request.key()).unwrap().clone(),
                    )
                })
                .collect(),
            ..Default::default()
        };
        let copy = temporary_path("assertions-copy.yml");
        create_bundled_workspace_from_collection(&copy, &collection).unwrap();
        assert_eq!(
            parse(&fs::read_to_string(&copy).unwrap())
                .unwrap()
                .collection(),
            &collection
        );
        let saved = if bundled {
            fs::read_to_string(&path).unwrap()
        } else {
            fs::read_to_string(path.join("http.yml")).unwrap()
        };
        assert!(saved.contains("preserve scripts"));
    }
}

#[test]
fn assertion_updates_replace_clear_and_preserve_empty_lists_in_both_layouts() {
    for bundled in [true, false] {
        let path = temporary_path(if bundled { "updates.yml" } else { "updates" });
        if bundled {
            fs::copy(fixture("assertions-bundled.yml"), &path).unwrap();
        } else {
            copy_directory(&fixture("assertions-unbundled"), &path);
        }
        let mut loaded = load_workspace(&path).unwrap();
        for selector in if bundled {
            ["items/0", "items/1"]
        } else {
            ["http.yml", "graphql.yml"]
        } {
            let key = loaded.request_key(selector).unwrap();
            let before = loaded.workspace().request(key).unwrap().clone();
            let mut edited = before.clone();
            edited.runtime.assertions.as_mut().unwrap()[0].operator =
                " operator unchanged by Probe ".into();
            edited.runtime.assertions.as_mut().unwrap()[0].value = Some(String::new());
            let update = RequestUpdate::between(Some(&before), &edited).unwrap();
            assert!(!update.is_empty());
            loaded.update_request(selector, &update).unwrap();
            let reloaded = load_workspace(&path).unwrap();
            assert_eq!(
                reloaded
                    .workspace()
                    .request(reloaded.request_key(selector).unwrap()),
                Some(&edited)
            );
            for (patch, expected) in [
                (FieldPatch::Set(vec![]), Some(vec![])),
                (FieldPatch::Clear, None),
            ] {
                loaded
                    .update_request(
                        selector,
                        &RequestUpdate {
                            assertions: patch,
                            ..Default::default()
                        },
                    )
                    .unwrap();
                let reloaded = load_workspace(&path).unwrap();
                assert_eq!(
                    reloaded
                        .workspace()
                        .request(reloaded.request_key(selector).unwrap())
                        .unwrap()
                        .runtime
                        .assertions,
                    expected
                );
            }
        }
    }
}

#[test]
fn malformed_assertions_fail_normal_loading_and_validation() {
    let invalid = [
        "null",
        "{}",
        "text",
        "[null]",
        "[[]]",
        "[{}]",
        "[{expression: response.status}]",
        "[{operator: equals}]",
        "[{expression: 200, operator: equals}]",
        "[{expression: x, operator: false}]",
        "[{expression: x, operator: equals, value: 200}]",
        "[{expression: x, operator: equals, value: null}]",
        "[{expression: x, operator: equals, disabled: 'false'}]",
        "[{expression: x, operator: equals, disabled: null}]",
        "[{expression: x, operator: equals, description: 123}]",
        "[{expression: x, operator: equals, description: {content: x}}]",
        "[{expression: x, operator: equals, description: {content: x, type: text/plain, extra: x}}]",
        "[{expression: x, operator: equals, unexpected: x}]",
    ];
    for protocol in ["http", "graphql"] {
        for assertions in invalid {
            let source = format!(
                "opencollection: 1.0.0\ninfo: {{name: Invalid}}\nbundled: true\nitems:\n  - info: {{type: {protocol}}}\n    runtime:\n      assertions: {assertions}\n"
            );
            assert!(parse(&source).is_err(), "{source}");
            assert!(load_workspace_from_str(&source).is_err(), "{source}");
            let path = temporary_path("invalid-assertions");
            fs::create_dir(&path).unwrap();
            fs::write(
                path.join("opencollection.yml"),
                "opencollection: 1.0.0\ninfo: {name: Invalid}\nbundled: false\n",
            )
            .unwrap();
            fs::write(
                path.join("request.yml"),
                format!("info: {{type: {protocol}}}\nruntime:\n  assertions: {assertions}\n"),
            )
            .unwrap();
            assert!(load_workspace(&path).is_err(), "{assertions}");
        }
    }
}

#[test]
fn accepts_schema_strings_without_normalization_and_distinguishes_list_presence() {
    for runtime in [
        "",
        "runtime: {}",
        "runtime: {assertions: []}",
        "runtime: {assertions: [{expression: '', operator: ''}]}",
    ] {
        let source = format!(
            "opencollection: 1.0.0\ninfo: {{name: Presence}}\nbundled: true\nitems:\n  - info: {{type: http}}\n    {runtime}\n"
        );
        let parsed = parse(&source).unwrap();
        let CollectionItem::Request(request) = &parsed.collection().items[0] else {
            panic!()
        };
        if runtime.contains("expression") {
            let assertion = &request.runtime.assertions.as_ref().unwrap()[0];
            assert_eq!(assertion.expression, "");
            assert_eq!(assertion.operator, "");
            assert_eq!(assertion.value, None);
            assert_eq!(assertion.disabled, None);
            assert_eq!(assertion.description, None);
        } else {
            assert_eq!(
                request.runtime.assertions,
                runtime.contains("assertions").then(Vec::new)
            );
        }
        let copy = temporary_path("presence.yml");
        create_bundled_workspace_from_collection(&copy, parsed.collection()).unwrap();
        assert_eq!(
            parse(&fs::read_to_string(&copy).unwrap())
                .unwrap()
                .collection(),
            parsed.collection()
        );
    }
    for runtime in ["null", "[]", "text"] {
        let source = format!(
            "opencollection: 1.0.0\ninfo: {{name: Invalid}}\nbundled: true\nitems:\n  - info: {{type: http}}\n    runtime: {runtime}\n"
        );
        assert!(load_workspace_from_str(&source).is_err());
    }
}

#[test]
fn assertion_values_are_not_interpolated_during_request_resolution() {
    let parsed = parse(&fs::read_to_string(fixture("assertions-bundled.yml")).unwrap()).unwrap();
    let CollectionItem::Request(request) = &parsed.collection().items[1] else {
        panic!()
    };
    let environment = probe_core::resolve_environment_with_overrides(&[], None, &[]).unwrap();
    let resolved = probe_core::resolve_request_strict(request, &environment).unwrap();
    assert_eq!(resolved.runtime, request.runtime);
}

#[test]
fn websocket_assertion_updates_are_rejected_without_mutating_the_request() {
    let mut request = probe_core::Request {
        kind: probe_core::RequestKind::WebSocket { message: None },
        ..Default::default()
    };
    let original = request.clone();
    let update = RequestUpdate {
        assertions: FieldPatch::Set(vec![]),
        ..Default::default()
    };
    assert!(matches!(
        update.apply(&mut request),
        Err(probe_core::RequestProtocolError::UnsupportedField {
            protocol: probe_core::RequestProtocol::WebSocket,
            ..
        })
    ));
    assert_eq!(request, original);

    // Structural creation writes through the YAML update path before projection.
    let path = temporary_path("websocket-assertions.yml");
    fs::copy(fixture("assertions-bundled.yml"), &path).unwrap();
    let source = fs::read(&path).unwrap();
    let mut loaded = load_workspace(&path).unwrap();
    assert!(
        loaded
            .apply_structure(probe_opencollection::StructureOperation::CreateRequest {
                parent: None,
                index: None,
                name: "WebSocket".into(),
                method: None,
                url: Some("wss://example.com".into()),
                protocol: probe_core::RequestProtocol::WebSocket,
                graphql: None,
                update: Some(update),
            })
            .is_err()
    );
    assert_eq!(fs::read(&path).unwrap(), source);
}

#[test]
fn request_creation_and_duplication_preserve_assertions_in_both_layouts() {
    for bundled in [true, false] {
        let path = temporary_path(if bundled { "create.yml" } else { "create" });
        if bundled {
            fs::copy(fixture("assertions-bundled.yml"), &path).unwrap();
        } else {
            copy_directory(&fixture("assertions-unbundled"), &path);
        }
        let mut loaded = load_workspace(&path).unwrap();
        for selector in if bundled {
            ["items/0", "items/1"]
        } else {
            ["http.yml", "graphql.yml"]
        } {
            let mut original = loaded
                .workspace()
                .request(loaded.request_key(selector).unwrap())
                .unwrap()
                .clone();
            // Creation assigns a fresh sibling sequence.
            original.metadata.sequence = None;
            let update = RequestUpdate::between(None, &original).unwrap();
            let result = loaded
                .apply_structure(probe_opencollection::StructureOperation::CreateRequest {
                    parent: None,
                    index: None,
                    name: format!("Copied {}", original.kind.as_str()),
                    method: original.method.clone(),
                    url: original.url.clone(),
                    protocol: original.kind.protocol(),
                    graphql: None,
                    update: Some(update),
                })
                .unwrap();
            let created = result.selector.unwrap();
            let reloaded = load_workspace(&path).unwrap();
            let created_request = reloaded
                .workspace()
                .request(reloaded.request_key(&created).unwrap())
                .unwrap();
            assert_eq!(created_request.runtime, original.runtime);
            let result = loaded
                .apply_structure(probe_opencollection::StructureOperation::DuplicateRequest {
                    selector: created,
                })
                .unwrap();
            let reloaded = load_workspace(&path).unwrap();
            let duplicated = result.selector.unwrap();
            assert_eq!(
                reloaded
                    .workspace()
                    .request(reloaded.request_key(&duplicated).unwrap())
                    .unwrap()
                    .runtime,
                original.runtime
            );
        }
    }
}
