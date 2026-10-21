use std::{cell::RefCell, collections::BTreeMap};

use probe_core::{
    Environment, EnvironmentVariable, Header, QueryParameter, Request, SecretContext, SecretError,
    SecretProvider, SecretValue, SecretVariable, Variable, VariableUsage, VariableValue,
    VariableValueSet, request_secret_usages, resolve_environment_for_request_with_provider,
    resolve_request, resolve_request_for_presentation,
};

struct Provider {
    calls: RefCell<Vec<String>>,
    fail: bool,
}

impl SecretProvider for Provider {
    fn resolve_secret(
        &self,
        context: &SecretContext<'_>,
    ) -> Result<Option<SecretValue>, SecretError> {
        self.calls
            .borrow_mut()
            .push(context.variable_name.to_owned());
        if self.fail {
            Err(SecretError)
        } else {
            Ok(Some(SecretValue::new(
                "SUPER_SECRET_VALUE_THAT_MUST_NEVER_APPEAR".into(),
            )))
        }
    }
}

fn environment() -> Vec<Environment> {
    vec![Environment {
        name: "production".into(),
        color: None,
        extends: None,
        description: None,
        dot_env_file_path: None,
        client_certificates: Vec::new(),
        variables: vec![
            EnvironmentVariable::Secret(SecretVariable {
                name: Some("token".into()),
                value_type: None,
                disabled: false,
            }),
            EnvironmentVariable::Secret(SecretVariable {
                name: Some("unused".into()),
                value_type: None,
                disabled: false,
            }),
            EnvironmentVariable::Plain(Variable {
                name: Some("authorization".into()),
                value: Some(VariableValueSet::Single(VariableValue::String(
                    "Bearer {{token}}".into(),
                ))),
                disabled: false,
            }),
            EnvironmentVariable::Plain(Variable {
                name: Some("outer".into()),
                value: Some(VariableValueSet::Single(VariableValue::String(
                    "{{authorization}}".into(),
                ))),
                disabled: false,
            }),
            EnvironmentVariable::Plain(Variable {
                name: Some("ordinary".into()),
                value: Some(VariableValueSet::Single(VariableValue::String(
                    "plain".into(),
                ))),
                disabled: false,
            }),
        ],
    }]
}

#[test]
fn only_reachable_secrets_are_read_once_and_presentation_is_safe() {
    let environments = environment();
    let provider = Provider {
        calls: RefCell::new(Vec::new()),
        fail: false,
    };
    let request = Request {
        url: Some("{{outer}}/{{token}}/{{token}}".into()),
        ..Request::default()
    };
    let resolved = resolve_environment_for_request_with_provider(
        &request,
        &environments,
        Some("production"),
        &[],
        &provider,
        None,
    )
    .unwrap();
    assert_eq!(&*provider.calls.borrow(), &["token"]);
    assert_eq!(
        resolved.variables(),
        &BTreeMap::from([("ordinary".into(), "plain".into())])
    );
    assert!(!format!("{resolved:?}").contains("SUPER_SECRET_VALUE_THAT_MUST_NEVER_APPEAR"));
    let executed = resolve_request(&request, &resolved).unwrap();
    assert_eq!(
        executed.url.as_deref(),
        Some(
            "Bearer SUPER_SECRET_VALUE_THAT_MUST_NEVER_APPEAR/SUPER_SECRET_VALUE_THAT_MUST_NEVER_APPEAR/SUPER_SECRET_VALUE_THAT_MUST_NEVER_APPEAR"
        )
    );
    let presented = resolve_request_for_presentation(&request, &resolved, false).unwrap();
    assert!(!format!("{presented:?}").contains("SUPER_SECRET_VALUE_THAT_MUST_NEVER_APPEAR"));
}

#[test]
fn secret_usages_track_direct_and_transitive_provenance_without_plain_or_missing_values() {
    let request = Request {
        url: Some("https://example.com/{{ordinary}}/{{missing}}".into()),
        headers: vec![Header {
            name: "Authorization".into(),
            value: "{{outer}}".into(),
            disabled: false,
        }],
        query_parameters: vec![QueryParameter {
            name: "token".into(),
            value: "{{token}}".into(),
            disabled: false,
        }],
        path_parameters: vec![QueryParameter {
            name: "id".into(),
            value: "{{authorization}}".into(),
            disabled: false,
        }],
        ..Request::default()
    };
    let provider = Provider {
        calls: RefCell::new(Vec::new()),
        fail: false,
    };
    let resolved = resolve_environment_for_request_with_provider(
        &request,
        &environment(),
        Some("production"),
        &[],
        &provider,
        None,
    )
    .unwrap();
    let expected = std::collections::BTreeSet::from([
        VariableUsage::Header {
            name: "Authorization".into(),
        },
        VariableUsage::QueryParameter {
            name: "token".into(),
        },
        VariableUsage::PathParameter { name: "id".into() },
    ]);
    assert_eq!(
        request_secret_usages(&request, &resolved).unwrap(),
        expected
    );
    let presented = resolve_request_for_presentation(&request, &resolved, false).unwrap();
    assert_eq!(
        request_secret_usages(&presented, &resolved).unwrap(),
        expected
    );
    assert_eq!(&*provider.calls.borrow(), &["token"]);
}

#[test]
fn unused_failure_is_not_called_but_used_failure_fails_closed() {
    let environments = environment();
    let provider = Provider {
        calls: RefCell::new(Vec::new()),
        fail: true,
    };
    let unrelated = Request {
        url: Some("{{ordinary}}".into()),
        ..Request::default()
    };
    let resolved = resolve_environment_for_request_with_provider(
        &unrelated,
        &environments,
        Some("production"),
        &[],
        &provider,
        None,
    )
    .unwrap();
    assert!(provider.calls.borrow().is_empty());
    assert_eq!(
        resolve_request(&unrelated, &resolved)
            .unwrap()
            .url
            .as_deref(),
        Some("plain")
    );
    let used = Request {
        url: Some("{{outer}}".into()),
        ..Request::default()
    };
    let resolved = resolve_environment_for_request_with_provider(
        &used,
        &environments,
        Some("production"),
        &[],
        &provider,
        None,
    )
    .unwrap();
    assert_eq!(&*provider.calls.borrow(), &["token"]);
    assert!(resolve_request(&used, &resolved).is_err());
    assert!(!format!("{resolved:?}").contains("SUPER_SECRET_VALUE_THAT_MUST_NEVER_APPEAR"));
}

#[test]
fn plain_override_changes_the_secret_dependency_closure() {
    let environments = environment();
    let provider = Provider {
        calls: RefCell::new(Vec::new()),
        fail: false,
    };
    let request = Request {
        url: Some("{{outer}}".into()),
        ..Request::default()
    };
    let resolved = resolve_environment_for_request_with_provider(
        &request,
        &environments,
        Some("production"),
        &[("outer".into(), "{{unused}}".into())],
        &provider,
        None,
    )
    .unwrap();
    assert_eq!(&*provider.calls.borrow(), &["unused"]);
    assert_eq!(
        resolve_request(&request, &resolved).unwrap().url.as_deref(),
        Some("SUPER_SECRET_VALUE_THAT_MUST_NEVER_APPEAR")
    );
}

#[test]
fn binary_response_redaction_preserves_other_bytes() {
    let environments = environment();
    let provider = Provider {
        calls: RefCell::new(Vec::new()),
        fail: false,
    };
    let request = Request {
        url: Some("{{token}}".into()),
        ..Request::default()
    };
    let resolved = resolve_environment_for_request_with_provider(
        &request,
        &environments,
        Some("production"),
        &[],
        &provider,
        None,
    )
    .unwrap();
    let secret = b"SUPER_SECRET_VALUE_THAT_MUST_NEVER_APPEAR";
    let mut body = vec![0xff, 0x00];
    body.extend_from_slice(secret);
    body.push(0xfe);
    body.extend_from_slice(secret);
    let redacted = resolved.redact_secret_bytes(&body);
    assert_eq!(redacted, b"\xff\x00[REDACTED]\xfe[REDACTED]");
    assert!(!redacted.windows(secret.len()).any(|part| part == secret));
}

#[test]
fn execution_templates_share_request_reachability_and_interpolation_semantics() {
    use probe_core::resolve_environment_for_templates_with_provider;
    let environments = environment();
    let provider = Provider {
        calls: RefCell::new(Vec::new()),
        fail: false,
    };
    // These can be PEM paths, archive paths, domains, or passphrases: the resolver
    // deliberately knows nothing about certificate formats or filesystem access.
    let templates = [
        "{{ordinary}}/client.p12",
        "{{outer}}",
        "{{outer}}",
        "{{missing}}",
    ];
    let resolved = resolve_environment_for_templates_with_provider(
        templates,
        &environments,
        Some("production"),
        &[],
        &provider,
        None,
    )
    .unwrap();
    assert_eq!(*provider.calls.borrow(), ["token"]);
    assert_eq!(
        resolved.interpolate(templates[0]).unwrap(),
        "plain/client.p12"
    );
    assert_eq!(
        resolved.interpolate(templates[1]).unwrap(),
        "Bearer SUPER_SECRET_VALUE_THAT_MUST_NEVER_APPEAR"
    );
    assert_eq!(resolved.interpolate(templates[3]).unwrap(), "{{missing}}");
    assert!(resolved.interpolate_strict(templates[3]).is_err());
    assert_eq!(
        resolved
            .interpolate_for_presentation(templates[1], true)
            .unwrap(),
        "{{outer}}"
    );
    assert!(resolved.has_resolved_secrets());
    assert!(
        resolve_environment_for_templates_with_provider(
            ["{{unclosed"],
            &environments,
            Some("production"),
            &[],
            &provider,
            None,
        )
        .is_err()
    );
}

#[test]
fn redaction_combines_overlapping_values_without_rescanning_replacement_markers() {
    use probe_core::{SecretRedactor, resolve_environment_for_templates_with_provider};
    let environments = environment();
    let provider = Provider {
        calls: RefCell::new(Vec::new()),
        fail: false,
    };
    let resolved: Vec<_> = ["abc", "abcdef", "REDACTED", "é", "xabc", "abcdefgh"]
        .into_iter()
        .map(|value| {
            resolve_environment_for_templates_with_provider(
                ["{{token}}"],
                &environments,
                Some("production"),
                &[("token".into(), value.into())],
                &provider,
                None,
            )
            .unwrap()
        })
        .collect();
    let redactor = SecretRedactor::from_environments(resolved.iter().chain(resolved.iter()));
    assert_eq!(
        redactor.redact("abcdef abc REDACTED é"),
        "[REDACTED] [REDACTED] [REDACTED] [REDACTED]"
    );
    assert_eq!(redactor.redact("xabcdefgh"), "[REDACTED]");
    // Adjacent matches are distinct; only crossing overlaps share a marker.
    assert_eq!(redactor.redact("abcabc"), "[REDACTED][REDACTED]");
    assert_eq!(
        redactor.redact_bytes(b"\xffabcdef\xfeabc"),
        b"\xff[REDACTED]\xfe[REDACTED]"
    );
    assert!(!format!("{redactor:?}").contains("abcdef"));
    assert!(provider.calls.borrow().is_empty());
}

#[test]
fn unified_redaction_includes_fetched_values_and_decoded_literals_without_debug_disclosure() {
    use probe_core::{SecretRedactor, SecretValue};
    let values =
        ["abc", "abcdef", r#""quoted\nsecret""#, "1e3"].map(|value| SecretValue::new(value.into()));
    let empty = SecretRedactor::from_environments([]);
    assert!(empty.is_empty());
    let redactor = empty.with_values(&values).with_json_literals();
    assert!(!redactor.is_empty());
    assert_eq!(
        redactor.redact("abcdef abc quoted\nsecret 1000"),
        "[REDACTED] [REDACTED] [REDACTED] [REDACTED]"
    );
    for sensitive in ["abcdef", "quoted", "1000", "1e3"] {
        assert!(!format!("{redactor:?}").contains(sensitive));
    }
}
