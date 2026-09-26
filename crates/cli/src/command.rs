use std::{iter::Peekable, path::PathBuf, vec::IntoIter};

use probe_core::{FieldPatch, GraphqlUpdate, RequestUpdate, StatusExpectation};
use probe_opencollection::{CreatedRequestProtocol, StructureOperation};
use serde_json::{Map, Value};

use crate::{CliError, WorkspaceInput};

#[derive(Debug)]
pub(crate) enum Command {
    CreateCollection {
        path: PathBuf,
        name: Option<String>,
    },
    ImportYaak {
        source: PathBuf,
        destination: PathBuf,
        workspace: Option<String>,
        allow_partial: bool,
    },
    ImportPostman {
        source: PathBuf,
        destination: PathBuf,
        allow_partial: bool,
    },
    Validate {
        input: WorkspaceInput,
    },
    ListRequests {
        input: WorkspaceInput,
    },
    ListFolders {
        input: WorkspaceInput,
    },
    ListEnvironments {
        input: WorkspaceInput,
    },
    Get {
        input: WorkspaceInput,
        selector: String,
        environment: Option<String>,
        strict_variables: bool,
    },
    Variables {
        input: WorkspaceInput,
        selector: String,
        environment: Option<String>,
    },
    Run {
        input: WorkspaceInput,
        selector: String,
        environment: Option<String>,
        variables: Vec<(String, String)>,
        output: Option<PathBuf>,
        strict_variables: bool,
        dry_run: bool,
        secret_provider_env: bool,
        expectations: Vec<StatusExpectation>,
    },
    Set {
        input: WorkspaceInput,
        selector: String,
        update: Box<RequestUpdate>,
    },
    Structure {
        input: WorkspaceInput,
        operation_name: &'static str,
        operation: Box<StructureOperation>,
    },
    EnvironmentSet {
        input: WorkspaceInput,
        environment: String,
        name: String,
        value: String,
    },
    EnvironmentUnset {
        input: WorkspaceInput,
        environment: String,
        name: String,
    },
    EnvironmentCreate {
        input: WorkspaceInput,
        name: String,
        extends: Option<String>,
    },
    EnvironmentDelete {
        input: WorkspaceInput,
        environment: String,
    },
    EnvironmentRename {
        input: WorkspaceInput,
        environment: String,
        name: String,
    },
}

pub(crate) fn parse(args: Vec<String>) -> Result<Command, CliError> {
    let mut parser = Parser::new(args);
    let group = parser.command_word()?;
    let action = parser.command_word()?;
    match (group.as_str(), action.as_str()) {
        ("collection", "create") => parse_collection_create(parser),
        ("collection", "import") => parse_import(parser),
        ("collection", "validate") => Ok(Command::Validate {
            input: workspace(parser)?,
        }),
        ("request", "list") => Ok(Command::ListRequests {
            input: workspace(parser)?,
        }),
        ("folder", "list") => Ok(Command::ListFolders {
            input: workspace(parser)?,
        }),
        ("environment", "list") => Ok(Command::ListEnvironments {
            input: workspace(parser)?,
        }),
        ("request", "get") => parse_get(parser),
        ("request", "variables") => parse_variables(parser),
        ("request", "run") => parse_run(parser),
        ("request", "set") => parse_request_set(parser),
        ("request", "create") => parse_request_create(parser),
        ("folder", "create") => parse_folder_create(parser),
        ("request" | "folder", "rename" | "delete" | "move" | "reorder") => {
            parse_item(parser, group == "request", &action)
        }
        ("environment", "create") => parse_environment_create(parser),
        ("environment", "set") => parse_environment_set(parser),
        ("environment", "unset") => parse_environment_unset(parser),
        ("environment", "delete") => parse_environment_delete(parser),
        ("environment", "rename") => parse_environment_rename(parser),
        _ => Err(invalid_command()),
    }
}

fn parse_collection_create(mut parser: Parser) -> Result<Command, CliError> {
    let mut path = Vec::new();
    let mut name = None;
    while let Some(argument) = parser.bump() {
        match argument.as_str() {
            "--name" => parser.once(&mut name, "--name")?,
            other => push_positional(&mut path, other, 1)?,
        }
    }
    let path = one_path(&path)?;
    reject_stdin(&path)?;
    Ok(Command::CreateCollection {
        path: PathBuf::from(path),
        name,
    })
}

fn parse_import(mut parser: Parser) -> Result<Command, CliError> {
    let format = parser.command_word()?;
    match format.as_str() {
        "yaak" => parse_yaak_import(parser),
        "postman" => parse_postman_import(parser),
        _ => Err(invalid_command()),
    }
}

fn parse_yaak_import(mut parser: Parser) -> Result<Command, CliError> {
    let mut positionals = Vec::new();
    let mut workspace = None;
    let mut allow_partial = false;
    while let Some(argument) = parser.bump() {
        match argument.as_str() {
            "--workspace" => parser.once(&mut workspace, "--workspace")?,
            "--allow-partial" => parser.flag(&mut allow_partial, "--allow-partial")?,
            other => push_positional(&mut positionals, other, 2)?,
        }
    }
    let (source, destination) = two_paths(&positionals)?;
    reject_stdin(&source)?;
    reject_stdin(&destination)?;
    Ok(Command::ImportYaak {
        source: PathBuf::from(source),
        destination: PathBuf::from(destination),
        workspace,
        allow_partial,
    })
}

fn parse_postman_import(mut parser: Parser) -> Result<Command, CliError> {
    let mut positionals = Vec::new();
    let mut allow_partial = false;
    while let Some(argument) = parser.bump() {
        match argument.as_str() {
            "--allow-partial" => parser.flag(&mut allow_partial, "--allow-partial")?,
            other => push_positional(&mut positionals, other, 2)?,
        }
    }
    let (source, destination) = two_paths(&positionals)?;
    reject_stdin(&source)?;
    reject_stdin(&destination)?;
    Ok(Command::ImportPostman {
        source: PathBuf::from(source),
        destination: PathBuf::from(destination),
        allow_partial,
    })
}

fn parse_get(mut parser: Parser) -> Result<Command, CliError> {
    let mut positionals = Vec::new();
    let mut environment = None;
    let mut strict_variables = false;
    while let Some(argument) = parser.bump() {
        match argument.as_str() {
            "--environment" => parser.once(&mut environment, "--environment")?,
            "--strict-variables" => parser.flag(&mut strict_variables, "--strict-variables")?,
            other => push_positional(&mut positionals, other, 2)?,
        }
    }
    let (path, selector) = two_paths(&positionals)?;
    Ok(Command::Get {
        input: input(&path),
        selector,
        environment,
        strict_variables,
    })
}

fn parse_variables(mut parser: Parser) -> Result<Command, CliError> {
    let mut positionals = Vec::new();
    let mut environment = None;
    while let Some(argument) = parser.bump() {
        match argument.as_str() {
            "--environment" => parser.once(&mut environment, "--environment")?,
            other => push_positional(&mut positionals, other, 2)?,
        }
    }
    let (path, selector) = two_paths(&positionals)?;
    Ok(Command::Variables {
        input: input(&path),
        selector,
        environment,
    })
}

struct RunOptions {
    environment: Option<String>,
    output: Option<PathBuf>,
    variables: Vec<(String, String)>,
    strict_variables: bool,
    dry_run: bool,
    secret_provider_env: bool,
    expectations: Vec<StatusExpectation>,
}

fn parse_run(mut parser: Parser) -> Result<Command, CliError> {
    let mut positionals = Vec::new();
    let mut options = RunOptions {
        environment: None,
        output: None,
        variables: Vec::new(),
        strict_variables: false,
        dry_run: false,
        secret_provider_env: false,
        expectations: Vec::new(),
    };
    while let Some(argument) = parser.bump() {
        match argument.as_str() {
            "--environment" => parser.once(&mut options.environment, "--environment")?,
            "--output" => parser.once_path(&mut options.output, "--output")?,
            "--var" => options.variables.push(parser.variable()?),
            "--strict-variables" => {
                parser.flag(&mut options.strict_variables, "--strict-variables")?
            }
            "--dry-run" => parser.flag(&mut options.dry_run, "--dry-run")?,
            "--secret-provider" => {
                let value = parser
                    .bump()
                    .ok_or_else(|| CliError::invalid_arguments("--secret-provider requires env"))?;
                if value != "env" || options.secret_provider_env {
                    return Err(CliError::invalid_arguments(
                        "--secret-provider accepts env once",
                    ));
                }
                options.secret_provider_env = true;
            }
            "--expect" => options.expectations.push(parser.expectation()?),
            other => push_positional(&mut positionals, other, 2)?,
        }
    }
    if options.dry_run && options.output.is_some() {
        return Err(CliError::invalid_arguments(
            "--dry-run cannot be combined with --output",
        ));
    }
    if options.dry_run && !options.expectations.is_empty() {
        return Err(CliError::invalid_arguments(
            "--expect cannot be combined with --dry-run",
        ));
    }
    let (path, selector) = two_paths(&positionals)?;
    Ok(Command::Run {
        input: input(&path),
        selector,
        environment: options.environment,
        variables: options.variables,
        output: options.output,
        strict_variables: options.strict_variables,
        dry_run: options.dry_run,
        secret_provider_env: options.secret_provider_env,
        expectations: options.expectations,
    })
}

fn parse_request_set(mut parser: Parser) -> Result<Command, CliError> {
    let mut positionals = Vec::new();
    let mut fields = RequestFields::default();
    while let Some(argument) = parser.bump() {
        match argument.as_str() {
            "--name"
            | "--method"
            | "--url"
            | "--graphql-query"
            | "--graphql-variables"
            | "--graphql-operation-name"
            | "--graphql-extensions" => {
                fields.take(&argument, &mut parser)?;
            }
            other => push_positional(&mut positionals, other, 2)?,
        }
    }
    let (path, selector) = two_paths(&positionals)?;
    let update = fields.update()?;
    if update.is_empty() {
        return Err(invalid_command());
    }
    Ok(Command::Set {
        input: input(&path),
        selector,
        update: Box::new(update),
    })
}

struct RequestCreateOptions {
    parent: Option<String>,
    index: Option<usize>,
    request_type: Option<String>,
    fields: RequestFields,
}

fn parse_request_create(mut parser: Parser) -> Result<Command, CliError> {
    let mut path = Vec::new();
    let mut options = RequestCreateOptions {
        parent: None,
        index: None,
        request_type: None,
        fields: RequestFields::default(),
    };
    while let Some(argument) = parser.bump() {
        match argument.as_str() {
            "--name"
            | "--method"
            | "--url"
            | "--graphql-query"
            | "--graphql-variables"
            | "--graphql-operation-name"
            | "--graphql-extensions" => {
                options.fields.take(&argument, &mut parser)?;
            }
            "--parent" => parser.once(&mut options.parent, "--parent")?,
            "--index" => parser.once_index(&mut options.index)?,
            "--type" => parser.once(&mut options.request_type, "--type")?,
            other => push_positional(&mut path, other, 1)?,
        }
    }
    let path = one_path(&path)?;
    let protocol = created_request_protocol(options.request_type.as_deref(), &options.fields)?;
    let update = options.fields.update()?;
    let name = update.name.ok_or_else(invalid_command)?;
    Ok(structure(
        input(&path),
        "create",
        StructureOperation::CreateRequest {
            parent: options.parent,
            index: options.index,
            name,
            method: update.method.into_set(),
            url: update.url.into_set(),
            protocol,
            graphql: update.graphql,
            update: None,
        },
    ))
}

fn parse_folder_create(mut parser: Parser) -> Result<Command, CliError> {
    let mut path = Vec::new();
    let mut name = None;
    let mut parent = None;
    let mut index = None;
    while let Some(argument) = parser.bump() {
        match argument.as_str() {
            "--name" => parser.once(&mut name, "--name")?,
            "--parent" => parser.once(&mut parent, "--parent")?,
            "--index" => parser.once_index(&mut index)?,
            other => push_positional(&mut path, other, 1)?,
        }
    }
    Ok(structure(
        input(&one_path(&path)?),
        "create",
        StructureOperation::CreateFolder {
            parent,
            index,
            name: name.ok_or_else(invalid_command)?,
        },
    ))
}

fn parse_item(mut parser: Parser, request: bool, action: &str) -> Result<Command, CliError> {
    let mut positionals = Vec::new();
    let mut name = None;
    let mut parent = None;
    let mut index = None;
    while let Some(argument) = parser.bump() {
        match (action, argument.as_str()) {
            ("rename", "--name") => parser.once(&mut name, "--name")?,
            ("move", "--parent") => parser.once(&mut parent, "--parent")?,
            ("move" | "reorder", "--index") => parser.once_index(&mut index)?,
            (_, other) => push_positional(&mut positionals, other, 2)?,
        }
    }
    let (path, selector) = two_paths(&positionals)?;
    let name = name.ok_or_else(invalid_command);
    let index = index.ok_or_else(invalid_command);
    let operation = match (request, action) {
        (true, "rename") => StructureOperation::RenameRequest {
            selector,
            name: name?,
        },
        (false, "rename") => StructureOperation::RenameFolder {
            selector,
            name: name?,
        },
        (true, "delete") => StructureOperation::DeleteRequest { selector },
        (false, "delete") => StructureOperation::DeleteFolder { selector },
        (true, "move") => StructureOperation::MoveRequest {
            selector,
            parent,
            index: index.ok(),
        },
        (false, "move") => StructureOperation::MoveFolder {
            selector,
            parent,
            index: index.ok(),
        },
        (true, "reorder") => StructureOperation::ReorderRequest {
            selector,
            index: index?,
        },
        (false, "reorder") => StructureOperation::ReorderFolder {
            selector,
            index: index?,
        },
        _ => return Err(invalid_command()),
    };
    let operation_name = match action {
        "rename" => "rename",
        "delete" => "delete",
        "move" => "move",
        "reorder" => "reorder",
        _ => return Err(invalid_command()),
    };
    Ok(structure(input(&path), operation_name, operation))
}

fn parse_environment_create(mut parser: Parser) -> Result<Command, CliError> {
    let mut path = Vec::new();
    let mut name = None;
    let mut extends = None;
    while let Some(argument) = parser.bump() {
        match argument.as_str() {
            "--name" => parser.once(&mut name, "--name")?,
            "--extends" => parser.once(&mut extends, "--extends")?,
            other => push_positional(&mut path, other, 1)?,
        }
    }
    Ok(Command::EnvironmentCreate {
        input: input(&one_path(&path)?),
        name: name.ok_or_else(invalid_command)?,
        extends,
    })
}

struct EnvironmentSetOptions {
    environment: Option<String>,
    name: Option<String>,
    value: Option<String>,
}

fn parse_environment_set(mut parser: Parser) -> Result<Command, CliError> {
    let mut path = Vec::new();
    let mut options = EnvironmentSetOptions {
        environment: None,
        name: None,
        value: None,
    };
    while let Some(argument) = parser.bump() {
        match argument.as_str() {
            "--environment" => parser.once(&mut options.environment, "--environment")?,
            "--name" => parser.once(&mut options.name, "--name")?,
            "--value" => parser.once(&mut options.value, "--value")?,
            other => push_positional(&mut path, other, 1)?,
        }
    }
    Ok(Command::EnvironmentSet {
        input: input(&one_path(&path)?),
        environment: options.environment.ok_or_else(invalid_command)?,
        name: options.name.ok_or_else(invalid_command)?,
        value: options.value.ok_or_else(invalid_command)?,
    })
}

fn parse_environment_unset(mut parser: Parser) -> Result<Command, CliError> {
    let mut path = Vec::new();
    let mut environment = None;
    let mut name = None;
    while let Some(argument) = parser.bump() {
        match argument.as_str() {
            "--environment" => parser.once(&mut environment, "--environment")?,
            "--name" => parser.once(&mut name, "--name")?,
            other => push_positional(&mut path, other, 1)?,
        }
    }
    Ok(Command::EnvironmentUnset {
        input: input(&one_path(&path)?),
        environment: environment.ok_or_else(invalid_command)?,
        name: name.ok_or_else(invalid_command)?,
    })
}

fn parse_environment_delete(mut parser: Parser) -> Result<Command, CliError> {
    let mut path = Vec::new();
    let mut environment = None;
    while let Some(argument) = parser.bump() {
        match argument.as_str() {
            "--environment" => parser.once(&mut environment, "--environment")?,
            other => push_positional(&mut path, other, 1)?,
        }
    }
    Ok(Command::EnvironmentDelete {
        input: input(&one_path(&path)?),
        environment: environment.ok_or_else(invalid_command)?,
    })
}

fn parse_environment_rename(mut parser: Parser) -> Result<Command, CliError> {
    let mut path = Vec::new();
    let mut environment = None;
    let mut name = None;
    while let Some(argument) = parser.bump() {
        match argument.as_str() {
            "--environment" => parser.once(&mut environment, "--environment")?,
            "--name" => parser.once(&mut name, "--name")?,
            other => push_positional(&mut path, other, 1)?,
        }
    }
    Ok(Command::EnvironmentRename {
        input: input(&one_path(&path)?),
        environment: environment.ok_or_else(invalid_command)?,
        name: name.ok_or_else(invalid_command)?,
    })
}

#[derive(Default)]
struct RequestFields {
    name: Option<String>,
    method: Option<String>,
    url: Option<String>,
    graphql_query: Option<String>,
    graphql_variables: Option<String>,
    graphql_operation_name: Option<String>,
    graphql_extensions: Option<String>,
}

impl RequestFields {
    fn take(&mut self, option: &str, parser: &mut Parser) -> Result<(), CliError> {
        let slot = match option {
            "--name" => &mut self.name,
            "--method" => &mut self.method,
            "--url" => &mut self.url,
            "--graphql-query" => &mut self.graphql_query,
            "--graphql-variables" => &mut self.graphql_variables,
            "--graphql-operation-name" => &mut self.graphql_operation_name,
            "--graphql-extensions" => &mut self.graphql_extensions,
            _ => return Err(invalid_command()),
        };
        parser.once(slot, option)
    }

    fn graphql_requested(&self) -> bool {
        self.graphql_query.is_some()
            || self.graphql_variables.is_some()
            || self.graphql_operation_name.is_some()
            || self.graphql_extensions.is_some()
    }

    fn update(self) -> Result<RequestUpdate, CliError> {
        let graphql_requested = self.graphql_requested();
        let mut update = RequestUpdate {
            name: self.name,
            method: self.method.map(FieldPatch::Set).unwrap_or_default(),
            url: self.url.map(FieldPatch::Set).unwrap_or_default(),
            ..RequestUpdate::default()
        };
        if !graphql_requested {
            return Ok(update);
        }
        update.graphql = Some(GraphqlUpdate {
            query: self.graphql_query.map(FieldPatch::Set).unwrap_or_default(),
            variables: self
                .graphql_variables
                .as_deref()
                .map(|source| parse_graphql_object(source, "variables"))
                .transpose()?
                .map(FieldPatch::from_optional)
                .unwrap_or_default(),
            operation_name: self
                .graphql_operation_name
                .as_deref()
                .map(|source| parse_graphql_string(source, "operation name"))
                .transpose()?
                .map(FieldPatch::from_optional)
                .unwrap_or_default(),
            extensions: self
                .graphql_extensions
                .as_deref()
                .map(|source| parse_graphql_object(source, "extensions"))
                .transpose()?
                .map(FieldPatch::from_optional)
                .unwrap_or_default(),
        });
        Ok(update)
    }
}

fn created_request_protocol(
    request_type: Option<&str>,
    fields: &RequestFields,
) -> Result<CreatedRequestProtocol, CliError> {
    match request_type {
        None if fields.graphql_requested() => Ok(CreatedRequestProtocol::Graphql),
        None | Some("http") => {
            if fields.graphql_requested() {
                Err(CliError::invalid_arguments(
                    "GraphQL fields cannot be applied to an HTTP request",
                ))
            } else {
                Ok(CreatedRequestProtocol::Http)
            }
        }
        Some("graphql") => Ok(CreatedRequestProtocol::Graphql),
        Some(_) => Err(CliError::invalid_arguments(
            "--type must be http or graphql",
        )),
    }
}

fn parse_graphql_object(source: &str, field: &str) -> Result<Option<Map<String, Value>>, CliError> {
    let value: Value = serde_json::from_str(source).map_err(|_| {
        CliError::invalid_arguments(format!("GraphQL {field} must be a JSON object or null"))
    })?;
    match value {
        Value::Null => Ok(None),
        Value::Object(value) => Ok(Some(value)),
        _ => Err(CliError::invalid_arguments(format!(
            "GraphQL {field} must be a JSON object or null"
        ))),
    }
}

fn parse_graphql_string(source: &str, field: &str) -> Result<Option<String>, CliError> {
    let value: Value = serde_json::from_str(source).map_err(|_| {
        CliError::invalid_arguments(format!("GraphQL {field} must be a JSON string or null"))
    })?;
    match value {
        Value::Null => Ok(None),
        Value::String(value) => Ok(Some(value)),
        _ => Err(CliError::invalid_arguments(format!(
            "GraphQL {field} must be a JSON string or null"
        ))),
    }
}

struct Parser {
    args: Peekable<IntoIter<String>>,
}

impl Parser {
    fn new(args: Vec<String>) -> Self {
        Self {
            args: args.into_iter().peekable(),
        }
    }

    fn peek(&mut self) -> Option<&str> {
        self.args.peek().map(String::as_str)
    }

    fn bump(&mut self) -> Option<String> {
        self.args.next()
    }

    fn command_word(&mut self) -> Result<String, CliError> {
        let accept = self.peek().is_some_and(|word| !is_known_option(word));
        if accept {
            Ok(self.bump().expect("peeked argument"))
        } else {
            Err(invalid_command())
        }
    }

    /// Reads the next token as an option value.
    ///
    /// A known Probe option is left unconsumed and reported as a missing value.
    /// An unknown dash-prefixed token is a literal value.
    fn value(&mut self, option: &str) -> Result<String, CliError> {
        let missing = match self.peek() {
            Some(next) if next.is_empty() || is_known_option(next) => true,
            Some(_) => false,
            None => true,
        };
        if missing {
            Err(missing_value(option))
        } else {
            Ok(self.bump().expect("peeked argument"))
        }
    }

    fn index_value(&mut self) -> Result<usize, CliError> {
        self.value("--index")?
            .parse()
            .map_err(|_| CliError::invalid_arguments("--index requires a non-negative integer"))
    }

    /// `--var` consumes the next token even when that token is a known option.
    fn variable(&mut self) -> Result<(String, String), CliError> {
        let argument = self.bump().ok_or_else(invalid_variable)?;
        let Some((name, value)) = argument.split_once('=') else {
            return Err(invalid_variable());
        };
        if name.is_empty() {
            return Err(invalid_variable());
        }
        Ok((name.to_owned(), value.to_owned()))
    }

    fn expectation(&mut self) -> Result<StatusExpectation, CliError> {
        let argument = self.value("--expect").map_err(|_| invalid_expectation())?;
        StatusExpectation::parse(&argument).map_err(CliError::expectation)
    }

    fn once(&mut self, slot: &mut Option<String>, option: &str) -> Result<(), CliError> {
        if slot.is_some() {
            return Err(duplicate_option(option));
        }
        *slot = Some(self.value(option)?);
        Ok(())
    }

    fn once_path(&mut self, slot: &mut Option<PathBuf>, option: &str) -> Result<(), CliError> {
        if slot.is_some() {
            return Err(duplicate_option(option));
        }
        *slot = Some(PathBuf::from(self.value(option)?));
        Ok(())
    }

    fn once_index(&mut self, slot: &mut Option<usize>) -> Result<(), CliError> {
        if slot.is_some() {
            return Err(duplicate_option("--index"));
        }
        *slot = Some(self.index_value()?);
        Ok(())
    }

    fn flag(&mut self, slot: &mut bool, option: &str) -> Result<(), CliError> {
        if *slot {
            return Err(duplicate_option(option));
        }
        *slot = true;
        Ok(())
    }
}

fn workspace(mut parser: Parser) -> Result<WorkspaceInput, CliError> {
    let mut path = Vec::new();
    while let Some(argument) = parser.bump() {
        push_positional(&mut path, &argument, 1)?;
    }
    Ok(input(&one_path(&path)?))
}

fn push_positional(
    positionals: &mut Vec<String>,
    argument: &str,
    expected: usize,
) -> Result<(), CliError> {
    if is_known_option(argument) {
        return Err(unexpected_option(argument));
    }
    if positionals.len() >= expected {
        return Err(invalid_command());
    }
    positionals.push(argument.to_owned());
    Ok(())
}

fn one_path(positionals: &[String]) -> Result<String, CliError> {
    match positionals {
        [path] => Ok(path.clone()),
        _ => Err(invalid_command()),
    }
}

fn two_paths(positionals: &[String]) -> Result<(String, String), CliError> {
    match positionals {
        [path, selector] => Ok((path.clone(), selector.clone())),
        _ => Err(invalid_command()),
    }
}

fn reject_stdin(path: &str) -> Result<(), CliError> {
    if path == "-" {
        Err(invalid_command())
    } else {
        Ok(())
    }
}

/// Probe option names. Used only to keep a value reader from consuming an option token.
/// Unknown dash-prefixed tokens are literal values or positionals, not options.
fn is_known_option(argument: &str) -> bool {
    matches!(
        argument,
        "--environment"
            | "--output"
            | "--name"
            | "--method"
            | "--url"
            | "--parent"
            | "--index"
            | "--value"
            | "--extends"
            | "--workspace"
            | "--allow-partial"
            | "--var"
            | "--strict-variables"
            | "--graphql-query"
            | "--graphql-variables"
            | "--graphql-operation-name"
            | "--graphql-extensions"
            | "--type"
            | "--dry-run"
            | "--expect"
            | "--json"
            | "-q"
            | "--quiet"
            | "-h"
            | "--help"
            | "-V"
            | "--version"
    )
}

fn unexpected_option(option: &str) -> CliError {
    match option {
        "--extends" => {
            CliError::invalid_arguments("--extends is only valid for environment create")
        }
        "--workspace" => CliError::invalid_arguments("--workspace is only valid for Yaak import"),
        "--allow-partial" => {
            CliError::invalid_arguments("--allow-partial is only valid for collection import")
        }
        "--type" => CliError::invalid_arguments("--type is only valid for request create"),
        _ => invalid_command(),
    }
}

fn input(path: &str) -> WorkspaceInput {
    WorkspaceInput::from_argument(path)
}

fn structure(
    input: WorkspaceInput,
    operation_name: &'static str,
    operation: StructureOperation,
) -> Command {
    Command::Structure {
        input,
        operation_name,
        operation: Box::new(operation),
    }
}

fn missing_value(option: &str) -> CliError {
    CliError::invalid_arguments(format!("{option} requires a non-empty value"))
}

fn invalid_expectation() -> CliError {
    CliError::invalid_arguments("--expect requires status=<code> or status=<code|code>")
}

fn invalid_variable() -> CliError {
    CliError::invalid_arguments("--var requires NAME=VALUE with a non-empty name")
}

fn duplicate_option(option: &str) -> CliError {
    CliError::invalid_arguments(format!("{option} may only be specified once"))
}

fn invalid_command() -> CliError {
    CliError::invalid_arguments("invalid command; run 'probe --help' for usage")
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn duplicate_index_is_reported_before_the_second_value_is_parsed() {
        let error = parse(vec![
            "request".into(),
            "reorder".into(),
            "collection.yml".into(),
            "items/0".into(),
            "--index".into(),
            "1".into(),
            "--index".into(),
            "nope".into(),
        ])
        .expect_err("duplicate --index should be rejected");
        assert_eq!(error.message, "--index may only be specified once");
    }
}
