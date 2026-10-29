use probe_core::{EnvironmentVariable, VariableValue, VariableValueSet, VariableValueType};
use serde_json::{Value, json};

use super::{PostmanEnvironmentImportError as ImportError, import_postman_environment};

const EXPORT: &str = include_str!("fixtures/standalone.json");

#[test]
fn preserves_exported_environment_and_secret_metadata() {
    let imported = import_postman_environment(EXPORT).unwrap();
    assert_eq!(imported.environment.name, " Development ");
    let expected = [
        (
            " base_url ",
            VariableValue::String("https://example.test/{{version}}".into()),
            false,
        ),
        ("disabled", VariableValue::String(String::new()), true),
        (
            "token",
            VariableValue::String("private-exported-token".into()),
            true,
        ),
        (
            "number",
            VariableValue::Typed {
                kind: VariableValueType::Number,
                data: "42".into(),
            },
            false,
        ),
        (
            "boolean",
            VariableValue::Typed {
                kind: VariableValueType::Boolean,
                data: "false".into(),
            },
            false,
        ),
        (
            "null",
            VariableValue::Typed {
                kind: VariableValueType::Null,
                data: "null".into(),
            },
            false,
        ),
        (
            "object",
            VariableValue::Typed {
                kind: VariableValueType::Object,
                data: r#"{"nested":"value"}"#.into(),
            },
            false,
        ),
    ];
    assert_eq!(imported.environment.variables.len(), expected.len());
    for (variable, (name, value, disabled)) in imported.environment.variables.iter().zip(expected) {
        let EnvironmentVariable::Plain(variable) = variable else {
            panic!("exported values remain plaintext")
        };
        assert_eq!(variable.name.as_deref(), Some(name));
        assert_eq!(variable.value, Some(VariableValueSet::Single(value)));
        assert_eq!(variable.disabled, disabled);
    }
    assert_eq!(
        imported.variable_types,
        [
            (" base_url ", "default"),
            ("disabled", "text"),
            ("token", "secret"),
            ("number", "number"),
            ("object", "object"),
        ]
        .into_iter()
        .map(|(key, kind)| (key.into(), kind.into()))
        .collect()
    );
    let [diagnostic] = imported.diagnostics.as_slice() else {
        panic!("one secret warning expected")
    };
    assert_eq!(diagnostic.code, "secret_value_plaintext");
    assert!(diagnostic.message.contains("plaintext"));
    assert!(diagnostic.message.contains("not securely stored"));
    let summary = format!("{imported:?}");
    assert!(summary.contains("ImportedPostmanEnvironment"));
    assert!(summary.contains(&format!(
        "variable_count: {}",
        imported.environment.variables.len()
    )));
    assert!(!summary.contains("private-exported-token"));
}

#[test]
fn supports_empty_environment_and_reports_dynamic_values_safely() {
    let empty = json!({"name": "Empty", "values": [], "_postman_variable_scope": "environment"});
    assert!(
        import_postman_environment(&empty.to_string())
            .unwrap()
            .environment
            .variables
            .is_empty()
    );
    let mut source: Value = serde_json::from_str(EXPORT).unwrap();
    source["values"][2]["value"] = json!("sensitive-{{$randomUUID}}");
    let imported = import_postman_environment(&source.to_string()).unwrap();
    assert!(
        imported
            .diagnostics
            .iter()
            .any(|d| d.code == "dynamic_variable_unsupported"
                && d.resource_type == "environment_variable")
    );
    assert!(!format!("{imported:?}").contains("sensitive-"));
}

#[test]
fn rejects_invalid_structures_without_echoing_input() {
    for source in ["{", "[]", r#"{"name":"private-exported-token"}"#] {
        assert_eq!(
            import_postman_environment(source).unwrap_err(),
            ImportError::Invalid
        );
    }
    for (pointer, replacement) in [
        ("/name", json!(" ")),
        ("/values", Value::Null),
        ("/values/0/key", json!("")),
        ("/values/0/key", json!(123)),
        ("/values/0/enabled", json!("private-exported-token")),
        ("/values/0/type", json!({"private-exported-token": true})),
        ("/values/0/description", json!(123)),
    ] {
        let mut source: Value = serde_json::from_str(EXPORT).unwrap();
        *source.pointer_mut(pointer).unwrap() = replacement;
        let error = import_postman_environment(&source.to_string()).unwrap_err();
        assert_eq!(error, ImportError::Invalid);
        assert!(!format!("{error} {error:?}").contains("private-exported-token"));
    }
    for field in ["key", "value"] {
        let mut source: Value = serde_json::from_str(EXPORT).unwrap();
        source["values"][0].as_object_mut().unwrap().remove(field);
        assert_eq!(
            import_postman_environment(&source.to_string()).unwrap_err(),
            ImportError::Invalid
        );
    }
}

#[test]
fn rejects_duplicates_and_unsupported_formats() {
    let mut source: Value = serde_json::from_str(EXPORT).unwrap();
    source["values"][1]["key"] = source["values"][0]["key"].clone();
    assert_eq!(
        import_postman_environment(&source.to_string()).unwrap_err(),
        ImportError::DuplicateVariable
    );
    for (pointer, replacement) in [
        ("/_postman_variable_scope", json!("globals")),
        ("/values/0/type", json!("private-exported-token")),
    ] {
        let mut source: Value = serde_json::from_str(EXPORT).unwrap();
        *source.pointer_mut(pointer).unwrap() = replacement;
        assert_eq!(
            import_postman_environment(&source.to_string()).unwrap_err(),
            ImportError::UnsupportedFormat
        );
    }
    // Alternate value fields and API wrappers must never supply invented values.
    for source in [
        json!({"environment": serde_json::from_str::<Value>(EXPORT).unwrap()}),
        json!({"info": {"name": "Collection"}, "item": []}),
        json!({"name": "Alternate", "_postman_variable_scope": "environment", "values": [{"key": "x", "initialValue": "initial", "currentValue": "current"}]}),
    ] {
        assert!(import_postman_environment(&source.to_string()).is_err());
    }
    let duplicate_field = EXPORT.replace(
        "\"enabled\": false",
        "\"enabled\": false, \"enabled\": true",
    );
    assert_eq!(
        import_postman_environment(&duplicate_field).unwrap_err(),
        ImportError::Invalid
    );
}

#[test]
fn accepts_empty_and_nonempty_variable_descriptions() {
    let baseline = import_postman_environment(EXPORT).unwrap();
    for description in [json!(null), json!(""), json!(" "), json!("Variable notes")] {
        let mut source: Value = serde_json::from_str(EXPORT).unwrap();
        source["values"][0]["description"] = description.clone();
        let imported = import_postman_environment(&source.to_string()).unwrap();
        assert_eq!(imported.environment, baseline.environment);
        assert_eq!(imported.variable_types, baseline.variable_types);
        let warnings = imported
            .diagnostics
            .iter()
            .filter(|d| d.code == "unsupported_description")
            .count();
        assert_eq!(
            warnings,
            usize::from(description == json!("Variable notes"))
        );
    }
}

#[test]
fn description_warning_does_not_disclose_description_or_value() {
    let mut source: Value = serde_json::from_str(EXPORT).unwrap();
    source["values"][2]["description"] = json!("private-description-content");
    let imported = import_postman_environment(&source.to_string()).unwrap();
    let diagnostic = imported
        .diagnostics
        .iter()
        .find(|d| d.code == "unsupported_description")
        .unwrap();
    assert_eq!(diagnostic.resource_type, "environment_variable");
    assert_eq!(diagnostic.resource_id.as_deref(), Some("2"));
    assert_eq!(diagnostic.field.as_deref(), Some("description"));
    assert_eq!(
        diagnostic.severity,
        probe_core::ImportDiagnosticSeverity::Warning
    );
    let report = format!("{:?} {imported:?}", imported.diagnostics);
    for sensitive in ["private-description-content", "private-exported-token"] {
        assert!(!report.contains(sensitive));
    }
}

#[test]
fn errors_explain_the_failure_category() {
    for (error, explanation) in [
        (ImportError::Invalid, "invalid Postman environment"),
        (
            ImportError::UnsupportedFormat,
            "unsupported Postman environment",
        ),
        (ImportError::DuplicateVariable, "duplicate variable names"),
    ] {
        assert!(error.to_string().contains(explanation));
    }
}
