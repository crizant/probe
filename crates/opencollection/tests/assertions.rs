use std::fs;

use probe_core::{Collection, CollectionItem, Documentation, FieldPatch, Request, RequestUpdate};
use probe_opencollection::{
    LoadedWorkspace, StructureOperation, create_bundled_workspace_from_collection, load_workspace,
    parse,
};

#[path = "assertions/runtime_persistence.rs"]
mod runtime_persistence;
mod support;
use support::{TemporaryPath, copy_directory, fixture, temporary_path};

fn fixture_workspace(bundled: bool) -> TemporaryPath {
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
    path
}

fn request<'a>(loaded: &'a LoadedWorkspace, selector: &str) -> &'a Request {
    loaded
        .workspace()
        .request(loaded.request_key(selector).unwrap())
        .unwrap()
}

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
    let environment = probe_core::resolve_environment_with_overrides(&[], None, &[]).unwrap();
    assert_eq!(
        probe_core::resolve_request_strict(graphql, &environment)
            .unwrap()
            .runtime,
        graphql.runtime
    );
    assert_eq!(
        parse(&parsed.to_yaml().unwrap()).unwrap().collection(),
        parsed.collection()
    );
}

#[test]
fn both_layouts_preserve_assertions_through_edits_creation_duplication_and_domain_copy() {
    for bundled in [true, false] {
        let path = fixture_workspace(bundled);
        let mut loaded = load_workspace(&path).unwrap();
        let selectors: Vec<_> = loaded
            .requests()
            .iter()
            .map(|r| r.selector().to_owned())
            .collect();
        let mut copied_items = Vec::new();
        for selector in &selectors {
            let before = request(&loaded, selector).clone();
            let file = if bundled {
                path.to_path_buf()
            } else {
                path.join(selector)
            };
            let source: serde_yaml_ng::Value =
                serde_yaml_ng::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
            let expected = serde_yaml_ng::to_string(&source).unwrap().replace(
                before.url.as_deref().unwrap(),
                "https://example.com/changed",
            );
            let mut edited = before.clone();
            edited.url = Some("https://example.com/changed".into());
            let update = RequestUpdate::between(Some(&before), &edited).unwrap();
            // Exercise immediate and prepared saves, and compare the complete saved YAML.
            if before.kind.is_graphql() {
                let key = loaded.request_key(selector).unwrap();
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
            assert_eq!(fs::read_to_string(&file).unwrap(), expected);
            assert_eq!(request(&load_workspace(&path).unwrap(), selector), &edited);
            copied_items.push(CollectionItem::Request(edited.clone()));

            // Creation assigns a new sequence instead of accepting the source sequence.
            let mut copy = edited.clone();
            copy.metadata.sequence = None;
            let created = loaded
                .apply_structure(StructureOperation::CreateRequest {
                    parent: None,
                    index: None,
                    name: format!("Copied {}", edited.kind.as_str()),
                    method: edited.method.clone(),
                    url: edited.url.clone(),
                    protocol: edited.kind.protocol(),
                    graphql: None,
                    update: Some(RequestUpdate::between(None, &copy).unwrap()),
                })
                .unwrap()
                .selector
                .unwrap();
            let duplicate = loaded
                .apply_structure(StructureOperation::DuplicateRequest {
                    selector: created.clone(),
                })
                .unwrap()
                .selector
                .unwrap();
            let reloaded = load_workspace(&path).unwrap();
            for selector in [&created, &duplicate] {
                assert_eq!(request(&reloaded, selector).runtime, edited.runtime);
            }

            // Structural writes may assign sibling sequence numbers.
            edited = request(&reloaded, selector).clone();
            // Replacement preserves literal operators/empty RHS; [] and absence stay distinct.
            let before = edited.clone();
            edited.runtime.assertions.as_mut().unwrap()[0].operator =
                " operator unchanged by Probe ".into();
            edited.runtime.assertions.as_mut().unwrap()[0].value = Some(String::new());
            let update = RequestUpdate::between(Some(&before), &edited).unwrap();
            assert!(!update.is_empty());
            for (assertions, expected) in [
                (update.assertions, edited.runtime.assertions.clone()),
                (FieldPatch::Set(vec![]), Some(vec![])),
                (FieldPatch::Clear, None),
            ] {
                edited.runtime.assertions = expected;
                loaded
                    .update_request(
                        selector,
                        &RequestUpdate {
                            assertions,
                            ..Default::default()
                        },
                    )
                    .unwrap();
                assert_eq!(request(&load_workspace(&path).unwrap(), selector), &edited);
                copied_items.push(CollectionItem::Request(edited.clone()));
            }
        }
        // Empty schema strings are valid and do not imply optional fields.
        let empty = probe_core::Assertion {
            expression: String::new(),
            operator: String::new(),
            value: None,
            disabled: None,
            description: None,
        };
        let mut empty_request = Request::default();
        empty_request.runtime.assertions = Some(vec![empty]);
        copied_items.push(CollectionItem::Request(empty_request));
        // A fresh domain copy cannot rely on retained source YAML.
        let collection = Collection {
            items: copied_items,
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
        let http_file = if bundled {
            path.to_path_buf()
        } else {
            path.join("http.yml")
        };
        assert!(
            fs::read_to_string(http_file)
                .unwrap()
                .contains("preserve scripts")
        );
    }
}
