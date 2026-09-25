use std::collections::BTreeMap;

use probe_core::{Authentication, AuthenticationKind, AuthenticationValue};
use serde_yaml_ng::Value;

use super::diagnostic;
use crate::{ProjectionDiagnostic, ProjectionDiagnosticKind};

pub(super) fn project_authentication(
    value: Value,
    path: &str,
    diagnostics: &mut Vec<ProjectionDiagnostic>,
) -> Result<Authentication, serde_yaml_ng::Error> {
    Ok(match value {
        Value::String(value) => {
            if value != "inherit" {
                diagnostic(
                    diagnostics,
                    path.to_owned(),
                    ProjectionDiagnosticKind::AuthenticationKind,
                    &value,
                );
            }
            Authentication {
                kind: if value == "inherit" {
                    AuthenticationKind::Inherit
                } else {
                    AuthenticationKind::Other(value)
                },
                properties: BTreeMap::new(),
            }
        }
        Value::Mapping(properties) => {
            let kind = properties
                .get(Value::String("type".to_owned()))
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    <serde_yaml_ng::Error as serde::de::Error>::custom(
                        "authentication type must be a string",
                    )
                })?
                .to_owned();
            if !matches!(kind.as_str(), "basic" | "bearer") {
                diagnostic(
                    diagnostics,
                    format!("{path}/type"),
                    ProjectionDiagnosticKind::AuthenticationKind,
                    &kind,
                );
            }
            let mut projected_properties = BTreeMap::new();
            for (name, value) in properties {
                let Some(name) = name.as_str() else {
                    diagnostic(
                        diagnostics,
                        path.to_owned(),
                        ProjectionDiagnosticKind::AuthenticationProperty,
                        format!("{name:?}"),
                    );
                    continue;
                };
                if name == "type" {
                    continue;
                }
                let supported = match kind.as_str() {
                    "basic" => matches!(name, "username" | "password"),
                    "bearer" => name == "token",
                    _ => true,
                };
                if !supported || (matches!(kind.as_str(), "basic" | "bearer") && !value.is_string())
                {
                    diagnostic(
                        diagnostics,
                        format!("{path}/{name}"),
                        ProjectionDiagnosticKind::AuthenticationProperty,
                        name,
                    );
                }
                projected_properties.insert(
                    name.to_owned(),
                    authentication_value(value, &format!("{path}/{name}"), diagnostics),
                );
            }
            Authentication {
                kind: match kind.as_str() {
                    "awsv4" => AuthenticationKind::AwsV4,
                    "basic" => AuthenticationKind::Basic,
                    "wsse" => AuthenticationKind::Wsse,
                    "bearer" => AuthenticationKind::Bearer,
                    "digest" => AuthenticationKind::Digest,
                    "ntlm" => AuthenticationKind::Ntlm,
                    "apikey" => AuthenticationKind::ApiKey,
                    "oauth1" => AuthenticationKind::OAuth1,
                    "oauth2" => AuthenticationKind::OAuth2,
                    other => AuthenticationKind::Other(other.to_owned()),
                },
                properties: projected_properties,
            }
        }
        _ => {
            return Err(<serde_yaml_ng::Error as serde::de::Error>::custom(
                "authentication must be a string or mapping",
            ));
        }
    })
}

fn authentication_value(
    value: Value,
    path: &str,
    diagnostics: &mut Vec<ProjectionDiagnostic>,
) -> AuthenticationValue {
    match value {
        Value::Null => AuthenticationValue::Null,
        Value::Bool(value) => AuthenticationValue::Boolean(value),
        Value::Number(value) => AuthenticationValue::Number(value.to_string()),
        Value::String(value) => AuthenticationValue::String(value),
        Value::Sequence(values) => AuthenticationValue::Sequence(
            values
                .into_iter()
                .enumerate()
                .map(|(index, value)| {
                    authentication_value(value, &format!("{path}/{index}"), diagnostics)
                })
                .collect(),
        ),
        Value::Mapping(values) => {
            let mut projected = BTreeMap::new();
            for (name, value) in values {
                if let Some(name) = name.as_str() {
                    projected.insert(
                        name.to_owned(),
                        authentication_value(value, &format!("{path}/{name}"), diagnostics),
                    );
                } else {
                    diagnostic(
                        diagnostics,
                        path.to_owned(),
                        ProjectionDiagnosticKind::AuthenticationProperty,
                        format!("{name:?}"),
                    );
                }
            }
            AuthenticationValue::Object(projected)
        }
        Value::Tagged(value) => authentication_value(value.value, path, diagnostics),
    }
}
