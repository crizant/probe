//! Command-line presentation adapter for Probe.

#![forbid(unsafe_code)]

use std::{fs, path::PathBuf};

use probe_core::{EnvironmentResolutionError, resolve_environment, resolve_request};
use probe_http::{ExecutionOptions, HttpEngine, HttpError, HttpResponse};
use probe_opencollection::{LoadedWorkspace, load_workspace};
use serde_json::json;

mod presentation;

use presentation::{request_human, request_json, response_human, response_json};

/// Exit code used when command-line arguments are invalid.
pub const INVALID_ARGUMENTS_EXIT_CODE: u8 = 2;
/// Exit code used when a workspace cannot be loaded or parsed.
pub const INVALID_WORKSPACE_EXIT_CODE: u8 = 3;
/// Exit code used when a request selector does not exist.
pub const REQUEST_NOT_FOUND_EXIT_CODE: u8 = 4;
/// Exit code reserved for environment and configuration errors.
pub const CONFIGURATION_EXIT_CODE: u8 = 5;
/// Exit code used for request execution and output errors.
pub const EXECUTION_EXIT_CODE: u8 = 6;

/// Captured CLI output and process status.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunOutput {
    /// Command output written to stdout.
    pub stdout: String,
    /// Diagnostics written to stderr.
    pub stderr: String,
    /// Process exit code.
    pub exit_code: u8,
}

impl RunOutput {
    fn success(stdout: String) -> Self {
        Self {
            stdout,
            stderr: String::new(),
            exit_code: 0,
        }
    }

    fn failure(error: CliError, json_output: bool) -> Self {
        if json_output {
            Self {
                stdout: pretty_json(&json!({
                    "error": {
                        "category": error.category,
                        "message": error.message,
                    }
                })),
                stderr: String::new(),
                exit_code: error.exit_code,
            }
        } else {
            Self {
                stdout: String::new(),
                stderr: format!("error[{}]: {}\n", error.category, error.message),
                exit_code: error.exit_code,
            }
        }
    }
}

/// Returns the stable top-level help text.
#[must_use]
pub const fn help() -> &'static str {
    concat!(
        "Probe - a fast, native, local-first API client\n",
        "\n",
        "Usage: probe [OPTIONS] <COMMAND>\n",
        "\n",
        "Commands:\n",
        "  collection validate <path>          Validate an OpenCollection workspace\n",
        "  request list <path>                 List HTTP requests\n",
        "  request get <path> <selector>       Inspect an HTTP request\n",
        "  request run <path> <selector>       Execute an HTTP request\n",
        "\n",
        "Options:\n",
        "      --environment <name>  Resolve request variables with an environment\n",
        "      --output <file>        Write the response body to a file\n",
        "      --json               Emit deterministic JSON\n",
        "  -h, --help               Print help\n",
    )
}

const COLLECTION_HELP: &str = concat!(
    "Usage: probe collection validate <path> [--json]\n",
    "\n",
    "Validate a bundled OpenCollection YAML file or an unbundled directory.\n",
);

const REQUEST_HELP: &str = concat!(
    "Usage: probe request <COMMAND>\n",
    "\n",
    "Commands:\n",
    "  list <path>                 List requests and repository selectors\n",
    "  get <path> <selector> [--environment <name>]  Inspect one request\n",
    "  run <path> <selector> [--environment <name>] [--output <file>]\n",
);

/// Runs the CLI adapter for arguments that exclude the executable name.
#[must_use]
pub fn run<I, S>(args: I) -> RunOutput
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut args: Vec<String> = args
        .into_iter()
        .map(|argument| argument.as_ref().to_owned())
        .collect();
    let json_count = args.iter().filter(|argument| *argument == "--json").count();
    let json_output = json_count == 1;
    args.retain(|argument| argument != "--json");

    if json_count > 1 {
        return RunOutput::failure(
            CliError::invalid_arguments("--json may only be specified once"),
            true,
        );
    }

    if args.is_empty() || args == ["-h"] || args == ["--help"] {
        return RunOutput::success(help().to_owned());
    }
    if args
        .iter()
        .any(|argument| matches!(argument.as_str(), "-h" | "--help"))
    {
        let help = match args.first().map(String::as_str) {
            Some("collection") => COLLECTION_HELP,
            Some("request") => REQUEST_HELP,
            _ => help(),
        };
        return RunOutput::success(help.to_owned());
    }

    let environment = match extract_environment(&mut args) {
        Ok(environment) => environment,
        Err(error) => return RunOutput::failure(error, json_output),
    };
    let output = match extract_output(&mut args) {
        Ok(output) => output,
        Err(error) => return RunOutput::failure(error, json_output),
    };

    match parse_command(&args, environment, output).and_then(execute) {
        Ok(output) => RunOutput::success(output.render(json_output)),
        Err(error) => RunOutput::failure(error, json_output),
    }
}

#[derive(Debug)]
enum Command {
    Validate {
        path: PathBuf,
    },
    List {
        path: PathBuf,
    },
    Get {
        path: PathBuf,
        selector: String,
        environment: Option<String>,
    },
    Run {
        path: PathBuf,
        selector: String,
        environment: Option<String>,
        output: Option<PathBuf>,
    },
}

#[derive(Debug)]
struct CommandOutput {
    human: String,
    json: serde_json::Value,
}

impl CommandOutput {
    fn render(self, json_output: bool) -> String {
        if json_output {
            pretty_json(&self.json)
        } else {
            self.human
        }
    }
}

#[derive(Debug)]
struct CliError {
    category: &'static str,
    message: String,
    exit_code: u8,
}

impl CliError {
    fn invalid_arguments(message: impl Into<String>) -> Self {
        Self {
            category: "invalid_arguments",
            message: message.into(),
            exit_code: INVALID_ARGUMENTS_EXIT_CODE,
        }
    }

    fn invalid_workspace(message: impl Into<String>) -> Self {
        Self {
            category: "invalid_workspace",
            message: message.into(),
            exit_code: INVALID_WORKSPACE_EXIT_CODE,
        }
    }

    fn request_not_found(selector: &str) -> Self {
        Self {
            category: "request_not_found",
            message: format!("request selector not found: {selector}"),
            exit_code: REQUEST_NOT_FOUND_EXIT_CODE,
        }
    }

    fn configuration(error: EnvironmentResolutionError) -> Self {
        let category = match error {
            EnvironmentResolutionError::EnvironmentNotFound(_) => "environment_not_found",
            EnvironmentResolutionError::MissingVariable(_) => "missing_variable",
            EnvironmentResolutionError::SecretVariableUnavailable(_) => {
                "secret_variable_unavailable"
            }
            _ => "environment_resolution",
        };
        Self {
            category,
            message: error.to_string(),
            exit_code: CONFIGURATION_EXIT_CODE,
        }
    }

    fn http(error: HttpError) -> Self {
        let category = if error.is_configuration() {
            "request_configuration"
        } else {
            match error {
                HttpError::Timeout => "request_timeout",
                HttpError::Cancelled => "request_cancelled",
                _ => "network_execution",
            }
        };
        Self {
            category,
            message: error.to_string(),
            exit_code: if error.is_configuration() {
                CONFIGURATION_EXIT_CODE
            } else {
                EXECUTION_EXIT_CODE
            },
        }
    }

    fn output(path: &std::path::Path, error: &std::io::Error) -> Self {
        Self {
            category: "output_error",
            message: format!(
                "cannot write response body to '{}': {error}",
                path.display()
            ),
            exit_code: EXECUTION_EXIT_CODE,
        }
    }

    fn runtime(error: &std::io::Error) -> Self {
        Self {
            category: "runtime_error",
            message: format!("cannot start asynchronous HTTP runtime: {error}"),
            exit_code: EXECUTION_EXIT_CODE,
        }
    }
}

fn extract_environment(args: &mut Vec<String>) -> Result<Option<String>, CliError> {
    let positions: Vec<_> = args
        .iter()
        .enumerate()
        .filter_map(|(index, argument)| (argument == "--environment").then_some(index))
        .collect();
    match positions.as_slice() {
        [] => Ok(None),
        [_first, _second, ..] => Err(CliError::invalid_arguments(
            "--environment may only be specified once",
        )),
        [position] => {
            if *position + 1 >= args.len() || args[*position + 1].starts_with('-') {
                return Err(CliError::invalid_arguments(
                    "--environment requires a non-empty name",
                ));
            }
            let value = args.remove(*position + 1);
            args.remove(*position);
            Ok(Some(value))
        }
    }
}

fn extract_output(args: &mut Vec<String>) -> Result<Option<PathBuf>, CliError> {
    let positions: Vec<_> = args
        .iter()
        .enumerate()
        .filter_map(|(index, argument)| (argument == "--output").then_some(index))
        .collect();
    match positions.as_slice() {
        [] => Ok(None),
        [_first, _second, ..] => Err(CliError::invalid_arguments(
            "--output may only be specified once",
        )),
        [position] => {
            if *position + 1 >= args.len() || args[*position + 1].starts_with('-') {
                return Err(CliError::invalid_arguments(
                    "--output requires a non-empty file path",
                ));
            }
            let value = PathBuf::from(args.remove(*position + 1));
            args.remove(*position);
            Ok(Some(value))
        }
    }
}

fn parse_command(
    args: &[String],
    environment: Option<String>,
    output: Option<PathBuf>,
) -> Result<Command, CliError> {
    match args {
        [group, action, path]
            if group == "collection"
                && action == "validate"
                && environment.is_none()
                && output.is_none() =>
        {
            Ok(Command::Validate {
                path: PathBuf::from(path),
            })
        }
        [group, action, path]
            if group == "request"
                && action == "list"
                && environment.is_none()
                && output.is_none() =>
        {
            Ok(Command::List {
                path: PathBuf::from(path),
            })
        }
        [group, action, path, selector]
            if group == "request" && action == "get" && output.is_none() =>
        {
            Ok(Command::Get {
                path: PathBuf::from(path),
                selector: selector.clone(),
                environment,
            })
        }
        [group, action, path, selector] if group == "request" && action == "run" => {
            Ok(Command::Run {
                path: PathBuf::from(path),
                selector: selector.clone(),
                environment,
                output,
            })
        }
        _ => Err(CliError::invalid_arguments(
            "invalid command; run 'probe --help' for usage",
        )),
    }
}

fn execute(command: Command) -> Result<CommandOutput, CliError> {
    match command {
        Command::Validate { path } => validate(&path),
        Command::List { path } => list_requests(&path),
        Command::Get {
            path,
            selector,
            environment,
        } => get_request(&path, &selector, environment.as_deref()),
        Command::Run {
            path,
            selector,
            environment,
            output,
        } => run_request(&path, &selector, environment.as_deref(), output.as_ref()),
    }
}

fn run_request(
    path: &PathBuf,
    selector: &str,
    environment: Option<&str>,
    output: Option<&PathBuf>,
) -> Result<CommandOutput, CliError> {
    let loaded = load(path)?;
    let key = loaded
        .request_key(selector)
        .ok_or_else(|| CliError::request_not_found(selector))?;
    let request = if let Some(environment) = environment {
        resolve_loaded_request(&loaded, key, environment)?
    } else {
        loaded
            .workspace()
            .request(key)
            .expect("repository request key must resolve")
            .clone()
    };
    let base_directory = if path.is_dir() {
        Some(path.clone())
    } else {
        path.parent().map(std::path::Path::to_owned)
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| CliError::runtime(&error))?;
    let response = runtime.block_on(async {
        let engine = HttpEngine::new().map_err(CliError::http)?;
        engine
            .execute_cancellable(
                &request,
                &ExecutionOptions { base_directory },
                tokio::signal::ctrl_c(),
            )
            .await
            .map_err(CliError::http)
    })?;
    if let Some(output) = output {
        fs::write(output, &response.body).map_err(|error| CliError::output(output, &error))?;
    }
    Ok(response_output(&request, &response, output))
}

fn response_output(
    request: &probe_core::HttpRequest,
    response: &HttpResponse,
    output: Option<&PathBuf>,
) -> CommandOutput {
    CommandOutput {
        human: response_human(request, response, output.map(PathBuf::as_path)),
        json: response_json(request, response, output.map(PathBuf::as_path)),
    }
}

fn validate(path: &PathBuf) -> Result<CommandOutput, CliError> {
    let loaded = load(path)?;
    let workspace = loaded.workspace();
    let name = workspace.metadata().name.as_deref().unwrap_or("<unnamed>");
    Ok(CommandOutput {
        human: format!(
            "Valid OpenCollection workspace\nName: {name}\nRequests: {}\nFolders: {}\nEnvironments: {}\n",
            workspace.request_count(),
            workspace.folder_count(),
            workspace.environments().len()
        ),
        json: json!({
            "collection": {
                "name": workspace.metadata().name,
                "summary": workspace.metadata().summary,
                "version": workspace.metadata().version,
            },
            "counts": {
                "environments": workspace.environments().len(),
                "folders": workspace.folder_count(),
                "requests": workspace.request_count(),
            },
            "valid": true,
        }),
    })
}

fn list_requests(path: &PathBuf) -> Result<CommandOutput, CliError> {
    let loaded = load(path)?;
    let mut lines = vec!["SELECTOR\tMETHOD\tNAME\tURL".to_owned()];
    let mut requests = Vec::with_capacity(loaded.requests().len());
    for located in loaded.requests() {
        let request = loaded
            .workspace()
            .request(located.key())
            .expect("repository request key must resolve");
        let name = request.metadata.name.as_deref().unwrap_or("");
        let method = request.method.as_deref().unwrap_or("");
        let url = request.url.as_deref().unwrap_or("");
        lines.push(format!("{}\t{method}\t{name}\t{url}", located.selector()));
        requests.push(json!({
            "method": request.method,
            "name": request.metadata.name,
            "selector": located.selector(),
            "url": request.url,
        }));
    }
    Ok(CommandOutput {
        human: format!("{}\n", lines.join("\n")),
        json: json!({ "requests": requests }),
    })
}

fn get_request(
    path: &PathBuf,
    selector: &str,
    environment: Option<&str>,
) -> Result<CommandOutput, CliError> {
    let loaded = load(path)?;
    let key = loaded
        .request_key(selector)
        .ok_or_else(|| CliError::request_not_found(selector))?;
    let resolved;
    let request = if let Some(environment) = environment {
        resolved = resolve_loaded_request(&loaded, key, environment)?;
        &resolved
    } else {
        loaded
            .workspace()
            .request(key)
            .expect("repository request key must resolve")
    };
    Ok(CommandOutput {
        human: request_human(selector, environment, request),
        json: request_json(selector, environment, request),
    })
}

fn resolve_loaded_request(
    loaded: &LoadedWorkspace,
    key: probe_core::RequestKey,
    environment: &str,
) -> Result<probe_core::HttpRequest, CliError> {
    let workspace = loaded.workspace();
    let environment = resolve_environment(workspace.environments(), environment)
        .map_err(CliError::configuration)?;
    let request = workspace
        .request(key)
        .expect("repository request key must resolve");
    resolve_request(request, &environment).map_err(CliError::configuration)
}

fn load(path: &PathBuf) -> Result<LoadedWorkspace, CliError> {
    load_workspace(path).map_err(|error| CliError::invalid_workspace(error.to_string()))
}

fn pretty_json(value: &serde_json::Value) -> String {
    let mut output =
        serde_json::to_string_pretty(value).expect("JSON value serialization cannot fail");
    output.push('\n');
    output
}

#[cfg(test)]
mod tests {
    use super::{INVALID_ARGUMENTS_EXIT_CODE, help, run};

    #[test]
    fn help_option_returns_usage() {
        let output = run(["--help"]);
        assert_eq!(output.exit_code, 0);
        assert_eq!(output.stdout, help());
        assert!(output.stderr.is_empty());
    }

    #[test]
    fn unknown_argument_is_rejected() {
        let output = run(["--unknown"]);
        assert_eq!(output.exit_code, INVALID_ARGUMENTS_EXIT_CODE);
        assert!(output.stdout.is_empty());
        assert!(output.stderr.contains("invalid_arguments"));
    }
}
