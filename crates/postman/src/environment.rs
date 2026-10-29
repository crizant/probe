//! In-memory conversion of standalone environment exports (not API responses).

use std::{collections::BTreeMap, error::Error, fmt};

use probe_core::{Environment, EnvironmentVariable, ImportDiagnostic, Variable};
use serde::Deserialize;
use serde_json::Value;

use crate::{conversion::variables::variable_value, diagnostics::warning};

/// An environment plus source metadata that the domain cannot represent.
///
/// Exported `value` is used verbatim: exports do not contain separate initial
/// and current values. No unavailable local/current value is reconstructed.
/// See <https://github.com/postmanlabs/newman#newmanrunoptions-object--callback-function>.
///
/// Secret-marked values remain **plaintext** in `environment`, not credential-store
/// secrets. Callers must handle them accordingly before any persistence or display.
pub struct ImportedPostmanEnvironment {
    /// Converted domain environment, including plaintext secret-marked values.
    pub environment: Environment,
    /// Original explicit Postman types, keyed by variable name (including `secret`).
    pub variable_types: BTreeMap<String, String>,
    /// Compatibility warnings, containing no exported variable values.
    pub diagnostics: Vec<ImportDiagnostic>,
}

impl fmt::Debug for ImportedPostmanEnvironment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ImportedPostmanEnvironment")
            .field("variable_count", &self.environment.variables.len())
            .field("diagnostics", &self.diagnostics)
            .finish_non_exhaustive()
    }
}

/// Value-free failures when importing a standalone Postman environment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PostmanEnvironmentImportError {
    /// Invalid JSON, missing/invalid fields, or empty names.
    Invalid,
    /// Not a standalone environment export, or an unsupported variable type.
    UnsupportedFormat,
    /// Two variables have the same key.
    DuplicateVariable,
}

impl fmt::Display for PostmanEnvironmentImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Invalid => "invalid Postman environment JSON or structure",
            Self::UnsupportedFormat => {
                "unsupported Postman environment source format or variable type"
            }
            Self::DuplicateVariable => "Postman environment contains duplicate variable names",
        })
    }
}

impl Error for PostmanEnvironmentImportError {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EnvironmentExport {
    name: String,
    values: Vec<EnvironmentEntry>,
    #[serde(rename = "_postman_variable_scope")]
    scope: String,
    // Official export bookkeeping has no domain equivalent.
    #[serde(default, rename = "id")]
    _id: Option<String>,
    #[serde(default, rename = "_postman_exported_at")]
    _exported_at: Option<String>,
    #[serde(default, rename = "_postman_exported_using")]
    _exported_using: Option<String>,
    #[serde(default, rename = "timestamp")]
    _timestamp: Option<Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EnvironmentEntry {
    key: String,
    // Requiring the field distinguishes an exported null from a missing value.
    #[serde(deserialize_with = "Value::deserialize")]
    value: Value,
    #[serde(default, rename = "type")]
    kind: Option<String>,
    #[serde(default = "enabled_by_default")]
    enabled: bool,
}

const fn enabled_by_default() -> bool {
    true
}

/// Converts one official standalone Postman environment JSON export in memory.
///
/// Accepts the legacy `text` and current `default`/`secret` type markers as well
/// as explicit SDK value types. An omitted enabled flag means enabled. Unknown
/// fields are rejected rather than silently dropping alternate value formats.
/// No files are read or written; diagnostics and errors omit variable values.
///
/// # Errors
/// Returns a typed error for malformed input, unsupported formats/types, or
/// duplicate variable names. JSON parser details are withheld to avoid disclosure.
pub fn import_postman_environment(
    source: &str,
) -> Result<ImportedPostmanEnvironment, PostmanEnvironmentImportError> {
    use PostmanEnvironmentImportError::{DuplicateVariable, Invalid, UnsupportedFormat};

    let document: EnvironmentExport = serde_json::from_str(source).map_err(|_| Invalid)?;
    if document.scope != "environment" {
        return Err(UnsupportedFormat);
    }
    if document.name.trim().is_empty() {
        return Err(Invalid);
    }
    let mut names = std::collections::BTreeSet::new();
    let mut imported = ImportedPostmanEnvironment {
        environment: Environment {
            name: document.name,
            color: None,
            description: None,
            extends: None,
            dot_env_file_path: None,
            variables: Vec::with_capacity(document.values.len()),
            client_certificates: Vec::new(),
        },
        variable_types: BTreeMap::new(),
        diagnostics: Vec::new(),
    };
    for (index, entry) in document.values.into_iter().enumerate() {
        if entry.key.trim().is_empty() {
            return Err(Invalid);
        }
        if !names.insert(entry.key.clone()) {
            return Err(DuplicateVariable);
        }
        let kind = match entry.kind.as_deref() {
            None | Some("default" | "any") => None,
            Some("text" | "secret") => Some("string"),
            Some(kind @ ("string" | "number" | "boolean" | "object")) => Some(kind),
            _ => return Err(UnsupportedFormat),
        };
        let resource_id = index.to_string();
        if entry.kind.as_deref() == Some("secret") {
            imported.diagnostics.push(warning(
                "secret_value_plaintext",
                "environment_variable",
                Some(&resource_id),
                Some("type"),
                "Postman secret marker retained as metadata; the exported value remains plaintext in memory and is not securely stored",
            ));
        }
        let value = variable_value(
            &entry.value,
            kind,
            "environment_variable",
            &resource_id,
            &mut imported.diagnostics,
        );
        if let Some(kind) = entry.kind {
            imported.variable_types.insert(entry.key.clone(), kind);
        }
        imported
            .environment
            .variables
            .push(EnvironmentVariable::Plain(Variable {
                name: Some(entry.key),
                value: Some(value),
                disabled: !entry.enabled,
            }));
    }
    Ok(imported)
}

#[cfg(test)]
mod tests;
