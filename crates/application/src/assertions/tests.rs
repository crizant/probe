use std::cell::Cell;

use probe_core::{SecretContext, SecretError, SecretValue, WorkspaceItemRef};

use super::*;
use crate::NoSecrets;

fn assertion(expression: &str, operator: &str, value: Option<&str>) -> Assertion {
    Assertion {
        expression: expression.into(),
        operator: operator.into(),
        value: value.map(str::to_owned),
        disabled: None,
        description: None,
    }
}

fn response(body: &[u8]) -> HttpResponse {
    HttpResponse {
        status: 200,
        reason: "OK".into(),
        initial_url: "https://example.com".into(),
        url: "https://example.com".into(),
        url_changed: false,
        duration: std::time::Duration::ZERO,
        size: body.len(),
        headers: vec![],
        body: body.into(),
        body_complete: true,
        body_file: None,
        body_retention_error: None,
    }
}

fn outcome(body: &[u8], expression: &str, operator: &str, value: Option<&str>) -> AssertionOutcome {
    evaluate_assertions(
        &[assertion(expression, operator, value)],
        &response(body),
        &RequestResolution::default(),
        &NoSecrets,
    )
    .remove(0)
    .outcome
}

fn fixture() -> probe_opencollection::LoadedWorkspace {
    probe_opencollection::load_workspace_from_str(include_str!(
        "../../../../tests/fixtures/opencollection/assertion-evaluation.yml"
    ))
    .unwrap()
}

#[test]
fn http_and_graphql_use_the_same_api_without_mutating_declarations() {
    let loaded = fixture();
    let workspace = loaded.workspace();
    let resolution = RequestResolution {
        environments: workspace.environments(),
        environment: Some("child"),
        ..RequestResolution::default()
    };
    for (item, body) in workspace.root_items().iter().zip([
        br#"{"users":[{"name":"Ada"}]}"#.as_slice(),
        br#"{"data":{"users":[{"name":"Ada"}]},"errors":null}"#.as_slice(),
    ]) {
        let WorkspaceItemRef::Request(key) = item else {
            panic!("request fixture")
        };
        let request = workspace.request(*key).unwrap();
        let before = request.clone();
        let assertions = request.runtime.assertions.as_ref().unwrap();
        let response = response(body);
        let response_before = response.clone();
        let results = evaluate_assertions(assertions, &response, &resolution, &NoSecrets);
        assert_eq!(results.len(), 3);
        for (index, result) in results.iter().enumerate() {
            assert_eq!(result.index, index);
            assert_eq!(result.outcome, AssertionOutcome::Passed);
        }
        assert_eq!(request, &before);
        assert_eq!(response, response_before);
    }
}

#[test]
fn paths_arrays_lengths_and_real_length_properties() {
    let body = br#"{"users":[{"names":["Ada","Lin"]}],"length":7,"text":"a\uD83D\uDE00"}"#;
    for (path, expected) in [
        ("response.body.users[0].names[1]", "Lin"),
        ("response.body.users[0].names.length", "2"),
        ("response.body.length", "7"),
        ("response.body.text.length", "2"),
    ] {
        assert_eq!(
            outcome(body, path, "equals", Some(expected)),
            AssertionOutcome::Passed
        );
    }
    assert_eq!(
        outcome(b"[[true]]", "response.body[0][0]", "equals", Some("true")),
        AssertionOutcome::Passed
    );
    assert_eq!(
        outcome(b"[]", "response.body.length", "equals", Some("0")),
        AssertionOutcome::Passed
    );
}

#[test]
fn property_names_include_underscore_and_dollar() {
    let body = br#"{"user_name":"Ada","$ref":"users","_id":7}"#;
    for (path, expected) in [
        ("response.body.user_name", "Ada"),
        ("response.body.$ref", "users"),
        ("response.body._id", "7"),
    ] {
        assert_eq!(
            outcome(body, path, "equals", Some(expected)),
            AssertionOutcome::Passed,
            "{path}"
        );
    }
}

#[test]
fn length_applies_only_to_the_final_segment() {
    let body = br#"{"items":["ab","cde"],"text":"abcd","length":{"n":1}}"#;
    assert_eq!(
        outcome(body, "response.body.items.length", "equals", Some("2")),
        AssertionOutcome::Passed
    );
    assert_eq!(
        outcome(body, "response.body.text.length", "equals", Some("4")),
        AssertionOutcome::Passed
    );
    assert_eq!(
        outcome(body, "response.body.length.n", "equals", Some("1")),
        AssertionOutcome::Passed
    );
    for (path, length) in [
        ("response.body.items.length.n", "2"),
        ("response.body.text.length.n", "4"),
    ] {
        assert_eq!(
            outcome(body, path, "equals", Some(length)),
            AssertionOutcome::Error(AssertionError::IncompatibleOperands),
            "{path}"
        );
    }
}

#[test]
fn typed_literals_and_every_operator() {
    for (body, operator, expected, passed) in [
        ("200", "equals", "200.0", true),
        ("200", "equals", "\"200\"", false),
        ("\"200\"", "equals", "\"200\"", true),
        ("true", "equals", "true", true),
        ("false", "equals", "false", true),
        ("null", "equals", "null", true),
        ("\"true\"", "equals", "true", false),
        ("\"null\"", "equals", "\"null\"", true),
        ("\"Ada\"", "equals", "Ada", true),
        ("\"\"", "equals", "", true),
        ("\" a \"", "equals", " a ", true),
        ("\"a\\nb\"", "equals", "\"a\\nb\"", true),
        ("false", "notEquals", "true", true),
        ("true", "notEquals", "true", false),
        ("\"Ada Lovelace\"", "contains", "Lovelace", true),
        ("\"Ada\"", "contains", "Lin", false),
        ("\"Ada\"", "notContains", "Lin", true),
        ("\"Ada\"", "notContains", "Ada", false),
        ("2.5", "greaterThan", "2", true),
        ("2", "greaterThan", "2", false),
        ("-3", "lessThan", "-2.5", true),
        ("2", "lessThan", "2", false),
        ("null", "isNull", "ignored", true),
        ("false", "isNull", "ignored", false),
        ("false", "isNotNull", "ignored", true),
        ("null", "isNotNull", "ignored", false),
    ] {
        assert_eq!(
            outcome(body.as_bytes(), "response.body", operator, Some(expected)),
            if passed {
                AssertionOutcome::Passed
            } else {
                AssertionOutcome::Failed
            },
            "{body} {operator} {expected}"
        );
    }
    assert_eq!(
        outcome(b"", "response.status", "equals", Some("200")),
        AssertionOutcome::Passed
    );
    assert_eq!(
        outcome(b"null", "response.body", "isNull", None),
        AssertionOutcome::Passed
    );
    assert_eq!(
        outcome(b"true", "response.body", "isNotNull", None),
        AssertionOutcome::Passed
    );
}

#[test]
fn numeric_comparisons_do_not_round_large_integers_to_float() {
    for (actual, operator, expected, passed) in [
        ("9007199254740993", "equals", "9007199254740992", false),
        (
            "18446744073709551615",
            "greaterThan",
            "18446744073709551614",
            true,
        ),
        (
            "-9223372036854775808",
            "lessThan",
            "-9223372036854775807",
            true,
        ),
        (
            "9007199254740993",
            "greaterThan",
            "9007199254740992.0",
            true,
        ),
        ("1e3", "equals", "1000", true),
        ("0.01", "lessThan", "0.1", true),
        ("0", "lessThan", "0.01", true),
        ("-0.0", "equals", "0", true),
        ("-0.01", "lessThan", "0", true),
        ("0", "greaterThan", "-1", true),
        // Positive, negative, and fractional exponents, including an integer spelling.
        ("1e19", "equals", "10000000000000000000", true),
        ("1e20", "greaterThan", "1e19", true),
        ("1e-7", "lessThan", "1e-6", true),
        ("-1e20", "lessThan", "-1e19", true),
        ("1e16", "greaterThan", "1e15", true),
    ] {
        assert_eq!(
            outcome(actual.as_bytes(), "response.body", operator, Some(expected)),
            if passed {
                AssertionOutcome::Passed
            } else {
                AssertionOutcome::Failed
            }
        );
    }
}

#[test]
fn invalid_literals_and_incompatible_comparisons_are_explicit_errors() {
    for literal in [
        "'text'",
        "\"unclosed",
        "[1]",
        "{}",
        "01",
        "1e9999",
        "NaN",
        "Infinity",
        "+1",
        "1x",
    ] {
        assert_eq!(
            outcome(b"1", "response.body", "equals", Some(literal)),
            AssertionOutcome::Error(AssertionError::InvalidExpectedValue)
        );
    }
    for (actual, operator, expected) in [
        ("\"2\"", "greaterThan", "1"),
        ("2", "lessThan", "\"3\""),
        ("true", "greaterThan", "1"),
        ("null", "lessThan", "1"),
        ("[1]", "contains", "1"),
        ("\"123\"", "notContains", "2"),
    ] {
        assert_eq!(
            outcome(actual.as_bytes(), "response.body", operator, Some(expected)),
            AssertionOutcome::Error(AssertionError::IncompatibleOperands)
        );
    }
    for operator in [
        "equals",
        "notEquals",
        "contains",
        "notContains",
        "greaterThan",
        "lessThan",
    ] {
        assert_eq!(
            outcome(b"1", "response.body", operator, None),
            AssertionOutcome::Error(AssertionError::MissingExpectedValue)
        );
    }
}

#[test]
fn missing_properties_are_distinct_from_explicit_null_for_all_operators() {
    let body = br#"{"present":null,"array":[]}"#;
    assert_eq!(
        outcome(body, "response.body.present", "isNull", None),
        AssertionOutcome::Passed
    );
    assert_eq!(
        outcome(body, "response.body.present", "isNotNull", None),
        AssertionOutcome::Failed
    );
    for operator in [
        "equals",
        "notEquals",
        "contains",
        "notContains",
        "greaterThan",
        "lessThan",
        "isNull",
        "isNotNull",
    ] {
        for path in ["response.body.missing", "response.body.array[0]"] {
            assert_eq!(
                outcome(body, path, operator, Some("null")),
                AssertionOutcome::Error(AssertionError::MissingProperty)
            );
        }
    }
}

#[test]
fn unsupported_syntax_and_operators_are_never_interpolated_or_executed() {
    for expression in [
        "response",
        "response.statusCode",
        "response.status.length",
        " response.status ",
        "response.bodyx",
        "response.body..name",
        "response.body.",
        "response.body[01]",
        "response.body[-1]",
        "response.body[+1]",
        "response.body[99999999999999999999999999]",
        "response.body[0",
        "response.body['name']",
        "response.body[\"name\"]",
        "response.body[*]",
        "response.body[0:1]",
        "response.body.name()",
        "response.body.name + 1",
        "{{expression}}",
        "process.exit()",
        "response.headers",
        "response.body.0",
    ] {
        assert_eq!(
            outcome(b"null", expression, "equals", Some("null")),
            AssertionOutcome::Error(AssertionError::UnsupportedExpression),
            "{expression}"
        );
    }
    for operator in ["isDefined", "Equals", "", "{{operator}}"] {
        assert_eq!(
            outcome(b"null", "response.body", operator, Some("null")),
            AssertionOutcome::Error(AssertionError::UnsupportedOperator)
        );
    }
}

#[test]
fn body_errors_do_not_prevent_status_or_later_results() {
    let assertions = [
        assertion("response.body.missing", "isNotNull", None),
        assertion("response.status", "equals", Some("200")),
        assertion("response.status", "equals", Some("500")),
        assertion("response.body", "notEquals", Some("null")),
    ];
    for bytes in [
        b"".as_slice(),
        b"not JSON",
        br#"{"partial":"#,
        &[0xff],
        b"{} trailing",
    ] {
        let mut response = response(bytes);
        for complete in [true, false] {
            response.body_complete = complete;
            response.body_retention_error = Some("untrusted diagnostic".into());
            let results = evaluate_assertions(
                &assertions,
                &response,
                &RequestResolution::default(),
                &NoSecrets,
            );
            let error = AssertionOutcome::Error(if complete {
                AssertionError::InvalidJsonBody
            } else {
                AssertionError::BodyUnavailable
            });
            assert_eq!(
                results.iter().map(|r| r.index).collect::<Vec<_>>(),
                [0, 1, 2, 3]
            );
            assert_eq!(
                results.into_iter().map(|r| r.outcome).collect::<Vec<_>>(),
                [
                    error.clone(),
                    AssertionOutcome::Passed,
                    AssertionOutcome::Failed,
                    error
                ]
            );
        }
    }
    let mut response = response(b"null");
    response.body_complete = false; // A valid JSON prefix still cannot prove anything.
    assert_eq!(
        evaluate_assertions(
            &[assertion("response.body", "isNull", None)],
            &response,
            &RequestResolution::default(),
            &NoSecrets
        )[0]
        .outcome,
        AssertionOutcome::Error(AssertionError::BodyUnavailable)
    );
}

#[test]
fn interpolation_uses_inheritance_selection_overrides_and_typed_values() {
    let loaded = fixture();
    let environments = loaded.workspace().environments();
    for (selected, overrides, expected) in [
        (Some("base"), vec![], "201"),
        (Some("child"), vec![], "200"),
        (Some("child"), vec![("status".into(), "202".into())], "202"),
        (None, vec![("status".into(), "203".into())], "203"),
    ] {
        for strict_variables in [true, false] {
            let resolution = RequestResolution {
                environments,
                environment: selected,
                overrides: &overrides,
                strict_variables,
                ..RequestResolution::default()
            };
            let result = evaluate_assertions(
                &[assertion("response.body", "equals", Some("{{status}}"))],
                &response(expected.as_bytes()),
                &resolution,
                &NoSecrets,
            );
            assert_eq!(result[0].outcome, AssertionOutcome::Passed);
        }
    }
    let resolution = RequestResolution {
        environments,
        environment: Some("child"),
        ..RequestResolution::default()
    };
    for (body, value) in [
        ("\"prefix-child-suffix\"", "{{embedded}}"),
        ("\"child-child\"", "{{text}}-{{text}}"),
        ("200", "{{status}}"),
        ("\"200\"", "\"{{status}}\""),
    ] {
        assert_eq!(
            evaluate_assertions(
                &[assertion("response.body", "equals", Some(value))],
                &response(body.as_bytes()),
                &resolution,
                &NoSecrets
            )[0]
            .outcome,
            AssertionOutcome::Passed
        );
    }
    for (literal, body) in [
        ("true", "true"),
        ("false", "false"),
        ("null", "null"),
        ("2.5", "2.5"),
    ] {
        let overrides = [("typed".into(), literal.into())];
        let resolution = RequestResolution {
            overrides: &overrides,
            ..resolution
        };
        assert_eq!(
            evaluate_assertions(
                &[assertion("response.body", "equals", Some("{{typed}}"))],
                &response(body.as_bytes()),
                &resolution,
                &NoSecrets
            )[0]
            .outcome,
            AssertionOutcome::Passed
        );
    }
}

struct Provider<'a> {
    value: Result<Option<&'a str>, SecretError>,
    calls: Cell<usize>,
}

impl SecretProvider for Provider<'_> {
    fn resolve_secret(
        &self,
        context: &SecretContext<'_>,
    ) -> Result<Option<SecretValue>, SecretError> {
        assert_eq!(context.variable_name, "token");
        assert_eq!(context.environment_name, Some("child"));
        assert_eq!(context.workspace_identity, Some("fixture"));
        self.calls.set(self.calls.get() + 1);
        self.value.map(|v| v.map(|v| SecretValue::new(v.into())))
    }
}

#[test]
fn unavailable_disabled_missing_and_malformed_variables_error_independently() {
    let loaded = fixture();
    for value in [Ok(None), Err(SecretError)] {
        let provider = Provider {
            value,
            calls: Cell::new(0),
        };
        for strict_variables in [false, true] {
            let resolution = RequestResolution {
                environments: loaded.workspace().environments(),
                environment: Some("child"),
                workspace_identity: Some("fixture"),
                strict_variables,
                ..RequestResolution::default()
            };
            let assertions = [
                "{{missing}}",
                "{{disabled}}",
                "{{token}}",
                "{{alias}}",
                "{{unfinished",
            ]
            .map(|v| assertion("response.body", "equals", Some(v)));
            let results = evaluate_assertions(
                &assertions,
                &response(b"\"{{missing}}\""),
                &resolution,
                &provider,
            );
            assert!(
                results
                    .iter()
                    .all(|r| r.outcome
                        == AssertionOutcome::Error(AssertionError::VariableResolution))
            );
        }
    }
    assert_eq!(
        outcome(
            br#""{{missing}}""#,
            "response.body",
            "equals",
            Some("{{missing}}")
        ),
        AssertionOutcome::Error(AssertionError::VariableResolution)
    );
    let resolution = RequestResolution {
        environments: loaded.workspace().environments(),
        environment: Some("disabled-secret"),
        ..RequestResolution::default()
    };
    assert_eq!(
        evaluate_assertions(
            &[assertion("response.body", "equals", Some("{{token}}"))],
            &response(b"null"),
            &resolution,
            &NoSecrets
        )[0]
        .outcome,
        AssertionOutcome::Error(AssertionError::VariableResolution)
    );
}

#[test]
fn disabled_and_unary_assertions_do_not_resolve_ignored_values() {
    let provider = Provider {
        value: Err(SecretError),
        calls: Cell::new(0),
    };
    let mut disabled = assertion("unsupported()", "unknown", Some("{{token}}"));
    disabled.disabled = Some(true);
    let assertions = [
        disabled,
        assertion("response.status", "isNotNull", Some("{{token}}")),
        assertion("response.status", "equals", Some("200")),
    ];
    let results = evaluate_assertions(
        &assertions,
        &response(b"invalid JSON"),
        &RequestResolution::default(),
        &provider,
    );
    assert_eq!(
        results.into_iter().map(|r| r.outcome).collect::<Vec<_>>(),
        [
            AssertionOutcome::Disabled,
            AssertionOutcome::Passed,
            AssertionOutcome::Passed
        ]
    );
    assert_eq!(provider.calls.get(), 0);
    assert!(
        evaluate_assertions(
            &[],
            &response(b""),
            &RequestResolution::default(),
            &provider
        )
        .is_empty()
    );
}

#[test]
fn variable_errors_do_not_abort_later_assertions() {
    let loaded = fixture();
    let provider = Provider {
        value: Err(SecretError),
        calls: Cell::new(0),
    };
    let resolution = RequestResolution {
        environments: loaded.workspace().environments(),
        environment: Some("child"),
        workspace_identity: Some("fixture"),
        ..RequestResolution::default()
    };
    let assertions = [
        assertion("response.status", "equals", Some("{{alias}}")),
        assertion("response.status", "equals", Some("{{status}}")),
        assertion("response.status", "equals", Some("201")),
        assertion("response.status", "isNotNull", None),
    ];
    let results = evaluate_assertions(&assertions, &response(b""), &resolution, &provider);
    assert_eq!(
        results.into_iter().map(|r| r.outcome).collect::<Vec<_>>(),
        [
            AssertionOutcome::Error(AssertionError::VariableResolution),
            AssertionOutcome::Passed,
            AssertionOutcome::Failed,
            AssertionOutcome::Passed,
        ]
    );
}

#[test]
fn secrets_and_secret_derived_values_never_enter_results_errors_or_debug() {
    let loaded = fixture();
    let resolution = RequestResolution {
        environments: loaded.workspace().environments(),
        environment: Some("child"),
        workspace_identity: Some("fixture"),
        ..RequestResolution::default()
    };
    for secret in [
        "sensitive-token",
        "200",
        "true",
        "null",
        "\"quoted-secret\"",
        "01",
    ] {
        let provider = Provider {
            value: Ok(Some(secret)),
            calls: Cell::new(0),
        };
        let body = serde_json::to_vec(secret).unwrap();
        let mut assertions = vec![];
        for value in [
            "{{token}}",
            "{{alias}}",
            "prefix-{{alias}}",
            "\"{{token}}\"",
        ] {
            for operator in [
                "equals",
                "notEquals",
                "contains",
                "notContains",
                "greaterThan",
                "lessThan",
            ] {
                assertions.push(assertion("response.body", operator, Some(value)));
            }
        }
        // Untrusted declaration metadata may itself contain secret text.
        assertions.push(assertion(secret, "equals", Some(secret)));
        assertions.push(assertion("response.body", secret, Some(secret)));
        let results = evaluate_assertions(&assertions, &response(&body), &resolution, &provider);
        assert!(provider.calls.get() > 0);
        let debug = format!("{results:?}");
        assert!(!debug.contains(secret));
        for result in results {
            if let AssertionOutcome::Error(error) = result.outcome {
                assert!(!error.to_string().contains(secret));
                assert!(!format!("{error:?}").contains(secret));
                assert!(std::error::Error::source(&error).is_none());
            }
        }
    }
    let provider = Provider {
        value: Ok(Some("sensitive-token")),
        calls: Cell::new(0),
    };
    assert_eq!(
        evaluate_assertions(
            &[assertion("response.body", "equals", Some("{{alias}}"))],
            &response(br#""sensitive-token""#),
            &resolution,
            &provider
        )[0]
        .outcome,
        AssertionOutcome::Passed
    );
    // Overrides of declared secrets remain private and do not consult the provider.
    let overrides = [("token".into(), "replacement-secret".into())];
    let before = provider.calls.get();
    let results = evaluate_assertions(
        &[assertion("response.body", "equals", Some("{{alias}}"))],
        &response(br#""replacement-secret""#),
        &RequestResolution {
            overrides: &overrides,
            ..resolution
        },
        &provider,
    );
    assert_eq!(results[0].outcome, AssertionOutcome::Passed);
    assert!(!format!("{results:?}").contains("replacement-secret"));
    assert_eq!(provider.calls.get(), before);
}

#[test]
fn every_diagnostic_path_withholds_untrusted_inputs() {
    const SECRET: &str = "a-private-value-that-must-never-be-displayed";
    let loaded = fixture();
    let resolution = RequestResolution {
        environments: loaded.workspace().environments(),
        environment: Some("child"),
        workspace_identity: Some("fixture"),
        ..RequestResolution::default()
    };
    let provider = Provider {
        value: Ok(Some(SECRET)),
        calls: Cell::new(0),
    };
    let cases = [
        (
            assertion(SECRET, "equals", Some("{{token}}")),
            b"null".as_slice(),
            true,
            AssertionError::UnsupportedExpression,
            "unsupported assertion expression",
        ),
        (
            assertion("response.status", SECRET, Some("{{token}}")),
            b"null".as_slice(),
            true,
            AssertionError::UnsupportedOperator,
            "unsupported assertion operator",
        ),
        (
            assertion("response.status", "equals", None),
            b"null".as_slice(),
            true,
            AssertionError::MissingExpectedValue,
            "operator requires an expected value",
        ),
        (
            assertion("response.status", "equals", Some("{{missing}}")),
            b"null".as_slice(),
            true,
            AssertionError::VariableResolution,
            "assertion variable resolution failed or a value is unavailable",
        ),
        (
            assertion("response.status", "equals", Some("\"{{token}}")),
            b"null".as_slice(),
            true,
            AssertionError::InvalidExpectedValue,
            "invalid or unsupported expected literal",
        ),
        (
            assertion("response.body", "isNotNull", None),
            SECRET.as_bytes(),
            false,
            AssertionError::BodyUnavailable,
            "complete in-memory response body is unavailable",
        ),
        (
            assertion("response.body", "isNotNull", None),
            SECRET.as_bytes(),
            true,
            AssertionError::InvalidJsonBody,
            "response body is not valid JSON",
        ),
        (
            assertion("response.body.absent", "isNull", None),
            b"{}".as_slice(),
            true,
            AssertionError::MissingProperty,
            "response property or array index does not exist",
        ),
        (
            assertion("response.status", "greaterThan", Some("{{token}}")),
            b"null".as_slice(),
            true,
            AssertionError::IncompatibleOperands,
            "operator does not support these operand types",
        ),
    ];
    for (mut assertion, body, complete, error, message) in cases {
        assertion.description = Some(probe_core::Documentation::Text(SECRET.into()));
        let mut response = response(body);
        response.body_complete = complete;
        response.body_retention_error = Some(SECRET.into());
        let results = evaluate_assertions(&[assertion], &response, &resolution, &provider);
        assert_eq!(results[0].outcome, AssertionOutcome::Error(error));
        assert_eq!(error.to_string(), message);
        assert!(!message.contains(SECRET));
        assert!(!format!("{results:?}").contains(SECRET));
        assert!(!error.to_string().contains(SECRET));
        assert!(!format!("{error:?}").contains(SECRET));
        assert!(std::error::Error::source(&error).is_none());
    }
}
