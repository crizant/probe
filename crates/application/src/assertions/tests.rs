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
        assert_eq!(results.len(), 5);
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
fn negative_indexes_select_from_the_end_and_preserve_length_properties() {
    let body = br#"{"items":[{"name":"Ada","length":7},{"name":"Lin","length":9},{"name":"Jo"}],"nested":[[1,2],[3,4]]}"#;
    for (path, expected) in [
        ("response.body.items[0].name", "Ada"),
        ("response.body.items[-1].name", "Jo"),
        ("response.body.items[-2].name", "Lin"),
        ("response.body.items[-3].name", "Ada"),
        ("response.body.items[0].length", "7"),
        ("response.body.items[1].length", "9"),
        ("response.body.items[-2].length", "9"),
        ("response.body.nested[-1][-2]", "3"),
        ("response.body.nested[0][-1]", "2"),
        ("response.body.nested[-1][0]", "3"),
    ] {
        assert_eq!(
            outcome(body, path, "equals", Some(expected)),
            AssertionOutcome::Passed,
            "{path}"
        );
    }
}

#[test]
fn index_bounds_and_incompatible_types_keep_their_error_categories() {
    for body in [b"[]".as_slice(), b"[1,2,3]"] {
        for index in [
            "3".into(),
            "-4".into(),
            usize::MAX.to_string(),
            format!("-{}", usize::MAX),
        ] {
            assert_eq!(
                outcome(body, &format!("response.body[{index}]"), "isNotNull", None),
                AssertionOutcome::Error(AssertionError::MissingProperty),
                "{index}"
            );
        }
    }
    assert_eq!(
        outcome(b"[]", "response.body[-1]", "isNull", None),
        AssertionOutcome::Error(AssertionError::MissingProperty)
    );
    for body in [r#"{"-1":"value"}"#, "null", "true", "42", r#""text""#] {
        assert_eq!(
            outcome(body.as_bytes(), "response.body[-1]", "isNotNull", None),
            AssertionOutcome::Error(AssertionError::IncompatibleOperands)
        );
    }
}

#[test]
fn explicit_length_counts_arrays_and_unicode_scalars_but_never_object_properties() {
    for (body, expected) in [
        ("[]", "0"),
        ("[1,2]", "2"),
        (r#""""#, "0"),
        (r#""a😀é""#, "4"),
    ] {
        for suffix in [".length", ".length()"] {
            assert_eq!(
                outcome(
                    body.as_bytes(),
                    &format!("response.body{suffix}"),
                    "equals",
                    Some(expected)
                ),
                AssertionOutcome::Passed
            );
        }
    }
    assert_eq!(
        outcome(
            br#"{"users":[{"name":"Ada"}]}"#,
            "response.body.users[-1].name.length()",
            "equals",
            Some("3")
        ),
        AssertionOutcome::Passed
    );
    for body in [r#"{"length":42}"#, "{}", "null", "true", "42"] {
        assert_eq!(
            outcome(body.as_bytes(), "response.body.length()", "isNotNull", None),
            AssertionOutcome::Error(AssertionError::IncompatibleOperands)
        );
    }
    assert_eq!(
        outcome(
            br#"[{"length":42}]"#,
            "response.body[0].length()",
            "equals",
            Some("42")
        ),
        AssertionOutcome::Error(AssertionError::IncompatibleOperands)
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
    // One supplementary-plane scalar, not its UTF-16 or UTF-8 length.
    assert_eq!(
        outcome(
            "\"😀\"".as_bytes(),
            "response.body.length",
            "equals",
            Some("1")
        ),
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
        "1e9999",
        "1e400",
        "-1e400",
        "1e+400",
        "1E400",
        "1.5e400",
        "10e400",
        "0.1e400",
        "9e400",
        "NaN",
        "Infinity",
        "-Infinity",
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
        ("200", "contains", "20"),
        ("200", "notContains", "20"),
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
fn unquoted_non_numbers_remain_strings_and_contains_uses_their_text() {
    for (body, expected, passed) in [
        (r#""2024-01-01""#, "2024-01-01", true),
        (r#""1.0.0""#, "1.0.0", true),
        (r#""200 OK""#, "200 OK", true),
        (r#""+200""#, "+200", true),
        (r#""01""#, "01", true),
        (r#""1.""#, "1.", true),
        (r#""-""#, "-", true),
        (r#"".5""#, ".5", true),
        ("200", "+200", false),
        ("200", "200 OK", false),
        ("1", "01", false),
        ("1", "+1", false),
        ("1", "1x", false),
    ] {
        assert_eq!(
            outcome(body.as_bytes(), "response.body", "equals", Some(expected)),
            if passed {
                AssertionOutcome::Passed
            } else {
                AssertionOutcome::Failed
            },
            "{body} equals {expected}"
        );
    }
    assert_eq!(
        outcome(
            br#""2024-01-01""#,
            "response.body",
            "contains",
            Some("2024")
        ),
        AssertionOutcome::Passed
    );
    assert_eq!(
        outcome(br#""x1e3y""#, "response.body", "contains", Some("1e3")),
        AssertionOutcome::Passed
    );
    assert_eq!(
        outcome(br#""1000""#, "response.body", "contains", Some("1e3")),
        AssertionOutcome::Failed
    );
    assert_eq!(
        outcome(br#""123""#, "response.body", "notContains", Some("2")),
        AssertionOutcome::Failed
    );
    assert_eq!(
        outcome(br#""123""#, "response.body", "notContains", Some("9")),
        AssertionOutcome::Passed
    );
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
        "response.body[-0]",
        "response.body[-01]",
        "response.body[--1]",
        "response.body[-]",
        "response.body[]",
        "response.body[ 1]",
        "response.body[-1 ]",
        "response.body[+1]",
        "response.body[99999999999999999999999999]",
        "response.body[-99999999999999999999999999]",
        "response.body.length().foo",
        "response.body.length()[0]",
        "response.body.length().length()",
        "response.body.length(1)",
        "response.body.length( )",
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
    for (expression, operator, expected_outcome) in [
        ("response.status", "isNull", AssertionOutcome::Failed),
        ("response.body", "isNotNull", AssertionOutcome::Failed),
        (
            "response.body.missing",
            "isNull",
            AssertionOutcome::Error(AssertionError::IncompatibleOperands),
        ),
        (
            "response.body.missing",
            "isNotNull",
            AssertionOutcome::Error(AssertionError::IncompatibleOperands),
        ),
    ] {
        for ignored_value in ["200", "private-ignored-literal", "{{token}}"] {
            let result = evaluate_assertions(
                &[assertion(expression, operator, Some(ignored_value))],
                &response(b"null"),
                &RequestResolution::default(),
                &provider,
            )
            .remove(0);
            assert_eq!(result.outcome, expected_outcome);
            assert_eq!(result.diagnostic, Some(format!("{expression} {operator}")));
        }
    }
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
fn values_without_templates_skip_environment_resolution() {
    let loaded = fixture();
    let provider = Provider {
        value: Err(SecretError),
        calls: Cell::new(0),
    };
    let missing = RequestResolution {
        environments: loaded.workspace().environments(),
        environment: Some("does-not-exist"),
        workspace_identity: Some("fixture"),
        ..RequestResolution::default()
    };
    let results = evaluate_assertions(
        &[
            assertion("response.status", "equals", Some("200")),
            assertion("response.status", "equals", Some("{{status}}")),
            assertion("response.body", "contains", Some("200")),
        ],
        &response(br#""200""#),
        &missing,
        &provider,
    );
    assert_eq!(
        results
            .into_iter()
            .map(|result| result.outcome)
            .collect::<Vec<_>>(),
        [
            AssertionOutcome::Passed,
            AssertionOutcome::Error(AssertionError::VariableResolution),
            AssertionOutcome::Passed,
        ]
    );
    assert_eq!(provider.calls.get(), 0);

    let child = RequestResolution {
        environments: loaded.workspace().environments(),
        environment: Some("child"),
        workspace_identity: Some("fixture"),
        ..RequestResolution::default()
    };
    let results = evaluate_assertions(
        &[
            assertion("response.status", "equals", Some("200")),
            assertion("response.status", "equals", Some("{{token}}")),
        ],
        &response(b""),
        &child,
        &provider,
    );
    assert_eq!(results[0].outcome, AssertionOutcome::Passed);
    assert_eq!(
        results[1].outcome,
        AssertionOutcome::Error(AssertionError::VariableResolution)
    );
    assert_eq!(provider.calls.get(), 1);
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
            "The complete response body is unavailable for this assertion.",
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

#[test]
fn batch_caches_provider_success_missing_values_and_failures() {
    let loaded = fixture();
    let resolution = RequestResolution {
        environments: loaded.workspace().environments(),
        environment: Some("child"),
        workspace_identity: Some("fixture"),
        ..RequestResolution::default()
    };
    for value in [Ok(Some("secret")), Ok(None), Err(SecretError)] {
        let provider = Provider {
            value,
            calls: Cell::new(0),
        };
        let results = evaluate_assertions(
            &[
                assertion("response.body", "equals", Some("{{token}}")),
                assertion("response.body", "equals", Some("{{alias}}")),
            ],
            &response(br#""secret""#),
            &resolution,
            &provider,
        );
        assert_eq!(provider.calls.get(), 1);
        let expected = if matches!(value, Ok(Some(_))) {
            AssertionOutcome::Passed
        } else {
            AssertionOutcome::Error(AssertionError::VariableResolution)
        };
        assert!(results.iter().all(|r| r.outcome == expected));
    }
}

#[test]
fn storage_preflight_distinguishes_plain_ignored_and_reachable_secret_values() {
    let loaded = fixture();
    let environments = loaded.workspace().environments();
    let resolution = RequestResolution {
        environments,
        environment: Some("child"),
        ..RequestResolution::default()
    };
    assert!(!may_resolve_secrets(&[], &resolution));
    for value in ["200", "{{status}}", "{{embedded}}"] {
        assert!(
            !may_resolve_secrets(
                &[assertion("response.status", "equals", Some(value))],
                &resolution
            ),
            "plain value: {value}"
        );
    }
    for value in ["{{token}}", "{{alias}}", "{{missing}}", "{{malformed"] {
        assert!(
            may_resolve_secrets(
                &[assertion("response.status", "equals", Some(value))],
                &resolution
            ),
            "secret or uncertain value: {value}"
        );
    }
    let mut disabled = assertion("response.status", "equals", Some("{{token}}"));
    disabled.disabled = Some(true);
    assert!(!may_resolve_secrets(&[disabled], &resolution));
    for operator in ["isNull", "isNotNull"] {
        assert!(!may_resolve_secrets(
            &[assertion("response.body", operator, Some("{{token}}"))],
            &resolution
        ));
    }
    let plain_override = [("alias".into(), "200".into())];
    assert!(!may_resolve_secrets(
        &[assertion("response.status", "equals", Some("{{alias}}"))],
        &RequestResolution {
            overrides: &plain_override,
            ..resolution
        }
    ));
    let secret_override = [("token".into(), "runtime-secret".into())];
    assert!(may_resolve_secrets(
        &[assertion("response.status", "equals", Some("{{token}}"))],
        &RequestResolution {
            overrides: &secret_override,
            ..resolution
        }
    ));
    assert!(may_resolve_secrets(
        &[assertion("response.status", "equals", Some("{{status}}"))],
        &RequestResolution {
            environment: Some("missing-environment"),
            ..resolution
        }
    ));
}

#[test]
fn batched_storage_preflight_blocks_missing_and_secret_values_in_any_position() {
    let loaded = fixture();
    let resolution = RequestResolution {
        environments: loaded.workspace().environments(),
        environment: Some("child"),
        ..RequestResolution::default()
    };
    for unsafe_value in [
        "{{missing}}",
        "{{disabled}}",
        "{{token}}",
        "{{alias}}",
        "{{malformed",
    ] {
        for values in [
            ["{{status}}", "{{embedded}}", unsafe_value],
            [unsafe_value, "{{status}}", "{{embedded}}"],
        ] {
            let assertions: Vec<_> = values
                .into_iter()
                .map(|value| assertion("response.status", "equals", Some(value)))
                .collect();
            assert!(may_resolve_secrets(&assertions, &resolution), "{values:?}");
        }
    }
}

#[test]
fn batched_storage_preflight_uses_inheritance_and_invocation_overrides() {
    let loaded = fixture();
    let assertions = [
        assertion("response.status", "equals", Some("{{status}}")),
        assertion("response.body.text", "equals", Some("{{text}}")),
        assertion("response.body.token", "equals", Some("{{alias}}")),
    ];
    let plain = [("alias".into(), "{{text}}".into())];
    let missing = [("alias".into(), "{{missing}}".into())];
    let secret = [("alias".into(), "{{token}}".into())];
    for environment in ["base", "child", "disabled-secret"] {
        for (overrides, blocked) in [
            (plain.as_slice(), false),
            (missing.as_slice(), true),
            (secret.as_slice(), true),
            (&[][..], true),
        ] {
            let resolution = RequestResolution {
                environments: loaded.workspace().environments(),
                environment: Some(environment),
                overrides,
                ..RequestResolution::default()
            };
            assert_eq!(
                may_resolve_secrets(&assertions, &resolution),
                blocked,
                "{environment}"
            );
        }
    }
}

#[test]
fn batched_storage_preflight_ignores_disabled_and_unary_values() {
    let loaded = fixture();
    for value in ["{{token}}", "{{missing}}", "{{malformed"] {
        let mut disabled = assertion("response.status", "equals", Some(value));
        disabled.disabled = Some(true);
        let mut assertions = vec![
            disabled,
            assertion("response.body", "isNull", Some(value)),
            assertion("response.body", "isNotNull", Some(value)),
        ];
        // Ignored values must not even trigger environment resolution.
        assert!(!may_resolve_secrets(
            &assertions,
            &RequestResolution {
                environments: loaded.workspace().environments(),
                environment: Some("missing-environment"),
                ..RequestResolution::default()
            }
        ));
        assertions.push(assertion("response.status", "equals", Some("{{status}}")));
        assert!(!may_resolve_secrets(
            &assertions,
            &RequestResolution {
                environments: loaded.workspace().environments(),
                environment: Some("child"),
                ..RequestResolution::default()
            }
        ));
    }
}

#[test]
fn observed_values_are_bounded_after_redaction_and_errors_are_not_failures() {
    let body = serde_json::json!({"name":"Ada", "items":[1,true,null], "long":"é".repeat(MAX_ASSERTION_ACTUAL_CHARS+1)});
    let declarations = [
        assertion("response.status", "equals", Some("200")),
        assertion("response.body.name", "equals", Some("Grace")),
        assertion("response.body.items[1]", "equals", Some("true")),
        assertion("response.body.items[2]", "isNull", None),
        assertion("response.body.missing", "equals", Some("null")),
        assertion("invalid", "equals", Some("200")),
        assertion("response.body.long", "equals", Some("short")),
        Assertion {
            disabled: Some(true),
            ..assertion("invalid", "invalid", None)
        },
    ];
    let results = evaluate_assertions(
        &declarations,
        &response(&serde_json::to_vec(&body).unwrap()),
        &RequestResolution::default(),
        &NoSecrets,
    );
    assert_eq!(results[0].actual.as_deref(), Some("200"));
    assert_eq!(results[0].outcome, AssertionOutcome::Passed);
    assert_eq!(results[1].actual.as_deref(), Some("Ada"));
    assert_eq!(results[1].outcome, AssertionOutcome::Failed);
    assert_eq!(results[2].actual.as_deref(), Some("true"));
    assert_eq!(results[3].actual.as_deref(), Some("null"));
    assert_eq!(
        results[4].outcome,
        AssertionOutcome::Error(AssertionError::MissingProperty)
    );
    assert_eq!(
        results[5].outcome,
        AssertionOutcome::Error(AssertionError::UnsupportedExpression)
    );
    assert!(results[4].actual.is_none());
    assert!(results[5].actual.is_none());
    assert_eq!(
        results[6].actual.as_deref(),
        Some(format!("{}… [truncated]", "é".repeat(MAX_ASSERTION_ACTUAL_CHARS)).as_str())
    );
    assert!(results[6].actual_truncated);
    assert_eq!(results[7].outcome, AssertionOutcome::Disabled);
    assert!(results[7].actual.is_none());
    for (bytes, complete, error) in [
        (&b"not json"[..], true, AssertionError::InvalidJsonBody),
        (&b"{}"[..], false, AssertionError::BodyUnavailable),
    ] {
        let mut response = response(bytes);
        response.body_complete = complete;
        let result = evaluate_assertions(
            &[assertion("response.body", "isNull", None)],
            &response,
            &RequestResolution::default(),
            &NoSecrets,
        )
        .remove(0);
        assert_eq!(result.outcome, AssertionOutcome::Error(error));
        assert!(result.actual.is_none());
    }
}

#[test]
fn actuals_use_secrets_disclosed_later_and_redact_decoded_json_before_escaping() {
    let loaded = fixture();
    let resolution = RequestResolution {
        environments: loaded.workspace().environments(),
        environment: Some("child"),
        workspace_identity: Some("fixture"),
        ..RequestResolution::default()
    };
    for secret in ["sensitive-token", "quote\"and\nnewline", "é-secret"] {
        let provider = Provider {
            value: Ok(Some(secret)),
            calls: Cell::new(0),
        };
        let body = serde_json::json!({"echo":secret,"nested": {(secret): [secret]}});
        let declarations = [
            assertion("response.body.echo", "equals", Some("different")),
            assertion("response.body.nested", "isNotNull", None),
            assertion("response.body.echo", "equals", Some("{{token}}")),
        ];
        let results = evaluate_assertions(
            &declarations,
            &response(&serde_json::to_vec(&body).unwrap()),
            &resolution,
            &provider,
        );
        assert_eq!(results[0].actual.as_deref(), Some("[REDACTED]"));
        assert!(!format!("{results:?}").contains(secret));
        assert!(!results[1].actual.as_ref().unwrap().contains(secret));
        assert!(results[1].actual.as_ref().unwrap().contains("[REDACTED]"));
    }
    // A long secret straddling the display limit is removed before truncation.
    let secret = "x".repeat(MAX_ASSERTION_ACTUAL_CHARS + 100);
    let provider = Provider {
        value: Ok(Some(&secret)),
        calls: Cell::new(0),
    };
    let results = evaluate_assertions(
        &[assertion("response.body", "equals", Some("{{token}}"))],
        &response(&serde_json::to_vec(&secret).unwrap()),
        &resolution,
        &provider,
    );
    assert_eq!(results[0].actual.as_deref(), Some("[REDACTED]"));
    assert!(!results[0].actual_truncated);
}

#[test]
fn new_selector_syntax_shares_redaction_and_diagnostics() {
    let loaded = fixture();
    let resolution = RequestResolution {
        environments: loaded.workspace().environments(),
        environment: Some("child"),
        workspace_identity: Some("fixture"),
        ..RequestResolution::default()
    };
    let secret = "sensitive-token";
    let provider = Provider {
        value: Ok(Some(secret)),
        calls: Cell::new(0),
    };
    let declarations = [
        assertion("response.body[-1].name", "equals", Some(secret)),
        assertion("response.body[-1].name.length()", "equals", Some("0")),
        assertion("response.body[-1].name", "equals", Some("{{token}}")),
        assertion("response.body[-1].length()", "equals", Some("0")),
        assertion("response.body.length().foo", "equals", Some("0")),
    ];
    let results = evaluate_assertions(
        &declarations,
        &response(br#"[{"name":"sensitive-token-other"}]"#),
        &resolution,
        &provider,
    );
    assert_eq!(results[0].outcome, AssertionOutcome::Failed);
    assert_eq!(results[0].actual.as_deref(), Some("[REDACTED]-other"));
    assert_eq!(
        results[0].diagnostic.as_deref(),
        Some("response.body[-1].name equals [REDACTED]")
    );
    assert_eq!(results[1].outcome, AssertionOutcome::Failed);
    assert_eq!(results[1].actual.as_deref(), Some("21"));
    assert_eq!(
        results[1].diagnostic.as_deref(),
        Some("response.body[-1].name.length() equals 0")
    );
    assert_eq!(
        results[3].outcome,
        AssertionOutcome::Error(AssertionError::IncompatibleOperands)
    );
    assert_eq!(
        results[3].diagnostic.as_deref(),
        Some("response.body[-1].length() equals 0")
    );
    assert_eq!(
        results[4].diagnostic.as_deref(),
        Some("[REDACTED] equals 0")
    );
    assert!(!format!("{results:?}").contains(secret));
}

#[test]
fn partially_failed_resolution_still_redacts_every_fetched_secret() {
    let loaded = fixture();
    let resolution = RequestResolution {
        environments: loaded.workspace().environments(),
        environment: Some("child"),
        workspace_identity: Some("fixture"),
        ..RequestResolution::default()
    };
    let provider = Provider {
        value: Ok(Some("secret-before-error")),
        calls: Cell::new(0),
    };
    let results = evaluate_assertions(
        &[
            assertion("response.body", "equals", Some("different")),
            assertion("response.body", "equals", Some("{{token}}-{{missing}}")),
        ],
        &response(br#""secret-before-error""#),
        &resolution,
        &provider,
    );
    assert_eq!(results[0].actual.as_deref(), Some("[REDACTED]"));
    assert_eq!(
        results[1].outcome,
        AssertionOutcome::Error(AssertionError::VariableResolution)
    );
    assert_eq!(provider.calls.get(), 1);
}

#[test]
fn actuals_redact_literal_decoding_and_secret_derived_numbers() {
    let loaded = fixture();
    let resolution = RequestResolution {
        environments: loaded.workspace().environments(),
        environment: Some("child"),
        workspace_identity: Some("fixture"),
        ..RequestResolution::default()
    };
    for (secret, expected, body) in [
        (
            "\"decoded-secret\"",
            "{{token}}",
            br#""decoded-secret""#.as_slice(),
        ),
        ("1e3", "{{token}}", b"1e3".as_slice()),
        ("3", "1e{{token}}", b"1000".as_slice()),
    ] {
        let provider = Provider {
            value: Ok(Some(secret)),
            calls: Cell::new(0),
        };
        let results = evaluate_assertions(
            &[assertion("response.body", "equals", Some(expected))],
            &response(body),
            &resolution,
            &provider,
        );
        assert_eq!(results[0].outcome, AssertionOutcome::Passed);
        assert_eq!(results[0].actual.as_deref(), Some("[REDACTED]"));
    }
}

#[test]
fn structured_actuals_redact_whole_values_when_serialized_secrets_match() {
    let secrets = [SecretValue::new("container-secret".into())];
    let redactor = probe_core::SecretRedactor::from_environments([]).with_values(&secrets);
    for body in [
        serde_json::json!({"echo": "container-secret"}),
        serde_json::json!(["container-secret"]),
    ] {
        let result = PendingAssertionResult {
            index: 0,
            outcome: AssertionOutcome::Passed,
            actual: Some(ObservedActual::Body {
                root: Arc::new(body),
                expression: "response.body".into(),
            }),
        }
        .redact(
            &redactor,
            &[assertion("response.body", "equals", Some("null"))],
        );
        assert_eq!(result.actual.as_deref(), Some("[REDACTED]"));
        assert!(!result.actual_truncated);
    }
}

#[test]
fn structured_actuals_redact_decoded_strings_and_arrays_preserving_json_types() {
    // JSON escaping prevents the raw matcher from seeing this secret in the
    // serialized container. Its decoded strings must still be redacted.
    let secret = "quoted\"secret\nline";
    let secrets = [SecretValue::new(secret.into())];
    let redactor = probe_core::SecretRedactor::from_environments([]).with_values(&secrets);
    let body = serde_json::json!({
        "text": secret,
        "nested": [secret, {"echo": secret}, [secret, true, false, 42, 2.5, null]],
        "ordinary": "safe text"
    });
    let result = PendingAssertionResult {
        index: 0,
        outcome: AssertionOutcome::Passed,
        actual: Some(ObservedActual::Body {
            root: Arc::new(body),
            expression: "response.body".into(),
        }),
    }
    .redact(
        &redactor,
        &[assertion("response.body", "equals", Some("null"))],
    );
    let actual: Value = serde_json::from_str(result.actual.as_deref().unwrap()).unwrap();
    assert_eq!(
        actual,
        serde_json::json!({
            "text": "[REDACTED]",
            "nested": ["[REDACTED]", {"echo": "[REDACTED]"},
                ["[REDACTED]", true, false, 42, 2.5, null]],
            "ordinary": "safe text"
        })
    );
    assert!(!result.actual_truncated);
}

#[test]
fn diagnostic_literals_fail_closed_when_their_encoding_cannot_be_checked() {
    let secrets = [SecretValue::new("private".into())];
    let redactor = probe_core::SecretRedactor::from_environments([]).with_values(&secrets);
    for (value, expected) in [
        (r#"["\u0070rivate"]"#, "[REDACTED]"),
        (r#""\u0070rivate"#, "[REDACTED]"),
        (r#""\u0070rivate{{alias}}"#, "[REDACTED]"),
        ("{{alias}}", "{{alias}}"),
        (" 2e2 ", " 2e2 "),
        (r#""safe value""#, r#""safe value""#),
    ] {
        let declarations = [assertion("response.status", "equals", Some(value))];
        let result = PendingAssertionResult {
            index: 0,
            outcome: AssertionOutcome::Error(AssertionError::InvalidExpectedValue),
            actual: None,
        }
        .redact(&redactor, &declarations);
        assert_eq!(
            result.diagnostic,
            Some(format!("response.status equals {expected}"))
        );
    }
}

#[test]
fn diagnostics_are_bounded_by_unicode_characters_after_redaction() {
    for expression in ["response.body", "response.body.missing"] {
        let prefix = format!("{expression} equals ");
        for length in [
            MAX_ASSERTION_DIAGNOSTIC_CHARS - 1,
            MAX_ASSERTION_DIAGNOSTIC_CHARS,
            MAX_ASSERTION_DIAGNOSTIC_CHARS + 1,
            MAX_ASSERTION_DIAGNOSTIC_CHARS * 2,
        ] {
            let value = "é".repeat(length - prefix.chars().count());
            let result = evaluate_assertions(
                &[assertion(expression, "equals", Some(&value))],
                &response(b"0"),
                &RequestResolution::default(),
                &NoSecrets,
            )
            .remove(0);
            let original = format!("{prefix}{value}");
            let expected = if length > MAX_ASSERTION_DIAGNOSTIC_CHARS {
                format!(
                    "{}… [truncated]",
                    original
                        .chars()
                        .take(MAX_ASSERTION_DIAGNOSTIC_CHARS)
                        .collect::<String>()
                )
            } else {
                original
            };
            assert_eq!(result.diagnostic, Some(expected));
        }
    }
    // A secret longer than the limit must be redacted before any prefix is retained.
    let secret = "s".repeat(MAX_ASSERTION_DIAGNOSTIC_CHARS * 2);
    let secrets = [SecretValue::new(secret.clone())];
    let redactor = probe_core::SecretRedactor::from_environments([]).with_values(&secrets);
    let result = PendingAssertionResult {
        index: 0,
        outcome: AssertionOutcome::Failed,
        actual: None,
    }
    .redact(
        &redactor,
        &[assertion("response.body", "equals", Some(&secret))],
    );
    assert_eq!(
        result.diagnostic.as_deref(),
        Some("response.body equals [REDACTED]")
    );
}
