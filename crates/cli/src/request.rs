use std::{borrow::Cow, io::Read, path::PathBuf};

use probe_application::{NoSecrets, RequestResolution, prepare_request};
use probe_core::{
    ExpectationOutcome, FieldPatch, Request, RequestUpdate, RequestVariableInfo, SecretContext,
    SecretError, SecretProvider, SecretValue, StatusExpectation, VariableUsage,
    discover_request_variables, evaluate_expectations, resolve_environment_with_overrides,
    resolve_request, resolve_request_strict,
};
use probe_http::{ExecutionOptions, HttpEngine, HttpResponse};
use serde_json::json;

use crate::{
    CliError, CommandOutput, WorkspaceInput,
    error::expectation_json,
    load,
    presentation::{
        dry_run_human, dry_run_json, request_human, request_json, response_human, response_json,
        run_request_json, unset_documentation,
    },
};

pub(crate) fn list(
    input: &WorkspaceInput,
    stdin: &mut impl Read,
) -> Result<CommandOutput, CliError> {
    let loaded = load(input, stdin)?;
    let mut lines = vec!["SELECTOR\tTYPE\tMETHOD\tNAME\tURL".to_owned()];
    let mut requests = Vec::with_capacity(loaded.requests().len());
    for located in loaded.requests() {
        let request = loaded
            .workspace()
            .request(located.key())
            .expect("repository request key must resolve");
        let name = request.metadata.name.as_deref().unwrap_or("");
        let request_type = request.kind.as_str();
        let method = request.method.as_deref().unwrap_or("");
        let url = request.url.as_deref().unwrap_or("");
        lines.push(format!(
            "{}\t{request_type}\t{method}\t{name}\t{url}",
            located.selector()
        ));
        requests.push(json!({
            "method": request.method,
            "name": request.metadata.name,
            "selector": located.selector(),
            "type": request_type,
            "url": request.url,
        }));
    }
    Ok(CommandOutput {
        human: format!("{}\n", lines.join("\n")),
        json: json!({ "requests": requests }),
    })
}

pub(crate) fn get(
    input: &WorkspaceInput,
    selector: &str,
    environment: Option<&str>,
    strict_variables: bool,
    stdin: &mut impl Read,
) -> Result<CommandOutput, CliError> {
    let loaded = load(input, stdin)?;
    let request = selected_request(&loaded, selector, environment, &[], strict_variables)?;
    Ok(CommandOutput {
        human: request_human(selector, environment, &request).map_err(CliError::graphql)?,
        json: request_json(selector, environment, &request).map_err(CliError::graphql)?,
    })
}

pub(crate) fn variables(
    input: &WorkspaceInput,
    selector: &str,
    environment: Option<&str>,
    stdin: &mut impl Read,
) -> Result<CommandOutput, CliError> {
    let loaded = load(input, stdin)?;
    let key = loaded
        .request_key(selector)
        .ok_or_else(|| CliError::request_not_found(selector))?;
    let workspace = loaded.workspace();
    let request = workspace
        .request(key)
        .expect("repository request key must resolve");
    let variables = discover_request_variables(request, workspace.environments(), environment)
        .map_err(CliError::configuration)?;
    Ok(variable_output(&variables))
}

fn variable_output(variables: &[RequestVariableInfo]) -> CommandOutput {
    let mut lines = vec!["NAME\tDEFINED\tSECRET\tUSED IN".to_owned()];
    let json_variables = variables
        .iter()
        .map(|variable| {
            let usages = variable
                .usages
                .iter()
                .map(variable_usage_json)
                .collect::<Vec<_>>();
            lines.push(format!(
                "{}\t{}\t{}\t{}",
                variable.name,
                variable.defined,
                variable.secret,
                variable
                    .usages
                    .iter()
                    .map(variable_usage_human)
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            json!({
                "name": variable.name,
                "defined": variable.defined,
                "secret": variable.secret,
                "usages": usages,
            })
        })
        .collect::<Vec<_>>();
    CommandOutput {
        human: format!("{}\n", lines.join("\n")),
        json: json!({ "variables": json_variables }),
    }
}

fn variable_usage_human(usage: &VariableUsage) -> String {
    match usage {
        VariableUsage::Method => "method".to_owned(),
        VariableUsage::Url => "url".to_owned(),
        VariableUsage::Header { name } => format!("header: {name}"),
        VariableUsage::QueryParameter { name } => format!("query parameter: {name}"),
        VariableUsage::PathParameter { name } => format!("path parameter: {name}"),
        VariableUsage::Body => "body".to_owned(),
        VariableUsage::GraphqlQuery => "GraphQL query".to_owned(),
        VariableUsage::GraphqlVariables => "GraphQL variables".to_owned(),
        VariableUsage::GraphqlOperationName => "GraphQL operation name".to_owned(),
        VariableUsage::GraphqlExtensions => "GraphQL extensions".to_owned(),
        VariableUsage::FormUrlEncoded { name } => format!("form field: {name}"),
        VariableUsage::Multipart { name } => format!("multipart: {name}"),
        VariableUsage::File => "file".to_owned(),
        VariableUsage::Authentication { name } => format!("authentication: {name}"),
    }
}

fn variable_usage_json(usage: &VariableUsage) -> serde_json::Value {
    match usage {
        VariableUsage::Method => json!({ "location": "method" }),
        VariableUsage::Url => json!({ "location": "url" }),
        VariableUsage::Header { name } => json!({ "location": "header", "name": name }),
        VariableUsage::QueryParameter { name } => {
            json!({ "location": "query_parameter", "name": name })
        }
        VariableUsage::PathParameter { name } => {
            json!({ "location": "path_parameter", "name": name })
        }
        VariableUsage::Body => json!({ "location": "body" }),
        VariableUsage::GraphqlQuery => json!({ "location": "graphql_query" }),
        VariableUsage::GraphqlVariables => json!({ "location": "graphql_variables" }),
        VariableUsage::GraphqlOperationName => json!({ "location": "graphql_operation_name" }),
        VariableUsage::GraphqlExtensions => json!({ "location": "graphql_extensions" }),
        VariableUsage::FormUrlEncoded { name } => {
            json!({ "location": "form_urlencoded", "name": name })
        }
        VariableUsage::Multipart { name } => {
            json!({ "location": "multipart", "name": name })
        }
        VariableUsage::File => json!({ "location": "file" }),
        VariableUsage::Authentication { name } => {
            json!({ "location": "authentication", "name": name })
        }
    }
}

pub(crate) fn update(
    input: &WorkspaceInput,
    selector: &str,
    update: &RequestUpdate,
    description: &crate::command::DescriptionEdit,
    stdin: &mut impl Read,
) -> Result<CommandOutput, CliError> {
    let mut loaded = load(input, stdin)?;
    let mut update = update.clone();
    let existing = loaded
        .request_key(selector)
        .and_then(|key| loaded.workspace().request(key))
        .and_then(|request| request.metadata.description.as_ref());
    update.description = description.resolve(existing);
    loaded
        .update_request(selector, &update)
        .map_err(CliError::persistence)?;
    let key = loaded
        .request_key(selector)
        .expect("successfully updated selector must resolve");
    let request = loaded
        .workspace()
        .request(key)
        .expect("repository request key must resolve");
    Ok(CommandOutput {
        human: format!(
            "Updated request\n{}",
            request_human(selector, None, request).map_err(CliError::graphql)?
        ),
        json: request_json(selector, None, request).map_err(CliError::graphql)?,
    })
}

pub(crate) fn unset(
    input: &WorkspaceInput,
    selector: &str,
    update: &RequestUpdate,
    stdin: &mut impl Read,
) -> Result<CommandOutput, CliError> {
    let mut fields = Vec::new();
    if matches!(update.description, FieldPatch::Clear) {
        fields.push("description");
    }
    if matches!(update.docs, FieldPatch::Clear) {
        fields.push("docs");
    }
    let mut loaded = load(input, stdin)?;
    loaded
        .update_request(selector, update)
        .map_err(CliError::persistence)?;
    Ok(unset_documentation("request", Some(selector), &fields))
}

pub(crate) struct RunOptions<'a> {
    pub(crate) environment: Option<&'a str>,
    pub(crate) variables: &'a [(String, String)],
    pub(crate) output: Option<&'a PathBuf>,
    pub(crate) strict_variables: bool,
    pub(crate) dry_run: bool,
    pub(crate) show_headers: bool,
    pub(crate) secret_provider_env: bool,
    pub(crate) expectations: &'a [StatusExpectation],
    pub(crate) human_output: bool,
}

pub(crate) fn run(
    input: &WorkspaceInput,
    selector: &str,
    options: &RunOptions<'_>,
    stdin: &mut impl Read,
) -> Result<CommandOutput, CliError> {
    let loaded = load(input, stdin)?;
    let key = loaded
        .request_key(selector)
        .ok_or_else(|| CliError::request_not_found(selector))?;
    let source = loaded
        .workspace()
        .request(key)
        .expect("repository request key must resolve");
    let provider: &dyn SecretProvider = if options.secret_provider_env {
        &ProcessEnvironmentSecretProvider
    } else {
        &NoSecrets
    };
    let workspace_identity = match input {
        WorkspaceInput::Path(path) => path.to_str(),
        WorkspaceInput::Stdin => Some("-"),
    };
    let prepared = prepare_request(
        source,
        &RequestResolution {
            environments: loaded.workspace().environments(),
            environment: options.environment,
            overrides: options.variables,
            strict_variables: options.strict_variables,
            workspace_identity,
        },
        provider,
    )
    .map_err(CliError::configuration)?;
    let display = prepared.presentation();
    if options.dry_run {
        return Ok(CommandOutput {
            human: dry_run_human(display),
            json: dry_run_json(display).map_err(CliError::graphql)?,
        });
    }
    let method = display
        .method
        .clone()
        .unwrap_or_else(|| "<unset>".to_owned());
    let request_json = run_request_json(display).map_err(CliError::graphql)?;
    let execution = prepared.into_http().map_err(CliError::graphql)?;
    let execution_options = ExecutionOptions {
        base_directory: input.base_directory(),
        ..ExecutionOptions::default()
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| CliError::runtime(&error))?;
    let response = runtime.block_on(async {
        let engine = HttpEngine::new().map_err(CliError::http)?;
        execution
            .execute(
                &engine,
                execution_options,
                options.output.map(PathBuf::as_path),
                tokio::signal::ctrl_c(),
                |_| {},
            )
            .await
            .map(|executed| executed.response)
            .map_err(CliError::http)
    })?;
    let outcomes = evaluate_expectations(options.expectations, response.status);
    if outcomes.iter().any(|outcome| !outcome.ok) {
        return Err(CliError::expectation_failed(&outcomes));
    }
    response_output(
        || {
            response_human(
                &method,
                &response,
                options.output.map(PathBuf::as_path),
                options.show_headers,
            )
        },
        options.human_output,
        request_json,
        &response,
        options.output,
        &outcomes,
    )
}

/// Use only with trusted collections: their secret names select process variables
/// whose values can be sent to collection-defined request destinations.
struct ProcessEnvironmentSecretProvider;
impl SecretProvider for ProcessEnvironmentSecretProvider {
    fn resolve_secret(
        &self,
        context: &SecretContext<'_>,
    ) -> Result<Option<SecretValue>, SecretError> {
        if context.variable_name.is_empty() || context.variable_name.contains(['=', '\0']) {
            return Err(SecretError);
        }
        match std::env::var_os(context.variable_name) {
            Some(value) => value
                .into_string()
                .map(SecretValue::new)
                .map(Some)
                .map_err(|_| SecretError),
            None => Ok(None),
        }
    }
}

fn selected_request<'a>(
    loaded: &'a probe_opencollection::LoadedWorkspace,
    selector: &str,
    environment: Option<&str>,
    variables: &[(String, String)],
    strict_variables: bool,
) -> Result<Cow<'a, Request>, CliError> {
    let key = loaded
        .request_key(selector)
        .ok_or_else(|| CliError::request_not_found(selector))?;
    if environment.is_some() || !variables.is_empty() || strict_variables {
        let workspace = loaded.workspace();
        let environment =
            resolve_environment_with_overrides(workspace.environments(), environment, variables)
                .map_err(CliError::configuration)?;
        let request = workspace
            .request(key)
            .expect("repository request key must resolve");
        if strict_variables {
            resolve_request_strict(request, &environment)
        } else {
            resolve_request(request, &environment)
        }
        .map(Cow::Owned)
        .map_err(CliError::configuration)
    } else {
        Ok(Cow::Borrowed(
            loaded
                .workspace()
                .request(key)
                .expect("repository request key must resolve"),
        ))
    }
}

fn response_output(
    render_human: impl FnOnce() -> String,
    human_output: bool,
    request_json: serde_json::Value,
    response: &HttpResponse,
    output: Option<&PathBuf>,
    outcomes: &[ExpectationOutcome],
) -> Result<CommandOutput, CliError> {
    let output = output.map(PathBuf::as_path);
    let mut json = response_json(request_json, response, output);
    if !outcomes.is_empty() {
        let Some(object) = json.as_object_mut() else {
            return Err(CliError {
                category: "runtime_error",
                message: "request run JSON output must be an object".to_owned(),
                exit_code: crate::EXECUTION_EXIT_CODE,
                details: None,
            });
        };
        object.insert(
            "expectations".to_owned(),
            json!(outcomes.iter().map(expectation_json).collect::<Vec<_>>()),
        );
    }
    Ok(CommandOutput {
        human: if human_output {
            render_human()
        } else {
            String::new()
        },
        json,
    })
}

#[cfg(test)]
mod response_output_tests {
    use super::response_output;
    use probe_http::HttpResponse;
    use serde_json::json;

    #[test]
    fn response_human_rendering_is_lazy_and_preserves_json() {
        let body = br#"{"message":"hello"}"#.to_vec();
        let response = HttpResponse {
            status: 200,
            initial_url: "https://example.com/start".to_owned(),
            url_changed: false,
            reason: "OK".to_owned(),
            url: "https://example.com".to_owned(),
            duration: std::time::Duration::ZERO,
            size: body.len(),
            headers: Vec::new(),
            body,
            body_complete: true,
            body_file: None,
            body_retention_error: None,
        };
        let human = response_output(
            || "human response\n".to_owned(),
            crate::human_output_requested(false, false),
            json!({}),
            &response,
            None,
            &[],
        )
        .unwrap();
        assert_eq!(human.human, "human response\n");

        // Both --json and --quiet disable the human renderer before response output.
        for (json_output, quiet) in [(true, false), (false, true)] {
            let output = response_output(
                || panic!("human response rendering must be skipped"),
                crate::human_output_requested(json_output, quiet),
                json!({}),
                &response,
                None,
                &[],
            )
            .unwrap();
            assert!(output.human.is_empty());
            assert_eq!(output.json, human.json);
            let rendered = output.render(json_output, quiet);
            if quiet {
                assert!(rendered.is_empty());
            } else {
                let value: serde_json::Value = serde_json::from_str(&rendered).unwrap();
                assert_eq!(value["schemaVersion"], crate::JSON_SCHEMA_VERSION);
            }
        }
    }
}

#[cfg(test)]
mod usage_json_tests {
    use super::{VariableUsage, variable_usage_json};
    use serde_json::json;

    #[test]
    fn structured_usage_locations_keep_the_documented_names_and_field_names() {
        let cases = [
            (
                VariableUsage::GraphqlExtensions,
                json!({"location": "graphql_extensions"}),
            ),
            (
                VariableUsage::FormUrlEncoded {
                    name: "key".to_owned(),
                },
                json!({"location": "form_urlencoded", "name": "key"}),
            ),
            (
                VariableUsage::Multipart {
                    name: "upload".to_owned(),
                },
                json!({"location": "multipart", "name": "upload"}),
            ),
            (VariableUsage::File, json!({"location": "file"})),
        ];
        for (usage, expected) in cases {
            assert_eq!(variable_usage_json(&usage), expected);
        }
    }
}

#[cfg(test)]
mod process_provider_tests {
    use super::ProcessEnvironmentSecretProvider;
    use probe_core::{SecretContext, SecretProvider};

    #[test]
    fn invalid_environment_variable_name_returns_an_error() {
        let provider = ProcessEnvironmentSecretProvider;
        for name in ["", "bad=key", "bad\0key"] {
            let result = provider.resolve_secret(&SecretContext {
                variable_name: name,
                environment_name: Some("local"),
                workspace_identity: None,
            });
            assert!(result.is_err(), "{name:?}");
        }
    }
}
