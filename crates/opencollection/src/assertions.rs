//! Projection of OpenCollection runtime assertion declarations, without evaluation.

use probe_core::{Assertion, RequestRuntime};
use serde_yaml_ng::{Mapping, Value};

use crate::document::{optional_documentation, yaml_error};

pub(crate) fn request_runtime(
    value: Option<&Value>,
) -> Result<RequestRuntime, serde_yaml_ng::Error> {
    let Some(value) = value else {
        return Ok(RequestRuntime::default());
    };
    let runtime = value
        .as_mapping()
        .ok_or_else(|| yaml_error("runtime must be an object"))?;
    let assertions = runtime
        .get("assertions")
        .map(|value| {
            let entries = value
                .as_sequence()
                .ok_or_else(|| yaml_error("runtime.assertions must be an array"))?;
            entries
                .iter()
                .enumerate()
                .map(|(index, entry)| {
                    assertion(entry).map_err(|error| {
                        yaml_error(&format!("runtime.assertions[{index}]: {error}"))
                    })
                })
                .collect()
        })
        .transpose()?;
    Ok(RequestRuntime { assertions })
}

fn assertion(value: &Value) -> Result<Assertion, serde_yaml_ng::Error> {
    let mapping = value
        .as_mapping()
        .ok_or_else(|| yaml_error("assertion must be an object"))?;
    if mapping.keys().any(|key| {
        !matches!(
            key.as_str(),
            Some("expression" | "operator" | "value" | "disabled" | "description")
        )
    }) {
        return Err(yaml_error("assertion contains an unknown field"));
    }
    let expression = optional_string(mapping, "expression")?
        .ok_or_else(|| yaml_error("assertion.expression is required"))?;
    let operator = optional_string(mapping, "operator")?
        .ok_or_else(|| yaml_error("assertion.operator is required"))?;
    let disabled = mapping
        .get("disabled")
        .map(|value| {
            value
                .as_bool()
                .ok_or_else(|| yaml_error("assertion.disabled must be a boolean"))
        })
        .transpose()?;
    Ok(Assertion {
        expression,
        operator,
        value: optional_string(mapping, "value")?,
        disabled,
        description: optional_documentation(mapping.get("description"))?,
    })
}

fn optional_string(mapping: &Mapping, key: &str) -> Result<Option<String>, serde_yaml_ng::Error> {
    mapping
        .get(key)
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| yaml_error(&format!("assertion.{key} must be a string")))
        })
        .transpose()
}
