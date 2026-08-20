use std::collections::BTreeMap;

use probe_core::{
    Authentication, AuthenticationKind, AuthenticationValue, Body, Environment,
    EnvironmentResolutionError, EnvironmentVariable, FormField, Header, HttpRequest, MultipartPart,
    MultipartPartKind, MultipartValue, QueryParameter, RawBody, RawBodyKind, RequestBody,
    SecretVariable, Variable, VariableValue, VariableValueSet, VariableValueVariant,
    resolve_environment, resolve_request,
};

fn variable(name: &str, value: &str) -> EnvironmentVariable {
    EnvironmentVariable::Plain(Variable {
        name: Some(name.to_owned()),
        value: Some(VariableValueSet::Single(VariableValue::String(
            value.to_owned(),
        ))),
        disabled: false,
    })
}

fn environment(
    name: &str,
    extends: Option<&str>,
    variables: Vec<EnvironmentVariable>,
) -> Environment {
    Environment {
        name: name.to_owned(),
        color: None,
        extends: extends.map(str::to_owned),
        dot_env_file_path: None,
        variables,
    }
}

#[test]
fn resolves_inheritance_overrides_variants_and_nested_values() {
    let environments = vec![
        environment(
            "base",
            None,
            vec![
                variable("host", "api.example.com"),
                variable("baseUrl", "https://{{host}}"),
                EnvironmentVariable::Plain(Variable {
                    name: Some("region".to_owned()),
                    value: Some(VariableValueSet::Variants(vec![
                        VariableValueVariant {
                            title: "US".to_owned(),
                            selected: false,
                            value: VariableValue::String("us".to_owned()),
                        },
                        VariableValueVariant {
                            title: "AU".to_owned(),
                            selected: true,
                            value: VariableValue::String("au".to_owned()),
                        },
                    ])),
                    disabled: false,
                }),
            ],
        ),
        environment(
            "development",
            Some("base"),
            vec![variable("host", "dev.example.com")],
        ),
    ];

    let resolved = resolve_environment(&environments, "development").unwrap();

    assert_eq!(resolved.name(), "development");
    assert_eq!(resolved.variable("host"), Some("dev.example.com"));
    assert_eq!(
        resolved.variable("baseUrl"),
        Some("https://dev.example.com")
    );
    assert_eq!(resolved.variable("region"), Some("au"));
    assert_eq!(
        resolved
            .interpolate("{{ baseUrl }}/{{region}}/users")
            .unwrap(),
        "https://dev.example.com/au/users"
    );
}

#[test]
fn resolves_supported_request_fields_without_mutating_the_source() {
    let request = HttpRequest {
        method: Some("{{method}}".to_owned()),
        url: Some("{{baseUrl}}/users".to_owned()),
        headers: vec![Header {
            name: "X-{{tenant}}".to_owned(),
            value: "Bearer {{token}}".to_owned(),
            disabled: false,
        }],
        query_parameters: vec![QueryParameter {
            name: "tenant".to_owned(),
            value: "{{tenant}}".to_owned(),
            disabled: false,
        }],
        path_parameters: vec![QueryParameter {
            name: "tenant".to_owned(),
            value: "{{tenant}}".to_owned(),
            disabled: false,
        }],
        body: Some(RequestBody::Variants(vec![probe_core::BodyVariant {
            title: "form".to_owned(),
            selected: true,
            body: Body::FormUrlEncoded(vec![FormField {
                name: "owner".to_owned(),
                value: "{{tenant}}".to_owned(),
                disabled: false,
            }]),
        }])),
        authentication: Some(Authentication {
            kind: AuthenticationKind::Bearer,
            properties: BTreeMap::from([(
                "token".to_owned(),
                AuthenticationValue::String("{{token}}".to_owned()),
            )]),
        }),
        ..HttpRequest::default()
    };
    let resolved = resolve_environment(
        &[environment(
            "complete",
            None,
            vec![
                variable("method", "POST"),
                variable("baseUrl", "https://dev.example.com"),
                variable("token", "test-token"),
                variable("tenant", "probe"),
            ],
        )],
        "complete",
    )
    .unwrap();

    let request_with_values = resolve_request(&request, &resolved).unwrap();

    assert_eq!(request.url.as_deref(), Some("{{baseUrl}}/users"));
    assert_eq!(request_with_values.method.as_deref(), Some("POST"));
    assert_eq!(
        request_with_values.url.as_deref(),
        Some("https://dev.example.com/users")
    );
    assert_eq!(request_with_values.headers[0].name, "X-probe");
    assert_eq!(request_with_values.headers[0].value, "Bearer test-token");
    assert_eq!(request_with_values.query_parameters[0].value, "probe");
    assert_eq!(request_with_values.path_parameters[0].value, "probe");
    let Some(RequestBody::Variants(variants)) = &request_with_values.body else {
        panic!("expected body variants");
    };
    let Body::FormUrlEncoded(fields) = &variants[0].body else {
        panic!("expected form body");
    };
    assert_eq!(fields[0].value, "probe");
    assert_eq!(
        request_with_values
            .authentication
            .as_ref()
            .unwrap()
            .properties["token"],
        AuthenticationValue::String("test-token".to_owned())
    );
}

#[test]
fn reports_missing_disabled_and_secret_variables() {
    let disabled = EnvironmentVariable::Plain(Variable {
        name: Some("disabled".to_owned()),
        value: Some(VariableValueSet::Single(VariableValue::String(
            "hidden".to_owned(),
        ))),
        disabled: true,
    });
    let secret = EnvironmentVariable::Secret(SecretVariable {
        name: Some("token".to_owned()),
        value_type: None,
        disabled: false,
    });
    let environments = [environment("development", None, vec![disabled, secret])];
    let resolved = resolve_environment(&environments, "development").unwrap();

    assert_eq!(
        resolved.interpolate("{{missing}}").unwrap_err(),
        EnvironmentResolutionError::MissingVariable("missing".to_owned())
    );
    assert_eq!(
        resolved.interpolate("{{disabled}}").unwrap_err(),
        EnvironmentResolutionError::MissingVariable("disabled".to_owned())
    );
    assert_eq!(
        resolved.interpolate("{{token}}").unwrap_err(),
        EnvironmentResolutionError::SecretVariableUnavailable("token".to_owned())
    );
}

#[test]
fn rejects_environment_and_variable_cycles() {
    let inheritance_cycle = [
        environment("a", Some("b"), vec![]),
        environment("b", Some("a"), vec![]),
    ];
    assert!(matches!(
        resolve_environment(&inheritance_cycle, "a"),
        Err(EnvironmentResolutionError::EnvironmentInheritanceCycle(_))
    ));

    let variable_cycle = [environment(
        "development",
        None,
        vec![variable("a", "{{b}}"), variable("b", "{{a}}")],
    )];
    assert!(matches!(
        resolve_environment(&variable_cycle, "development"),
        Err(EnvironmentResolutionError::VariableInterpolationCycle(_))
    ));
}

#[test]
fn rejects_unselected_variable_variants() {
    let environments = [environment(
        "development",
        None,
        vec![EnvironmentVariable::Plain(Variable {
            name: Some("region".to_owned()),
            value: Some(VariableValueSet::Variants(vec![VariableValueVariant {
                title: "AU".to_owned(),
                selected: false,
                value: VariableValue::String("au".to_owned()),
            }])),
            disabled: false,
        })],
    )];

    assert_eq!(
        resolve_environment(&environments, "development").unwrap_err(),
        EnvironmentResolutionError::NoSelectedVariant {
            environment: "development".to_owned(),
            variable: "region".to_owned(),
        }
    );
}

#[test]
fn resolves_raw_and_multipart_body_values() {
    let environment = resolve_environment(
        &[environment(
            "development",
            None,
            vec![variable("value", "resolved")],
        )],
        "development",
    )
    .unwrap();
    let mut raw_request = HttpRequest {
        body: Some(RequestBody::Single(Body::Raw(RawBody {
            kind: RawBodyKind::Json,
            data: "{\"value\":\"{{value}}\"}".to_owned(),
        }))),
        ..HttpRequest::default()
    };
    let multipart_request = HttpRequest {
        body: Some(RequestBody::Single(Body::Multipart(vec![MultipartPart {
            name: "upload".to_owned(),
            kind: MultipartPartKind::File,
            value: MultipartValue::Multiple(vec!["./{{value}}.txt".to_owned()]),
            content_type: Some("text/{{value}}".to_owned()),
            disabled: false,
        }]))),
        ..HttpRequest::default()
    };

    raw_request = resolve_request(&raw_request, &environment).unwrap();
    let multipart_request = resolve_request(&multipart_request, &environment).unwrap();
    let Some(RequestBody::Single(Body::Raw(raw))) = raw_request.body else {
        panic!("expected raw body");
    };
    assert_eq!(raw.data, "{\"value\":\"resolved\"}");
    let Some(RequestBody::Single(Body::Multipart(parts))) = multipart_request.body else {
        panic!("expected multipart body");
    };
    assert_eq!(
        parts[0].value,
        MultipartValue::Multiple(vec!["./resolved.txt".to_owned()])
    );
    assert_eq!(parts[0].content_type.as_deref(), Some("text/resolved"));
}

#[test]
fn set_environment_variable_updates_overrides_and_rejects_secrets() {
    let mut environments = vec![
        environment(
            "base",
            None,
            vec![
                variable("host", "api.example.com"),
                EnvironmentVariable::Secret(SecretVariable {
                    name: Some("inheritedSecret".to_owned()),
                    value_type: None,
                    disabled: false,
                }),
            ],
        ),
        environment(
            "development",
            Some("base"),
            vec![
                variable("token", "development-token"),
                EnvironmentVariable::Secret(SecretVariable {
                    name: Some("secretToken".to_owned()),
                    value_type: None,
                    disabled: false,
                }),
            ],
        ),
    ];

    probe_core::set_environment_variable(
        &mut environments,
        "development",
        "token",
        "rotated".to_owned(),
    )
    .unwrap();
    probe_core::set_environment_variable(
        &mut environments,
        "development",
        "host",
        "dev.example.com".to_owned(),
    )
    .unwrap();

    let resolved = resolve_environment(&environments, "development").unwrap();
    assert_eq!(resolved.variable("token"), Some("rotated"));
    assert_eq!(resolved.variable("host"), Some("dev.example.com"));
    assert_eq!(
        resolve_environment(&environments, "base")
            .unwrap()
            .variable("host"),
        Some("api.example.com")
    );

    assert_eq!(
        probe_core::set_environment_variable(
            &mut environments,
            "development",
            "secretToken",
            "nope".to_owned(),
        )
        .unwrap_err(),
        EnvironmentResolutionError::SecretVariableUnavailable("secretToken".to_owned())
    );
    assert_eq!(
        probe_core::set_environment_variable(
            &mut environments,
            "development",
            "inheritedSecret",
            "nope".to_owned(),
        )
        .unwrap_err(),
        EnvironmentResolutionError::SecretVariableUnavailable("inheritedSecret".to_owned())
    );
    assert!(
        environments[1]
            .variables
            .iter()
            .all(|variable| match variable {
                EnvironmentVariable::Plain(variable) => {
                    variable.name.as_deref() != Some("inheritedSecret")
                }
                EnvironmentVariable::Secret(_) => true,
            })
    );
}
