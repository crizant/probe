use std::{cell::RefCell, collections::BTreeMap};

use probe_core::{
    Environment, EnvironmentVariable, Request, SecretContext, SecretError, SecretProvider,
    SecretValue, SecretVariable, Variable, VariableValue, VariableValueSet,
    resolve_environment_for_request_with_provider, resolve_request,
    resolve_request_for_presentation,
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
        dot_env_file_path: None,
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
