use probe_core::{
    Body, BodyVariant, Documentation, FieldPatch, GraphqlBody, GraphqlBodyVariant,
    GraphqlOperation, ItemMetadata, RawBody, RawBodyKind, Request, RequestBody, RequestDiffError,
    RequestKind, RequestProtocolError, RequestUpdate,
};

#[test]
fn diff_round_trips_optional_field_clears() {
    let base = Request {
        method: Some("POST".into()),
        url: Some("https://example.test/graphql".into()),
        kind: RequestKind::Graphql {
            body: Some(GraphqlBody::Single(GraphqlOperation {
                query: Some("query { viewer }".into()),
                ..GraphqlOperation::default()
            })),
        },
        ..Request::default()
    };
    assert!(
        RequestUpdate::between(Some(&base), &base)
            .unwrap()
            .is_empty()
    );
    let mut current = base.clone();
    current.method = None;
    current.url = None;
    if let RequestKind::Graphql {
        body: Some(GraphqlBody::Single(operation)),
    } = &mut current.kind
    {
        operation.query = None;
    } else {
        unreachable!();
    }

    let update = RequestUpdate::between(Some(&base), &current).unwrap();
    assert_eq!(update.method, FieldPatch::Clear);
    assert_eq!(update.url, FieldPatch::Clear);
    assert_eq!(update.graphql.as_ref().unwrap().query, FieldPatch::Clear);
    let mut restored = base;
    update.apply(&mut restored).unwrap();
    assert_eq!(restored, current);
}

#[test]
fn diff_keeps_description_objects_and_request_docs_until_they_change() {
    let base = Request {
        metadata: ItemMetadata {
            description: Some(Documentation::Content {
                content: "Create a pet".into(),
                media_type: "text/markdown".into(),
            }),
            ..ItemMetadata::default()
        },
        docs: Some("See the guide".into()),
        ..Request::default()
    };

    let unchanged = RequestUpdate::between(Some(&base), &base).unwrap();
    assert!(unchanged.is_empty());
    assert!(unchanged.description.is_unchanged());
    assert!(unchanged.docs.is_unchanged());

    let mut only_description = base.clone();
    only_description.metadata.description = Some(Documentation::Text("plain".into()));
    assert!(
        !RequestUpdate::between(Some(&base), &only_description)
            .unwrap()
            .is_empty()
    );
    let mut only_docs = base.clone();
    only_docs.docs = Some("other".into());
    assert!(
        !RequestUpdate::between(Some(&base), &only_docs)
            .unwrap()
            .is_empty()
    );

    let mut current = base.clone();
    current.metadata.description = Some(Documentation::Text("plain".into()));
    current.docs = Some("Updated guide".into());
    let update = RequestUpdate::between(Some(&base), &current).unwrap();
    assert_eq!(
        update.description,
        FieldPatch::Set(Documentation::Text("plain".into()))
    );
    assert_eq!(update.docs, FieldPatch::Set("Updated guide".into()));
    let mut restored = base;
    update.apply(&mut restored).unwrap();
    assert_eq!(restored, current);
}

#[test]
fn diff_rejects_changes_without_persistence_support() {
    let base = Request::default();
    let cases = [
        (
            "request protocol",
            Request {
                kind: RequestKind::Graphql { body: None },
                ..base.clone()
            },
        ),
        (
            "request settings",
            Request {
                settings: probe_core::RequestSettings {
                    follow_redirects: Some(false),
                    ..Default::default()
                },
                ..base.clone()
            },
        ),
        (
            "request sequence",
            Request {
                metadata: probe_core::ItemMetadata {
                    sequence: Some(2.0),
                    ..Default::default()
                },
                ..base.clone()
            },
        ),
    ];
    for (field, current) in cases {
        assert_eq!(
            RequestUpdate::between(Some(&base), &current),
            Err(RequestDiffError::UnsupportedChange(field))
        );
    }
    let mut named = base.clone();
    named.metadata.name = Some("name".into());
    assert_eq!(
        RequestUpdate::between(Some(&named), &base),
        Err(RequestDiffError::UnsupportedChange("request name removal"))
    );
}

#[test]
fn diff_rejects_graphql_variant_metadata_changes() {
    let base = Request {
        kind: RequestKind::Graphql {
            body: Some(GraphqlBody::Variants(vec![GraphqlBodyVariant {
                title: "first".into(),
                selected: true,
                body: GraphqlOperation {
                    query: Some("query { viewer }".into()),
                    ..Default::default()
                },
            }])),
        },
        ..Default::default()
    };
    let mut current = base.clone();
    if let RequestKind::Graphql {
        body: Some(GraphqlBody::Variants(variants)),
    } = &mut current.kind
    {
        variants[0].title = "renamed".into();
    }
    assert_eq!(
        RequestUpdate::between(Some(&base), &current),
        Err(RequestDiffError::UnsupportedChange("GraphQL body variants"))
    );
    if let RequestKind::Graphql {
        body: Some(GraphqlBody::Variants(variants)),
    } = &mut current.kind
    {
        variants[0].selected = false;
    }
    assert!(matches!(
        RequestUpdate::between(Some(&base), &current),
        Err(RequestDiffError::Protocol(
            probe_core::RequestProtocolError::InvalidBodySelection(_)
        ))
    ));
    assert!(matches!(
        RequestUpdate::between(None, &current),
        Err(RequestDiffError::Protocol(
            probe_core::RequestProtocolError::InvalidBodySelection(_)
        ))
    ));
}

fn text_body(data: &str) -> Body {
    Body::Raw(RawBody {
        kind: RawBodyKind::Text,
        data: data.to_owned(),
    })
}

#[test]
fn body_content_updates_only_the_selected_http_variant() {
    let variants = || {
        RequestBody::Variants(vec![
            BodyVariant {
                title: "JSON".to_owned(),
                selected: true,
                body: text_body("old"),
            },
            BodyVariant {
                title: "Text".to_owned(),
                selected: false,
                body: text_body("kept"),
            },
        ])
    };
    let content_update = RequestUpdate {
        body_content: FieldPatch::Set(text_body("replaced")),
        ..RequestUpdate::default()
    };
    assert!(RequestUpdate::default().is_empty());
    assert!(!content_update.is_empty());
    assert!(
        !RequestUpdate {
            body: FieldPatch::Clear,
            ..RequestUpdate::default()
        }
        .is_empty()
    );
    let mut request = Request {
        kind: RequestKind::Http {
            body: Some(variants()),
        },
        ..Request::default()
    };
    content_update.apply(&mut request).unwrap();
    let RequestBody::Variants(variants) = request.http_body().unwrap() else {
        panic!("variant list should remain");
    };
    assert_eq!(variants[0].title, "JSON");
    assert!(variants[0].selected);
    assert_eq!(variants[0].body, text_body("replaced"));
    assert_eq!(variants[1].title, "Text");
    assert!(!variants[1].selected);
    assert_eq!(variants[1].body, text_body("kept"));

    let mut unselected = Request {
        kind: RequestKind::Http {
            body: Some(variants_without_selection()),
        },
        method: Some("POST".to_owned()),
        ..Request::default()
    };
    let original = unselected.clone();
    let error = RequestUpdate {
        name: Some("renamed".to_owned()),
        body_content: FieldPatch::Set(text_body("nope")),
        ..RequestUpdate::default()
    }
    .apply(&mut unselected)
    .unwrap_err();
    assert_eq!(
        error,
        RequestProtocolError::InvalidBodySelection(
            "request body variants have no selected value".to_owned()
        )
    );
    assert_eq!(unselected, original);

    let mut ambiguous = Request {
        kind: RequestKind::Http {
            body: Some(RequestBody::Variants(vec![
                BodyVariant {
                    title: "One".to_owned(),
                    selected: true,
                    body: text_body("a"),
                },
                BodyVariant {
                    title: "Two".to_owned(),
                    selected: true,
                    body: text_body("b"),
                },
            ])),
        },
        ..Request::default()
    };
    assert_eq!(
        RequestUpdate {
            body_content: FieldPatch::Set(text_body("nope")),
            ..RequestUpdate::default()
        }
        .apply(&mut ambiguous)
        .unwrap_err(),
        RequestProtocolError::InvalidBodySelection(
            "request body variants have multiple selected values".to_owned()
        )
    );

    let mut graphql = Request {
        kind: RequestKind::Graphql { body: None },
        ..Request::default()
    };
    assert_eq!(
        RequestUpdate {
            body_content: FieldPatch::Set(text_body("nope")),
            ..RequestUpdate::default()
        }
        .apply(&mut graphql),
        Err(RequestProtocolError::NotHttp)
    );
}

#[test]
fn failed_http_body_apply_leaves_the_request_unchanged() {
    let mut request = Request {
        metadata: ItemMetadata {
            name: Some("Original".to_owned()),
            description: Some(Documentation::Text("keep description".to_owned())),
            ..ItemMetadata::default()
        },
        docs: Some("keep docs".to_owned()),
        method: Some("POST".to_owned()),
        url: Some("https://example.test/pets".to_owned()),
        kind: RequestKind::Http {
            body: Some(RequestBody::Single(text_body("original"))),
        },
        ..Request::default()
    };
    let original = request.clone();
    let cases = [
        (
            RequestBody::Variants(vec![
                BodyVariant {
                    title: "Only".to_owned(),
                    selected: false,
                    body: text_body("kept"),
                },
                BodyVariant {
                    title: "Other".to_owned(),
                    selected: false,
                    body: text_body("also"),
                },
            ]),
            "request body variants have no selected value",
        ),
        (
            RequestBody::Variants(vec![
                BodyVariant {
                    title: "One".to_owned(),
                    selected: true,
                    body: text_body("a"),
                },
                BodyVariant {
                    title: "Two".to_owned(),
                    selected: true,
                    body: text_body("b"),
                },
            ]),
            "request body variants have multiple selected values",
        ),
    ];
    for (variants, message) in cases {
        let error = RequestUpdate {
            name: Some("Renamed".to_owned()),
            description: FieldPatch::Set(Documentation::Text("changed description".to_owned())),
            docs: FieldPatch::Set("changed docs".to_owned()),
            method: FieldPatch::Set("PUT".to_owned()),
            url: FieldPatch::Set("https://changed.example".to_owned()),
            body: FieldPatch::Set(variants),
            body_content: FieldPatch::Set(text_body("nope")),
            ..RequestUpdate::default()
        }
        .apply(&mut request)
        .unwrap_err();
        assert_eq!(
            error,
            RequestProtocolError::InvalidBodySelection(message.to_owned())
        );
        assert_eq!(request, original);
    }
}

fn variants_without_selection() -> RequestBody {
    RequestBody::Variants(vec![BodyVariant {
        title: "Only".to_owned(),
        selected: false,
        body: text_body("kept"),
    }])
}

#[test]
fn reconciliation_covers_every_request_field_and_preserves_conflict_order() {
    fn changed(value: &str, sequence: f64) -> Request {
        Request {
            metadata: ItemMetadata {
                name: Some(value.into()),
                sequence: Some(sequence),
                description: Some(Documentation::Text(value.into())),
            },
            docs: Some(value.into()),
            method: Some(value.into()),
            url: Some(value.into()),
            headers: vec![probe_core::Header {
                name: "header".into(),
                value: value.into(),
                disabled: false,
            }],
            query_parameters: vec![probe_core::QueryParameter {
                name: "query".into(),
                value: value.into(),
                disabled: false,
            }],
            path_parameters: vec![probe_core::QueryParameter {
                name: "path".into(),
                value: value.into(),
                disabled: false,
            }],
            kind: RequestKind::Http {
                body: Some(RequestBody::Single(Body::Raw(RawBody {
                    kind: RawBodyKind::Text,
                    data: value.into(),
                }))),
            },
            authentication: Some(probe_core::Authentication {
                kind: probe_core::AuthenticationKind::Bearer,
                properties: [(
                    "token".into(),
                    probe_core::AuthenticationValue::String(value.into()),
                )]
                .into(),
            }),
            settings: probe_core::RequestSettings {
                max_redirects: Some(sequence as usize),
                ..Default::default()
            },
        }
    }
    let baseline = Request::default();
    let local = changed("local", 1.0);
    let incoming = changed("incoming", 2.0);
    for (left, right) in [(&local, &baseline), (&baseline, &local), (&local, &local)] {
        let (merged, conflicts) = Request::reconcile(&baseline, left, right);
        assert_eq!(merged, local);
        assert!(conflicts.is_empty());
    }
    let (merged, conflicts) = Request::reconcile(&baseline, &local, &incoming);
    assert_eq!(merged, baseline);
    assert_eq!(
        conflicts,
        [
            "name",
            "sequence",
            "description",
            "docs",
            "method",
            "URL",
            "headers",
            "query parameters",
            "path parameters",
            "body",
            "authentication",
            "settings",
        ]
    );

    // Every supported save change also survives reconciliation with a separate
    // incoming metadata edit. Unsupported sequence/settings edits stay incoming.
    let mut draft = local;
    draft.metadata.sequence = None;
    draft.settings = Default::default();
    let update = RequestUpdate::between(Some(&baseline), &draft).unwrap();
    let mut saved = baseline.clone();
    update.apply(&mut saved).unwrap();
    assert_eq!(saved, draft);
    let mut incoming = baseline.clone();
    incoming.metadata.sequence = Some(3.0);
    incoming.settings.follow_redirects = Some(false);
    let (merged, conflicts) = Request::reconcile(&baseline, &draft, &incoming);
    assert!(conflicts.is_empty());
    draft.metadata.sequence = incoming.metadata.sequence;
    draft.settings = incoming.settings;
    assert_eq!(merged, draft);
}

#[test]
fn reconciliation_keeps_protocol_and_graphql_body_as_one_conflict_unit() {
    let baseline = Request {
        kind: RequestKind::Graphql {
            body: Some(GraphqlBody::Single(GraphqlOperation::default())),
        },
        ..Request::default()
    };
    let mut local = baseline.clone();
    local
        .apply_graphql_update(&probe_core::GraphqlUpdate {
            query: FieldPatch::Set("query { viewer }".into()),
            ..Default::default()
        })
        .unwrap();
    let mut incoming = baseline.clone();
    incoming
        .apply_graphql_update(&probe_core::GraphqlUpdate {
            operation_name: FieldPatch::Set("Viewer".into()),
            ..Default::default()
        })
        .unwrap();
    let (merged, conflicts) = Request::reconcile(&baseline, &local, &incoming);
    assert_eq!(conflicts, ["body"]);
    assert_eq!(merged, baseline);
    incoming.kind = RequestKind::Http { body: None };
    assert_eq!(Request::reconcile(&baseline, &local, &incoming).1, ["body"]);
    assert_eq!(
        Request::reconcile(&baseline, &baseline, &incoming),
        (incoming, vec![])
    );
}
