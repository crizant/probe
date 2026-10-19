use std::{fs, path::PathBuf};

use probe_core::{FieldPatch, Request, RequestProtocol, RequestProtocolError, RequestUpdate};
use probe_opencollection::load_workspace;
use serde_yaml_ng::Value;

use super::support::{TemporaryPath, fixture, temporary_path};

fn request_document(protocol: &str, runtime: Option<&str>) -> Value {
    let mut request: Value = serde_yaml_ng::from_str(&format!(
        "info: {{name: Runtime preservation, type: {protocol}}}\n{protocol}: {{url: 'https://example.com/original'}}\n"
    ))
    .unwrap();
    if let Some(runtime) = runtime {
        request["runtime"] = serde_yaml_ng::from_str(runtime).unwrap();
    }
    request
}

fn document_text(bundled: bool, request: Value) -> String {
    let document = if bundled {
        let mut root: Value = serde_yaml_ng::from_str(
            "opencollection: 1.0.0\ninfo: {name: Runtime preservation}\nbundled: true\nitems: []\n",
        )
        .unwrap();
        root["items"].as_sequence_mut().unwrap().push(request);
        root
    } else {
        request
    };
    serde_yaml_ng::to_string(&document).unwrap()
}

fn write_workspace(bundled: bool, request: Value) -> (TemporaryPath, PathBuf, &'static str) {
    let root = temporary_path(if bundled { "runtime.yml" } else { "runtime" });
    let (file, selector) = if bundled {
        (root.to_path_buf(), "items/0")
    } else {
        fs::create_dir(&root).unwrap();
        fs::copy(
            fixture("assertions-unbundled/opencollection.yml"),
            root.join("opencollection.yml"),
        )
        .unwrap();
        (root.join("request.yml"), "request.yml")
    };
    fs::write(&file, document_text(bundled, request)).unwrap();
    (root, file, selector)
}

#[test]
fn clearing_assertions_only_removes_the_assertion_field_and_its_emptied_container() {
    let assertions =
        "{assertions: [{expression: response.status, operator: equals, value: '200'}]}";
    let scripts = "{scripts: [{type: before-request, code: 'console.log(1)'}]}";
    let scripts_and_assertions = "{scripts: [{type: before-request, code: 'console.log(1)'}], assertions: [{expression: response.status, operator: equals, value: '200'}]}";
    for bundled in [true, false] {
        for protocol in ["http", "graphql"] {
            for (runtime, expected_runtime) in [
                (None, None),
                (Some("{}"), Some("{}")),
                (Some("{assertions: []}"), None),
                (Some(assertions), None),
                (Some(scripts), Some(scripts)),
                (Some(scripts_and_assertions), Some(scripts)),
            ] {
                let (root, file, selector) =
                    write_workspace(bundled, request_document(protocol, runtime));
                let expected = document_text(bundled, request_document(protocol, expected_runtime));
                let mut loaded = load_workspace(&root).unwrap();
                // Repeated Clear must not create a runtime mapping or remove a
                // pre-existing empty mapping or another runtime declaration.
                for _ in 0..2 {
                    loaded
                        .update_request(
                            selector,
                            &RequestUpdate {
                                assertions: FieldPatch::Clear,
                                ..Default::default()
                            },
                        )
                        .unwrap();
                    assert_eq!(fs::read_to_string(&file).unwrap(), expected);
                    let reloaded = load_workspace(&root).unwrap();
                    assert_eq!(
                        reloaded
                            .workspace()
                            .request(reloaded.request_key(selector).unwrap())
                            .unwrap()
                            .runtime
                            .assertions,
                        None
                    );
                }
            }
        }
    }
}

#[test]
fn websocket_assertion_set_and_clear_leave_the_domain_and_saved_yaml_unchanged() {
    for bundled in [true, false] {
        for runtime in [
            None,
            Some("{scripts: [{type: before-request, code: 'console.log(1)'}]}"),
        ] {
            let (root, file, selector) =
                write_workspace(bundled, request_document("websocket", runtime));
            let source = fs::read(&file).unwrap();
            let mut loaded = load_workspace(&root).unwrap();
            let original = loaded
                .workspace()
                .request(loaded.request_key(selector).unwrap())
                .unwrap()
                .clone();
            for assertions in [FieldPatch::Set(vec![]), FieldPatch::Clear] {
                let update = RequestUpdate {
                    assertions,
                    ..Default::default()
                };
                let mut request = original.clone();
                assert!(matches!(
                    update.apply(&mut request),
                    Err(RequestProtocolError::UnsupportedField {
                        protocol: RequestProtocol::WebSocket,
                        field: "runtime assertions"
                    })
                ));
                assert_eq!(request, original);
                assert!(loaded.update_request(selector, &update).is_err());
                assert_eq!(fs::read(&file).unwrap(), source);
                assert_eq!(
                    loaded
                        .workspace()
                        .request(loaded.request_key(selector).unwrap()),
                    Some(&original)
                );
                // Prepared saves use the YAML update path directly.
                assert!(
                    loaded
                        .prepare_request_save(selector, update)
                        .unwrap()
                        .execute()
                        .is_err()
                );
                assert_eq!(fs::read(&file).unwrap(), source);
            }
        }
    }
}

#[test]
fn reconciliation_conflict_keeps_baseline_runtime_yaml_when_saving_an_unrelated_edit() {
    let runtime = "{scripts: [{type: before-request, code: 'console.log(1)'}], assertions: [{expression: response.status, operator: equals, value: '200', disabled: false, description: null}]}";
    for bundled in [true, false] {
        for protocol in ["http", "graphql"] {
            let document = request_document(protocol, Some(runtime));
            let (root, file, selector) = write_workspace(bundled, document.clone());
            let mut loaded = load_workspace(&root).unwrap();
            let baseline = loaded
                .workspace()
                .request(loaded.request_key(selector).unwrap())
                .unwrap()
                .clone();
            let mut local = baseline.clone();
            local.runtime.assertions = None;
            local.url = Some("https://example.com/edited".into());
            let mut incoming = baseline.clone();
            incoming.runtime.assertions.as_mut().unwrap()[0].value = Some("201".into());
            let (merged, conflicts) = Request::reconcile(&baseline, &local, &incoming);
            assert_eq!(conflicts, ["runtime"]);
            assert_eq!(merged.runtime, baseline.runtime);
            let update = RequestUpdate::between(Some(&baseline), &merged).unwrap();
            assert!(update.assertions.is_unchanged());
            loaded.update_request(selector, &update).unwrap();
            let mut expected = document;
            expected[protocol]["url"] = Value::String("https://example.com/edited".into());
            assert_eq!(
                fs::read_to_string(&file).unwrap(),
                document_text(bundled, expected)
            );
            let reloaded = load_workspace(&root).unwrap();
            assert_eq!(
                reloaded
                    .workspace()
                    .request(reloaded.request_key(selector).unwrap()),
                Some(&merged)
            );
        }
    }
}

#[test]
fn malformed_non_first_assertion_reports_its_index_in_both_layouts() {
    let runtime = "{assertions: [{expression: response.status, operator: equals, value: '200'}, {expression: response.body.token, operator: isDefined}, {expression: response.status, operator: equals, value: 200}]}";
    let expected = "runtime.assertions[2]: assertion.value must be a string";
    for bundled in [true, false] {
        for protocol in ["http", "graphql"] {
            let document = request_document(protocol, Some(runtime));
            let (root, _, _) = write_workspace(bundled, document.clone());
            let error = load_workspace(&root).unwrap_err().to_string();
            assert!(error.ends_with(expected), "{error}");
            if bundled {
                let error = super::parse(&document_text(true, document))
                    .unwrap_err()
                    .to_string();
                assert!(error.ends_with(expected), "{error}");
            }
        }
    }
}

#[test]
fn bundled_domain_creation_rejects_websocket_assertions_before_writing_any_files() {
    use probe_core::{Assertion, Collection, CollectionItem, Folder, RequestKind, RequestRuntime};
    use probe_opencollection::{CreateError, create_bundled_workspace_from_collection};

    let assertions = vec![Assertion {
        expression: "response.status".into(),
        operator: "equals".into(),
        value: Some("200".into()),
        disabled: None,
        description: None,
    }];
    for assertions in [vec![], assertions] {
        for nested in [false, true] {
            let request = CollectionItem::Request(Request {
                kind: RequestKind::WebSocket { message: None },
                runtime: RequestRuntime {
                    assertions: Some(assertions.clone()),
                },
                ..Default::default()
            });
            let item = if nested {
                CollectionItem::Folder(Folder {
                    items: vec![request],
                    ..Default::default()
                })
            } else {
                request
            };
            let collection = Collection {
                items: vec![item],
                ..Default::default()
            };
            let root = temporary_path("rejected-websocket-domain");
            let output = root.join("nested/collection.yml");
            let error = create_bundled_workspace_from_collection(&output, &collection).unwrap_err();
            let expected = RequestProtocolError::UnsupportedField {
                protocol: RequestProtocol::WebSocket,
                field: "runtime assertions",
            };
            assert!(matches!(&error, CreateError::Protocol(source) if *source == expected));
            assert_eq!(
                error.to_string(),
                format!("cannot create OpenCollection workspace: {expected}")
            );
            assert_eq!(
                std::error::Error::source(&error).unwrap().to_string(),
                expected.to_string()
            );
            assert!(
                !root.exists(),
                "rejected creation must not create files or parent directories"
            );
        }
    }
}

#[test]
fn invalid_assertion_descriptions_have_indexed_field_specific_errors_only() {
    let expected = "runtime.assertions[1]: assertion.description must be a string, null, or an object containing only string fields 'content' and 'type'";
    for bundled in [true, false] {
        for protocol in ["http", "graphql"] {
            for description in [
                "42",
                "false",
                "[]",
                "{}",
                "{content: text}",
                "{type: text/plain}",
                "{content: 42, type: text/plain}",
                "{content: text, type: false}",
                "{content: text, type: text/plain, extra: text}",
            ] {
                let runtime = format!(
                    "{{assertions: [{{expression: x, operator: equals}}, {{expression: x, operator: equals, description: {description}}}]}}"
                );
                let (root, _, _) =
                    write_workspace(bundled, request_document(protocol, Some(&runtime)));
                let error = load_workspace(&root).unwrap_err().to_string();
                assert!(error.ends_with(expected), "{error}");
            }
            // The same invalid object retains the existing messages outside assertions.
            for (field, expected) in [
                (
                    "description",
                    "documentation object must contain only content and type",
                ),
                (
                    "docs",
                    "request docs must be a string; an object or null is invalid",
                ),
            ] {
                let mut request = request_document(protocol, None);
                let invalid = serde_yaml_ng::from_str("{content: text}").unwrap();
                if field == "description" {
                    request["info"][field] = invalid;
                } else {
                    request[field] = invalid;
                }
                let (root, _, _) = write_workspace(bundled, request);
                let error = load_workspace(&root).unwrap_err().to_string();
                assert!(error.ends_with(expected), "{error}");
            }
        }
    }
}
