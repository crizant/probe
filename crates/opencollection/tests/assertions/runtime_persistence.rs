use std::{fs, path::PathBuf};

use probe_core::{FieldPatch, Request, RequestProtocol, RequestProtocolError, RequestUpdate};
use probe_opencollection::{SaveError, load_workspace};
use serde_yaml_ng::Value;

use super::{
    request,
    support::{TemporaryPath, fixture, temporary_path},
};

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
            "opencollection: 1.0.0\ninfo: {name: Runtime preservation}\nbundled: true\n",
        )
        .unwrap();
        root["items"] = Value::Sequence(vec![request]);
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
                let context = format!("{protocol} bundled={bundled} runtime={runtime:?}");
                let (root, file, selector) =
                    write_workspace(bundled, request_document(protocol, runtime));
                let expected = document_text(bundled, request_document(protocol, expected_runtime));
                let mut loaded = load_workspace(&root).unwrap();
                let clear = RequestUpdate {
                    assertions: FieldPatch::Clear,
                    ..Default::default()
                };
                // Repeated Clear preserves absent/empty runtimes and other runtime fields.
                for _ in 0..2 {
                    loaded.update_request(selector, &clear).unwrap();
                    assert_eq!(fs::read_to_string(&file).unwrap(), expected, "{context}");
                    let reloaded = load_workspace(&root).unwrap();
                    assert_eq!(
                        request(&reloaded, selector).runtime.assertions,
                        None,
                        "{context}"
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
            let original = request(&loaded, selector).clone();
            for assertions in [FieldPatch::Set(vec![]), FieldPatch::Clear] {
                let context = format!(
                    "websocket bundled={bundled} runtime={runtime:?} assertions={assertions:?}"
                );
                let update = RequestUpdate {
                    assertions,
                    ..Default::default()
                };
                let mut edited = original.clone();
                assert!(
                    matches!(
                        update.apply(&mut edited),
                        Err(RequestProtocolError::UnsupportedField {
                            protocol: RequestProtocol::WebSocket,
                            field: "runtime assertions"
                        })
                    ),
                    "{context}"
                );
                assert_eq!(edited, original, "{context}");
                let error = loaded.update_request(selector, &update).unwrap_err();
                assert!(
                    matches!(
                        &error,
                        SaveError::Protocol(RequestProtocolError::UnsupportedField {
                            protocol: RequestProtocol::WebSocket,
                            field: "runtime assertions"
                        })
                    ),
                    "{context}: {error}"
                );
                assert_eq!(fs::read(&file).unwrap(), source, "{context}");
                assert_eq!(request(&loaded, selector), &original, "{context}");
                // Prepared saves use the YAML update path directly.
                let error = loaded
                    .prepare_request_save(selector, update.clone())
                    .unwrap()
                    .execute()
                    .unwrap_err();
                assert!(
                    matches!(
                        &error,
                        SaveError::Protocol(RequestProtocolError::UnsupportedField {
                            protocol: RequestProtocol::WebSocket,
                            field: "runtime assertions"
                        })
                    ),
                    "{context}: {error}"
                );
                assert_eq!(fs::read(&file).unwrap(), source, "{context}");
                let error = loaded
                    .apply_structure(super::StructureOperation::CreateRequest {
                        parent: None,
                        index: None,
                        name: "WebSocket".into(),
                        method: None,
                        url: Some("wss://example.com".into()),
                        protocol: RequestProtocol::WebSocket,
                        graphql: None,
                        update: Some(update),
                    })
                    .unwrap_err();
                assert!(
                    matches!(&error, probe_opencollection::StructureError::InvalidDocument(message)
                        if message == "native WebSocket requests do not support runtime assertions"),
                    "{context}: {error}"
                );
                assert_eq!(fs::read(&file).unwrap(), source, "{context}");
            }
        }
    }
}

#[test]
fn reconciliation_conflict_keeps_baseline_runtime_yaml_when_saving_an_unrelated_edit() {
    let runtime = "{scripts: [{type: before-request, code: 'console.log(1)'}], assertions: [{expression: response.status, operator: equals, value: '200', disabled: false, description: null}]}";
    for bundled in [true, false] {
        for protocol in ["http", "graphql"] {
            let context = format!("{protocol} bundled={bundled} runtime={runtime:?}");
            let document = request_document(protocol, Some(runtime));
            let (root, file, selector) = write_workspace(bundled, document.clone());
            let mut loaded = load_workspace(&root).unwrap();
            let baseline = request(&loaded, selector).clone();
            let mut local = baseline.clone();
            local.runtime.assertions = None;
            local.url = Some("https://example.com/edited".into());
            let mut incoming = baseline.clone();
            incoming.runtime.assertions.as_mut().unwrap()[0].value = Some("201".into());
            let (merged, conflicts) = Request::reconcile(&baseline, &local, &incoming);
            assert_eq!(conflicts, ["runtime"], "{context}");
            assert_eq!(merged.runtime, baseline.runtime, "{context}");
            let update = RequestUpdate::between(Some(&baseline), &merged).unwrap();
            assert!(update.assertions.is_unchanged(), "{context}");
            loaded.update_request(selector, &update).unwrap();
            let mut expected = document;
            expected[protocol]["url"] = Value::String("https://example.com/edited".into());
            assert_eq!(
                fs::read_to_string(&file).unwrap(),
                document_text(bundled, expected),
                "{context}"
            );
            let reloaded = load_workspace(&root).unwrap();
            assert_eq!(request(&reloaded, selector), &merged, "{context}");
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
            let context = format!("websocket nested={nested} assertions={assertions:?}");
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
            assert!(
                matches!(&error, CreateError::Protocol(source) if *source == expected),
                "{context}: {error}"
            );
            assert_eq!(
                error.to_string(),
                format!("cannot create OpenCollection workspace: {expected}"),
                "{context}"
            );
            assert_eq!(
                std::error::Error::source(&error).unwrap().to_string(),
                expected.to_string(),
                "{context}"
            );
            assert!(
                !root.exists(),
                "{context}: rejected creation must not create files or parent directories"
            );
        }
    }
}

#[test]
fn malformed_runtime_fields_fail_normal_parsing_loading_and_validation() {
    // One table drives every normal entry point in both layouts and protocols.
    let mut cases = Vec::new();
    for runtime in ["null", "[]", "text"] {
        cases.push((
            format!("runtime: {runtime}"),
            "runtime must be an object".to_owned(),
        ));
    }
    for assertions in ["null", "{}", "text"] {
        cases.push((
            format!("runtime: {{assertions: {assertions}}}"),
            "runtime.assertions must be an array".to_owned(),
        ));
    }
    for entry in [
        "null",
        "[]",
        "{}",
        "{expression: x}",
        "{operator: equals}",
        "{expression: 200, operator: equals}",
        "{expression: x, operator: false}",
        "{expression: x, operator: equals, value: 200}",
        "{expression: x, operator: equals, value: null}",
        "{expression: x, operator: equals, disabled: 'false'}",
        "{expression: x, operator: equals, disabled: null}",
        "{expression: x, operator: equals, unexpected: x}",
    ] {
        // Every error must identify the malformed third entry.
        cases.push((format!("runtime: {{assertions: [{{expression: x, operator: equals}}, {{expression: y, operator: isDefined}}, {entry}]}}"), "runtime.assertions[2]: ".to_owned()));
    }
    cases.push(("runtime: {assertions: [{expression: x, operator: equals}, {expression: y, operator: isDefined}, {expression: response.status, operator: equals, value: 200}]}".into(), "runtime.assertions[2]: assertion.value must be a string".into()));
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
        cases.push((format!("runtime: {{assertions: [{{expression: x, operator: equals}}, {{expression: x, operator: equals, description: {description}}}]}}"), "runtime.assertions[1]: assertion.description must be a string, null, or an object containing only string fields 'content' and 'type'".to_owned()));
    }
    // Shared documentation diagnostics must remain unchanged outside assertions.
    cases.extend([
        (
            "info: {description: {content: text}}".into(),
            "documentation object must contain only content and type".into(),
        ),
        (
            "docs: {content: text}".into(),
            "request docs must be a string; an object or null is invalid".into(),
        ),
    ]);
    for bundled in [true, false] {
        for protocol in ["http", "graphql"] {
            for (input, expected) in &cases {
                let mut document = request_document(protocol, None);
                let fields: Value = serde_yaml_ng::from_str(input).unwrap();
                for (field, value) in fields.as_mapping().unwrap() {
                    if field == "info" {
                        document["info"]["description"] = value["description"].clone();
                    } else {
                        document[field.clone()] = value.clone();
                    }
                }
                let (root, _, _) = write_workspace(bundled, document.clone());
                let mut errors = vec![load_workspace(&root).unwrap_err().to_string()];
                if bundled {
                    let source = document_text(true, document);
                    errors.push(super::parse(&source).unwrap_err().to_string());
                    errors.push(
                        probe_opencollection::load_workspace_from_str(&source)
                            .unwrap_err()
                            .to_string(),
                    );
                }
                for error in errors {
                    assert!(
                        error.contains(expected),
                        "{protocol} bundled={bundled} input={input:?}: {error}"
                    );
                }
            }
        }
    }
}
