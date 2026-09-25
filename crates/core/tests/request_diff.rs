use probe_core::{
    FieldPatch, GraphqlBody, GraphqlBodyVariant, GraphqlOperation, GraphqlRequest, HttpRequest,
    RequestDiffError, RequestProtocol, RequestUpdate,
};

#[test]
fn diff_round_trips_optional_field_clears() {
    let base = GraphqlRequest {
        method: Some("POST".into()),
        url: Some("https://example.test/graphql".into()),
        body: Some(GraphqlBody::Single(GraphqlOperation {
            query: Some("query { viewer }".into()),
            ..GraphqlOperation::default()
        })),
        ..GraphqlRequest::default()
    }
    .into_request();
    assert!(
        RequestUpdate::between(Some(&base), &base)
            .unwrap()
            .is_empty()
    );
    let mut current = base.clone();
    current.method = None;
    current.url = None;
    if let RequestProtocol::Graphql(Some(GraphqlBody::Single(operation))) = &mut current.protocol {
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
fn diff_rejects_changes_without_persistence_support() {
    let base = HttpRequest::default();
    let cases = [
        (
            "request protocol",
            HttpRequest {
                protocol: RequestProtocol::Graphql(None),
                ..base.clone()
            },
        ),
        (
            "request settings",
            HttpRequest {
                settings: probe_core::RequestSettings {
                    follow_redirects: Some(false),
                    ..Default::default()
                },
                ..base.clone()
            },
        ),
        (
            "request sequence",
            HttpRequest {
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
    let base = GraphqlRequest {
        body: Some(GraphqlBody::Variants(vec![GraphqlBodyVariant {
            title: "first".into(),
            selected: true,
            body: GraphqlOperation {
                query: Some("query { viewer }".into()),
                ..Default::default()
            },
        }])),
        ..Default::default()
    }
    .into_request();
    let mut current = base.clone();
    if let RequestProtocol::Graphql(Some(GraphqlBody::Variants(variants))) = &mut current.protocol {
        variants[0].title = "renamed".into();
    }
    assert_eq!(
        RequestUpdate::between(Some(&base), &current),
        Err(RequestDiffError::UnsupportedChange("GraphQL body variants"))
    );
    if let RequestProtocol::Graphql(Some(GraphqlBody::Variants(variants))) = &mut current.protocol {
        variants[0].selected = false;
    }
    assert!(matches!(
        RequestUpdate::between(Some(&base), &current),
        Err(RequestDiffError::Graphql(
            probe_core::GraphqlRequestError::InvalidBodySelection(_)
        ))
    ));
    assert!(matches!(
        RequestUpdate::between(None, &current),
        Err(RequestDiffError::Graphql(
            probe_core::GraphqlRequestError::InvalidBodySelection(_)
        ))
    ));
}
