use std::collections::{BTreeMap, BTreeSet};

use probe_core::{
    Authentication, AuthenticationKind, AuthenticationValue, Body, Environment,
    EnvironmentResolutionError, EnvironmentVariable, FormField, Header, MultipartPart,
    MultipartPartKind, MultipartValue, QueryParameter, RawBody, RawBodyKind, Request, RequestBody,
    RequestKind, ResolvedEnvironment, SecretVariable, Variable, VariableStatus, VariableValue,
    VariableValueSet, VariableValueVariant, resolve_environment,
    resolve_environment_with_overrides, resolve_request, variable_status,
};

struct FakeProvider;
impl probe_core::SecretProvider for FakeProvider {
    fn resolve_secret(
        &self,
        context: &probe_core::SecretContext<'_>,
    ) -> Result<Option<probe_core::SecretValue>, probe_core::SecretError> {
        Ok((context.variable_name == "token").then(|| {
            probe_core::SecretValue::new("SUPER_SECRET_VALUE_THAT_MUST_NEVER_APPEAR".to_owned())
        }))
    }
}

#[test]
fn runtime_secrets_are_separate_and_follow_effective_inheritance() {
    let base = environment(
        "base",
        None,
        vec![secret("token"), variable("derived", "Bearer {{token}}")],
    );
    let child = environment("child", Some("base"), vec![]);
    let resolved = probe_core::resolve_environment_with_provider(
        &[base.clone(), child],
        Some("child"),
        &[],
        &FakeProvider,
        None,
    )
    .unwrap();
    assert_eq!(resolved.variable("token"), None);
    assert_eq!(resolved.variable("derived"), None);
    assert!(resolved.has_resolved_secrets());
    assert_eq!(
        resolved.redact_secrets(
            "Bearer SUPER_SECRET_VALUE_THAT_MUST_NEVER_APPEAR; SUPER_SECRET_VALUE_THAT_MUST_NEVER_APPEAR"
        ),
        "[REDACTED]; [REDACTED]"
    );
    assert_eq!(resolved.variable_status("token"), VariableStatus::Resolved);
    assert_eq!(
        resolved.interpolate("{{derived}}").unwrap(),
        "Bearer SUPER_SECRET_VALUE_THAT_MUST_NEVER_APPEAR"
    );
    assert_eq!(
        resolved
            .interpolate_for_presentation("{{derived}}", true)
            .unwrap(),
        "{{derived}}"
    );
    let debug = format!("{resolved:?}");
    assert!(debug.contains("child"));
    assert!(debug.contains("token"));
    assert!(!debug.contains("SUPER_SECRET_VALUE_THAT_MUST_NEVER_APPEAR"));
    let secret_value =
        probe_core::SecretValue::new("SUPER_SECRET_VALUE_THAT_MUST_NEVER_APPEAR".to_owned());
    assert_eq!(format!("{secret_value:?}"), "SecretValue([REDACTED])");
    assert_eq!(format!("{secret_value}"), "[REDACTED]");

    let plain_child = environment("plain", Some("base"), vec![variable("token", "public")]);
    let plain = probe_core::resolve_environment_with_provider(
        &[base.clone(), plain_child],
        Some("plain"),
        &[],
        &FakeProvider,
        None,
    )
    .unwrap();
    assert_eq!(plain.variable("token"), Some("public"));
    assert_eq!(plain.variable("derived"), Some("Bearer public"));
    let secret_child = environment(
        "secret",
        Some("base"),
        vec![variable("token", "public"), secret("token2")],
    );
    let secret = probe_core::resolve_environment_with_provider(
        &[base, secret_child],
        Some("secret"),
        &[],
        &FakeProvider,
        None,
    )
    .unwrap();
    assert_eq!(secret.variable("token"), Some("public"));
    assert_eq!(
        secret.variable_status("token2"),
        VariableStatus::SecretWithoutValue
    );
}

#[test]
fn secret_runtime_override_is_never_public() {
    let environments = [environment("local", None, vec![secret("token")])];
    let resolved = probe_core::resolve_environment_with_provider(
        &environments,
        Some("local"),
        &[(
            "token".into(),
            "SUPER_SECRET_VALUE_THAT_MUST_NEVER_APPEAR".into(),
        )],
        &FakeProvider,
        None,
    )
    .unwrap();
    assert_eq!(resolved.variable("token"), None);
    assert_eq!(
        resolved.interpolate("{{token}}").unwrap(),
        "SUPER_SECRET_VALUE_THAT_MUST_NEVER_APPEAR"
    );
    assert_eq!(
        resolved
            .interpolate_for_presentation("{{token}}", false)
            .unwrap(),
        "{{token}}"
    );
    assert!(!format!("{resolved:?}").contains("SUPER_SECRET_VALUE_THAT_MUST_NEVER_APPEAR"));
    let without_provider = resolve_environment_with_overrides(
        &environments,
        Some("local"),
        &[(
            "token".into(),
            "SUPER_SECRET_VALUE_THAT_MUST_NEVER_APPEAR".into(),
        )],
    )
    .unwrap();
    assert_eq!(without_provider.variable("token"), None);
    assert_eq!(
        without_provider
            .interpolate_for_presentation("{{token}}", false)
            .unwrap(),
        "{{token}}"
    );
    assert!(!format!("{without_provider:?}").contains("SUPER_SECRET_VALUE_THAT_MUST_NEVER_APPEAR"));
}

#[test]
fn provider_failure_is_distinct_and_never_formats_secret_material() {
    struct FailingProvider;
    impl probe_core::SecretProvider for FailingProvider {
        fn resolve_secret(
            &self,
            _: &probe_core::SecretContext<'_>,
        ) -> Result<Option<probe_core::SecretValue>, probe_core::SecretError> {
            Err(probe_core::SecretError)
        }
    }
    let environments = [environment("local", None, vec![secret("token")])];
    let resolved = probe_core::resolve_environment_with_provider(
        &environments,
        Some("local"),
        &[],
        &FailingProvider,
        None,
    )
    .unwrap();
    assert!(!resolved.has_resolved_secrets());
    assert_eq!(
        resolved.redact_secrets("ordinary output"),
        "ordinary output"
    );
    let error = resolved.interpolate("{{token}}").unwrap_err();
    assert_eq!(
        error,
        EnvironmentResolutionError::SecretProviderFailure("token".into())
    );
    assert!(!format!("{error:?} {error}").contains("SUPER_SECRET_VALUE_THAT_MUST_NEVER_APPEAR"));
    assert_eq!(
        error.to_string(),
        "secret provider failed for variable: token"
    );
    assert_eq!(
        resolved.interpolate("ordinary value").unwrap(),
        "ordinary value"
    );
    assert_eq!(
        resolved
            .interpolate_for_presentation("{{token}}", false)
            .unwrap_err(),
        error
    );
}

#[test]
fn provider_failure_is_deferred_through_unused_plain_variables() {
    struct FailingProvider;
    impl probe_core::SecretProvider for FailingProvider {
        fn resolve_secret(
            &self,
            _: &probe_core::SecretContext<'_>,
        ) -> Result<Option<probe_core::SecretValue>, probe_core::SecretError> {
            Err(probe_core::SecretError)
        }
    }
    let environments = [environment(
        "local",
        None,
        vec![
            secret("unusedToken"),
            variable("authorization", "Bearer {{unusedToken}}"),
            variable("chained", "{{authorization}}"),
            variable("ordinary", "public"),
        ],
    )];
    let resolved = probe_core::resolve_environment_with_provider(
        &environments,
        Some("local"),
        &[],
        &FailingProvider,
        None,
    )
    .unwrap();
    assert_eq!(resolved.variable("ordinary"), Some("public"));
    assert_eq!(resolved.variable("authorization"), None);
    assert_eq!(resolved.variable("chained"), None);
    assert_eq!(
        resolved.variable_status("authorization"),
        VariableStatus::SecretWithoutValue
    );
    let unrelated = Request {
        url: Some("https://example.test/{{ordinary}}".into()),
        ..Request::default()
    };
    assert_eq!(
        resolve_request(&unrelated, &resolved)
            .unwrap()
            .url
            .as_deref(),
        Some("https://example.test/public")
    );
    let error = EnvironmentResolutionError::SecretProviderFailure("unusedToken".into());
    for name in ["unusedToken", "authorization", "chained"] {
        assert_eq!(
            resolved
                .interpolate(&format!("{{{{{name}}}}}"))
                .unwrap_err(),
            error
        );
        assert_eq!(
            resolved
                .interpolate_for_presentation(&format!("{{{{{name}}}}}"), false)
                .unwrap_err(),
            error
        );
    }
}

#[test]
fn missing_secret_in_unused_plain_variable_is_deferred_until_interpolation() {
    let environments = [environment(
        "local",
        None,
        vec![
            secret("token"),
            variable("authorization", "Bearer {{token}}"),
            variable("ordinary", "public"),
        ],
    )];
    let resolved = resolve_environment(&environments, "local").unwrap();
    assert_eq!(resolved.variable("ordinary"), Some("public"));
    assert_eq!(resolved.variable("authorization"), None);
    assert_eq!(
        resolved.interpolate("{{authorization}}").unwrap_err(),
        EnvironmentResolutionError::SecretVariableUnavailable("token".into())
    );
}

#[test]
fn request_presentation_retains_secret_references_instead_of_execution_values() {
    let environments = [environment("local", None, vec![secret("token")])];
    let resolved = probe_core::resolve_environment_with_provider(
        &environments,
        Some("local"),
        &[],
        &FakeProvider,
        None,
    )
    .unwrap();
    let request = Request {
        url: Some("https://example.test/{{token}}".into()),
        headers: vec![Header {
            name: "Authorization".into(),
            value: "Bearer {{token}}".into(),
            disabled: false,
        }],
        ..Request::default()
    };
    let execution = resolve_request(&request, &resolved).unwrap();
    let display = probe_core::resolve_request_for_presentation(&request, &resolved, true).unwrap();
    assert!(
        execution
            .url
            .unwrap()
            .contains("SUPER_SECRET_VALUE_THAT_MUST_NEVER_APPEAR")
    );
    assert_eq!(
        display.url.as_deref(),
        Some("https://example.test/{{token}}")
    );
    assert_eq!(display.headers[0].value, "Bearer {{token}}");
    assert!(!format!("{display:?}").contains("SUPER_SECRET_VALUE_THAT_MUST_NEVER_APPEAR"));
}

#[test]
fn plain_to_secret_override_uses_the_effective_declaration() {
    let environments = [
        environment("base", None, vec![variable("token", "public")]),
        environment("child", Some("base"), vec![secret("token")]),
    ];
    let resolved = probe_core::resolve_environment_with_provider(
        &environments,
        Some("child"),
        &[],
        &FakeProvider,
        None,
    )
    .unwrap();
    assert_eq!(resolved.variable("token"), None);
    assert_eq!(
        resolved.interpolate("{{token}}").unwrap(),
        "SUPER_SECRET_VALUE_THAT_MUST_NEVER_APPEAR"
    );
}

#[test]
fn runtime_secret_text_is_opaque_even_when_it_looks_like_a_template() {
    let environments = [environment(
        "local",
        None,
        vec![secret("token"), variable("derived", "Bearer {{token}}")],
    )];
    let literal = "abc{{nonce}}";
    let resolved = probe_core::resolve_environment_with_provider(
        &environments,
        Some("local"),
        &[("token".into(), literal.into())],
        &FakeProvider,
        None,
    )
    .unwrap();
    assert_eq!(resolved.interpolate("{{token}}").unwrap(), literal);
    assert_eq!(
        resolved.interpolate("{{derived}}").unwrap(),
        "Bearer abc{{nonce}}"
    );
    assert_eq!(resolved.variable("derived"), None);
    assert_eq!(
        resolved
            .interpolate_for_presentation("{{derived}}", false)
            .unwrap(),
        "{{derived}}"
    );
}

fn variable(name: &str, value: &str) -> EnvironmentVariable {
    EnvironmentVariable::Plain(Variable {
        name: Some(name.to_owned()),
        value: Some(VariableValueSet::Single(VariableValue::String(
            value.to_owned(),
        ))),
        disabled: false,
    })
}

fn secret(name: &str) -> EnvironmentVariable {
    EnvironmentVariable::Secret(SecretVariable {
        name: Some(name.to_owned()),
        value_type: None,
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
fn runtime_variables_override_selected_and_inherited_values_before_resolution() {
    let environments = vec![
        environment(
            "base",
            None,
            vec![
                variable("baseUrl", "https://api.example.com"),
                variable("usersUrl", "{{baseUrl}}/users"),
                variable("region", "au"),
            ],
        ),
        environment(
            "development",
            Some("base"),
            vec![variable("baseUrl", "https://dev.example.com")],
        ),
    ];

    let resolved = resolve_environment_with_overrides(
        &environments,
        Some("development"),
        &[
            (
                "baseUrl".to_owned(),
                "https://staging.example.com".to_owned(),
            ),
            ("region".to_owned(), "us".to_owned()),
            ("runtimeOnly".to_owned(), "present".to_owned()),
        ],
    )
    .unwrap();

    assert_eq!(
        resolved.variable("baseUrl"),
        Some("https://staging.example.com")
    );
    assert_eq!(resolved.variable("region"), Some("us"));
    assert_eq!(resolved.variable("runtimeOnly"), Some("present"));
    assert_eq!(
        resolved.variable("usersUrl"),
        Some("https://staging.example.com/users")
    );
}

#[test]
fn runtime_only_variables_resolve_without_a_selected_environment() {
    let resolved =
        resolve_environment_with_overrides(&[], None, &[("userId".to_owned(), "123".to_owned())])
            .unwrap();

    assert_eq!(resolved.name(), "");
    assert_eq!(resolved.variable("userId"), Some("123"));
    assert_eq!(
        resolved.interpolate("/users/{{userId}}").unwrap(),
        "/users/123"
    );
}

#[test]
fn duplicate_runtime_variables_use_the_last_value() {
    let resolved = resolve_environment_with_overrides(
        &[],
        None,
        &[
            ("userId".to_owned(), "123".to_owned()),
            ("userId".to_owned(), "456".to_owned()),
        ],
    )
    .unwrap();

    assert_eq!(resolved.variable("userId"), Some("456"));
}

#[test]
fn runtime_variables_reject_an_empty_name() {
    assert_eq!(
        resolve_environment_with_overrides(&[], None, &[(String::new(), "value".to_owned())])
            .unwrap_err(),
        EnvironmentResolutionError::InvalidVariableName
    );
}

#[test]
fn resolves_supported_request_fields_without_mutating_the_source() {
    let request = Request {
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
        kind: RequestKind::Http {
            body: Some(RequestBody::Variants(vec![probe_core::BodyVariant {
                title: "form".to_owned(),
                selected: true,
                body: Body::FormUrlEncoded(vec![FormField {
                    name: "owner".to_owned(),
                    value: "{{tenant}}".to_owned(),
                    disabled: false,
                }]),
            }])),
        },
        authentication: Some(Authentication {
            kind: AuthenticationKind::Bearer,
            properties: BTreeMap::from([(
                "token".to_owned(),
                AuthenticationValue::String("{{token}}".to_owned()),
            )]),
        }),
        ..Request::default()
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
    let Some(RequestBody::Variants(variants)) = request_with_values.http_body() else {
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
fn preserves_undefined_variables_and_rejects_unavailable_secrets() {
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
        resolved.interpolate("{{missing}}"),
        Ok("{{missing}}".to_owned())
    );
    assert_eq!(
        resolved.interpolate("prefix {{ disabled }} suffix"),
        Ok("prefix {{ disabled }} suffix".to_owned())
    );
    assert_eq!(
        resolved.interpolate_strict("{{missing}}").unwrap_err(),
        EnvironmentResolutionError::MissingVariable("missing".to_owned())
    );
    assert_eq!(
        resolved.interpolate_strict("{{disabled}}").unwrap_err(),
        EnvironmentResolutionError::MissingVariable("disabled".to_owned())
    );
    assert_eq!(
        resolved.interpolate("{{token}}").unwrap_err(),
        EnvironmentResolutionError::SecretVariableUnavailable("token".to_owned())
    );
}

#[test]
fn variable_status_matches_strict_interpolation_for_effective_names() {
    let disabled = EnvironmentVariable::Plain(Variable {
        name: Some("disabled".to_owned()),
        value: Some(VariableValueSet::Single(VariableValue::String(
            "hidden".to_owned(),
        ))),
        disabled: true,
    });
    let environments = [
        environment(
            "base",
            None,
            vec![variable("inherited", "from-base"), secret("token")],
        ),
        environment(
            "development",
            Some("base"),
            vec![variable("local", "child"), variable("empty", ""), disabled],
        ),
    ];
    let resolved = resolve_environment(&environments, "development").unwrap();

    assert_status(
        &resolved,
        "inherited",
        VariableStatus::Resolved,
        Some("from-base"),
    );
    assert_status(&resolved, "local", VariableStatus::Resolved, Some("child"));
    assert_status(&resolved, "empty", VariableStatus::Resolved, Some(""));
    assert_status(&resolved, "token", VariableStatus::SecretWithoutValue, None);
    assert_status(&resolved, "disabled", VariableStatus::Missing, None);
    assert_status(&resolved, "absent", VariableStatus::Missing, None);

    let mut variables = BTreeMap::new();
    variables.insert("token".to_owned(), "stored".to_owned());
    let secrets = BTreeSet::from(["token".to_owned()]);
    assert_eq!(
        variable_status(&variables, &secrets, "token"),
        VariableStatus::SecretWithoutValue
    );
}

fn assert_status(
    resolved: &ResolvedEnvironment,
    name: &str,
    expected: VariableStatus,
    value: Option<&str>,
) {
    let status = resolved.variable_status(name);
    assert_eq!(status, expected, "{name}");
    assert_eq!(resolved.variable(name), value, "{name}");
    let strict = resolved.interpolate_strict(&format!("{{{{{name}}}}}"));
    assert_eq!(
        status.is_resolved(),
        strict.is_ok(),
        "{name} status {status:?} should agree with strict interpolation {strict:?}"
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
    let mut raw_request = Request {
        kind: RequestKind::Http {
            body: Some(RequestBody::Single(Body::Raw(RawBody {
                kind: RawBodyKind::Json,
                data: "{\"value\":\"{{value}}\"}".to_owned(),
            }))),
        },
        ..Request::default()
    };
    let multipart_request = Request {
        kind: RequestKind::Http {
            body: Some(RequestBody::Single(Body::Multipart(vec![MultipartPart {
                name: "upload".to_owned(),
                kind: MultipartPartKind::File,
                value: MultipartValue::Multiple(vec!["./{{value}}.txt".to_owned()]),
                content_type: Some("text/{{value}}".to_owned()),
                disabled: false,
            }]))),
        },
        ..Request::default()
    };

    raw_request = resolve_request(&raw_request, &environment).unwrap();
    let multipart_request = resolve_request(&multipart_request, &environment).unwrap();
    let Some(RequestBody::Single(Body::Raw(raw))) = raw_request.http_body() else {
        panic!("expected raw body");
    };
    assert_eq!(raw.data, "{\"value\":\"resolved\"}");
    let Some(RequestBody::Single(Body::Multipart(parts))) = multipart_request.http_body() else {
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
    assert_eq!(
        probe_core::set_environment_variable(
            &mut environments,
            "missing",
            "host",
            "nope".to_owned(),
        )
        .unwrap_err(),
        EnvironmentResolutionError::EnvironmentNotFound("missing".to_owned())
    );
    assert_eq!(
        probe_core::set_environment_variable(
            &mut environments,
            "development",
            "",
            "nope".to_owned()
        )
        .unwrap_err(),
        EnvironmentResolutionError::InvalidVariableName
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

#[test]
fn setting_a_variant_variable_preserves_its_type_and_other_choices() {
    let variants = |selected: bool| {
        EnvironmentVariable::Plain(Variable {
            name: Some("mode".to_owned()),
            value: Some(VariableValueSet::Variants(vec![
                VariableValueVariant {
                    title: "primary".to_owned(),
                    selected,
                    value: VariableValue::Typed {
                        kind: probe_core::VariableValueType::String,
                        data: "old".to_owned(),
                    },
                },
                VariableValueVariant {
                    title: "fallback".to_owned(),
                    selected: !selected,
                    value: VariableValue::String("untouched".to_owned()),
                },
            ])),
            disabled: false,
        })
    };
    for selected in [true, false] {
        let mut environments = vec![environment("local", None, vec![variants(selected)])];
        probe_core::set_environment_variable(&mut environments, "local", "mode", "new".to_owned())
            .unwrap();
        let EnvironmentVariable::Plain(variable) = &environments[0].variables[0] else {
            panic!("expected plain variable")
        };
        let Some(VariableValueSet::Variants(values)) = &variable.value else {
            panic!("expected variants")
        };
        assert_eq!(values[0].selected, selected);
        assert_eq!(values[1].selected, !selected);
        if selected {
            assert_eq!(
                values[0].value,
                VariableValue::Typed {
                    kind: probe_core::VariableValueType::String,
                    data: "new".to_owned()
                }
            );
            assert_eq!(
                values[1].value,
                VariableValue::String("untouched".to_owned())
            );
        } else {
            assert_eq!(
                values[0].value,
                VariableValue::Typed {
                    kind: probe_core::VariableValueType::String,
                    data: "old".to_owned()
                }
            );
            assert_eq!(values[1].value, VariableValue::String("new".to_owned()));
        }
    }
}

#[test]
fn unset_environment_variable_removes_local_entry_and_restores_parent() {
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
                variable("host", "dev.example.com"),
                variable("token", "development-token"),
                EnvironmentVariable::Secret(SecretVariable {
                    name: Some("secretToken".to_owned()),
                    value_type: None,
                    disabled: false,
                }),
            ],
        ),
    ];

    probe_core::unset_environment_variable(&mut environments, "development", "host").unwrap();
    let resolved = resolve_environment(&environments, "development").unwrap();
    assert_eq!(resolved.variable("host"), Some("api.example.com"));
    assert_eq!(resolved.variable("token"), Some("development-token"));
    assert_eq!(
        resolve_environment(&environments, "base")
            .unwrap()
            .variable("host"),
        Some("api.example.com")
    );

    assert_eq!(
        probe_core::unset_environment_variable(&mut environments, "development", "host")
            .unwrap_err(),
        EnvironmentResolutionError::VariableNotFound {
            environment: "development".to_owned(),
            variable: "host".to_owned(),
        }
    );
    assert_eq!(
        probe_core::unset_environment_variable(&mut environments, "development", "secretToken")
            .unwrap_err(),
        EnvironmentResolutionError::SecretVariableUnavailable("secretToken".to_owned())
    );
    assert_eq!(
        probe_core::unset_environment_variable(
            &mut environments,
            "development",
            "inheritedSecret",
        )
        .unwrap_err(),
        EnvironmentResolutionError::VariableNotFound {
            environment: "development".to_owned(),
            variable: "inheritedSecret".to_owned(),
        }
    );
    assert_eq!(
        probe_core::unset_environment_variable(&mut environments, "missing", "host").unwrap_err(),
        EnvironmentResolutionError::EnvironmentNotFound("missing".to_owned())
    );
    assert!(environments[1].variables.iter().any(|variable| {
        matches!(
            variable,
            EnvironmentVariable::Secret(secret) if secret.name.as_deref() == Some("secretToken")
        )
    }));
}

#[test]
fn create_environment_appends_and_validates_inheritance() {
    let mut environments = vec![environment(
        "base",
        None,
        vec![variable("host", "api.example.com")],
    )];

    probe_core::create_environment(
        &mut environments,
        "development".to_owned(),
        Some("base".to_owned()),
    )
    .unwrap();

    assert_eq!(environments.len(), 2);
    assert_eq!(environments[1].name, "development");
    assert_eq!(environments[1].extends.as_deref(), Some("base"));
    assert!(environments[1].variables.is_empty());
    assert_eq!(
        resolve_environment(&environments, "development")
            .unwrap()
            .variable("host"),
        Some("api.example.com")
    );
}

#[test]
fn create_environment_rejects_invalid_names_and_parents() {
    let mut environments = vec![environment("base", None, vec![])];

    assert_eq!(
        probe_core::create_environment(&mut environments, String::new(), None).unwrap_err(),
        EnvironmentResolutionError::InvalidEnvironmentName
    );
    assert_eq!(
        probe_core::create_environment(
            &mut environments,
            "base".to_owned(),
            Some("base".to_owned()),
        )
        .unwrap_err(),
        EnvironmentResolutionError::DuplicateEnvironment("base".to_owned())
    );
    assert_eq!(
        probe_core::create_environment(
            &mut environments,
            "staging".to_owned(),
            Some("missing".to_owned()),
        )
        .unwrap_err(),
        EnvironmentResolutionError::ParentEnvironmentNotFound {
            environment: "staging".to_owned(),
            parent: "missing".to_owned(),
        }
    );
}

#[test]
fn create_environment_rejects_inheritance_cycles() {
    let mut environments = vec![Environment {
        name: "a".to_owned(),
        color: None,
        extends: Some("b".to_owned()),
        dot_env_file_path: None,
        variables: Vec::new(),
    }];

    let before = environments.clone();
    assert!(matches!(
        probe_core::create_environment(&mut environments, "b".to_owned(), Some("a".to_owned()))
            .unwrap_err(),
        EnvironmentResolutionError::EnvironmentInheritanceCycle(_)
    ));
    assert_eq!(environments, before);
}

#[test]
fn revert_created_environment_removes_unused_names_but_keeps_parents() {
    let mut environments = vec![environment("base", None, vec![])];
    probe_core::create_environment(&mut environments, "staging".to_owned(), None).unwrap();
    probe_core::revert_created_environment(&mut environments, "staging");
    assert_eq!(environments.len(), 1);
    assert_eq!(environments[0].name, "base");

    probe_core::create_environment(
        &mut environments,
        "staging".to_owned(),
        Some("base".to_owned()),
    )
    .unwrap();
    probe_core::revert_created_environment(&mut environments, "base");
    assert_eq!(environments.len(), 2);
}

#[test]
fn replace_environment_validates_and_updates_child_references() {
    let mut environments = vec![
        environment("base", None, vec![]),
        environment("development", Some("base"), vec![]),
    ];
    let mut replacement = environments[0].clone();
    replacement.name = "shared".to_owned();
    probe_core::replace_environment(&mut environments, "base", replacement).unwrap();
    assert_eq!(environments[0].name, "shared");
    assert_eq!(environments[1].extends.as_deref(), Some("shared"));

    let before = environments.clone();
    let mut invalid = environments[1].clone();
    invalid.extends = Some("missing".to_owned());
    assert!(matches!(
        probe_core::replace_environment(&mut environments, "development", invalid),
        Err(EnvironmentResolutionError::ParentEnvironmentNotFound { .. })
    ));
    assert_eq!(environments, before);
}

#[test]
fn replace_environment_rejects_duplicate_variable_names() {
    let original = environment(
        "development",
        None,
        vec![variable("host", "dev.example.com")],
    );
    let cases = [
        (
            vec![
                variable("host", "one.example.com"),
                variable("host", "two.example.com"),
            ],
            "host",
        ),
        (vec![variable("token", "plain"), secret("token")], "token"),
        (vec![secret("token"), variable("token", "plain")], "token"),
    ];
    for (variables, name) in cases {
        let mut environments = vec![original.clone()];
        let before = environments.clone();
        let replacement = environment("development", None, variables);
        assert_eq!(
            probe_core::replace_environment(&mut environments, "development", replacement)
                .unwrap_err(),
            EnvironmentResolutionError::DuplicateVariable {
                environment: "development".to_owned(),
                variable: name.to_owned(),
            }
        );
        assert_eq!(environments, before);
    }
}

#[test]
fn delete_environment_rejects_parents() {
    let mut environments = vec![
        environment("base", None, vec![]),
        environment("development", Some("base"), vec![]),
    ];
    assert_eq!(
        probe_core::delete_environment(&mut environments, "base").unwrap_err(),
        EnvironmentResolutionError::EnvironmentInUse("base".to_owned())
    );
    probe_core::delete_environment(&mut environments, "development").unwrap();
    assert_eq!(environments.len(), 1);
}

fn effective_names(rows: &[probe_core::EffectiveEnvironmentVariable]) -> Vec<(&str, &str, bool)> {
    rows.iter()
        .map(|row| {
            (
                row.variable.name.as_deref().unwrap_or(""),
                row.defined_in.as_str(),
                row.direct_index.is_some(),
            )
        })
        .collect()
}

#[test]
fn effective_environment_variables_resolve_inheritance_overrides_and_secrets() {
    let environments = vec![
        environment(
            "root",
            None,
            vec![variable("host", "root.example.com"), secret("shadowed")],
        ),
        environment(
            "base",
            Some("root"),
            vec![
                variable("host", "base.example.com"),
                variable("region", "us"),
                EnvironmentVariable::Plain(Variable {
                    name: Some("disabled".to_owned()),
                    value: Some(VariableValueSet::Single(VariableValue::String(
                        "hidden".to_owned(),
                    ))),
                    disabled: true,
                }),
                variable("token", "parent-token"),
            ],
        ),
        environment(
            "development",
            Some("base"),
            vec![secret("token"), variable("local", "dev")],
        ),
    ];
    let rows = probe_core::effective_environment_variables(&environments, &environments[2]);
    assert_eq!(
        effective_names(&rows),
        vec![
            ("local", "development", true),
            ("host", "base", false),
            ("region", "base", false),
            ("disabled", "base", false),
        ]
    );
    assert!(
        rows.iter().any(|row| {
            row.variable.name.as_deref() == Some("disabled") && row.variable.disabled
        })
    );
}
