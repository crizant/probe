use std::{iter::Peekable, path::PathBuf, vec::IntoIter};

use probe_application::{SessionData, WebSocketInputFormat};
use probe_core::{
    Assertion, CollectionUpdate, Documentation, FieldPatch, FolderUpdate, GraphqlUpdate, ItemKind,
    RequestProtocol, RequestUpdate,
};
use probe_opencollection::{ItemLocator, StructureOperation};
use serde_json::{Map, Value};

use crate::{
    CliError, WorkspaceInput,
    request_input::{
        HttpBodyWrite, parse_authentication, parse_headers, parse_http_body, parse_path_parameters,
        parse_query_parameters,
    },
};

#[derive(Debug)]
pub(crate) enum Command {
    InstallAgentSkill {
        force: bool,
    },
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
    ImportCurl {
        input: WorkspaceInput,
        name: String,
        parent: Option<String>,
        index: Option<usize>,
        file: Option<PathBuf>,
    },
    Validate {
        input: WorkspaceInput,
    },
    GetCollection {
        input: WorkspaceInput,
    },
    SetCollection {
        input: WorkspaceInput,
        update: CollectionUpdate,
    },
    UnsetCollection {
        input: WorkspaceInput,
        update: CollectionUpdate,
    },
    GetFolder {
        input: WorkspaceInput,
        selector: String,
    },
    SetFolder {
        input: WorkspaceInput,
        selector: String,
        update: FolderUpdate,
        description: DescriptionEdit,
    },
    UnsetFolder {
        input: WorkspaceInput,
        selector: String,
        update: FolderUpdate,
    },
    UnsetRequest {
        input: WorkspaceInput,
        selector: String,
        update: Box<RequestUpdate>,
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
        show_headers: bool,
        secret_provider_env: bool,
        assertions: Vec<Assertion>,
        no_assertions: bool,
        sends: Vec<SessionData>,
        stdin_format: Option<WebSocketInputFormat>,
        timeout: Option<u64>,
        max_messages: Option<u64>,
    },
    Set {
        input: WorkspaceInput,
        selector: String,
        update: Box<RequestUpdate>,
        description: DescriptionEdit,
    },
    Structure {
        input: WorkspaceInput,
        operation_name: &'static str,
        operation: Box<StructureOperation>,
    },
    EnvironmentSet {
        input: WorkspaceInput,
        environment: String,
        variable: Option<(String, String)>,
        description: DescriptionEdit,
    },
    EnvironmentUnset {
        input: WorkspaceInput,
        environment: String,
        name: Option<String>,
        clear_description: bool,
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

#[derive(Debug)]
pub(crate) enum DescriptionEdit {
    Unchanged,
    Content(String),
    Replace(Documentation),
}

impl DescriptionEdit {
    pub(crate) fn is_unchanged(&self) -> bool {
        matches!(self, Self::Unchanged)
    }

    pub(crate) fn resolve(&self, existing: Option<&Documentation>) -> FieldPatch<Documentation> {
        match self {
            Self::Unchanged => FieldPatch::Unchanged,
            Self::Content(text) => {
                FieldPatch::Set(Documentation::edited_content(existing, text.clone()))
            }
            Self::Replace(value) => FieldPatch::Set(value.clone()),
        }
    }
}

pub(crate) fn parse(args: Vec<String>) -> Result<Command, CliError> {
    let mut parser = Parser::new(args);
    let group = parser.command_word()?;
    let action = parser.command_word()?;
    match (group.as_str(), action.as_str()) {
        ("agent", "skill") => parse_agent_skill(parser),
        ("collection", "create") => parse_collection_create(parser),
        ("collection", "import") => parse_import(parser),
        ("collection", "validate") => Ok(Command::Validate {
            input: workspace(parser)?,
        }),
        ("collection", "get") => Ok(Command::GetCollection {
            input: workspace(parser)?,
        }),
        ("collection", "set") => parse_collection_set(parser),
        ("collection", "unset") => parse_collection_unset(parser),
        ("request", "list") => Ok(Command::ListRequests {
            input: workspace(parser)?,
        }),
        ("folder", "list") => Ok(Command::ListFolders {
            input: workspace(parser)?,
        }),
        ("folder", "get") => parse_folder_get(parser),
        ("folder", "set") => parse_folder_set(parser),
        ("folder", "unset") => parse_folder_unset(parser),
        ("request", "unset") => parse_request_unset(parser),
        ("environment", "list") => Ok(Command::ListEnvironments {
            input: workspace(parser)?,
        }),
        ("request", "get") => parse_get(parser),
        ("request", "variables") => parse_variables(parser),
        ("request", "run") => parse_run(parser),
        ("request", "set") => parse_request_set(parser),
        ("request", "create") => parse_request_create(parser),
        ("request", "import") => parse_curl_import(parser),
        ("folder", "create") => parse_folder_create(parser),
        ("request", "rename" | "delete" | "move" | "reorder") => {
            parse_item(parser, ItemKind::Request, &action)
        }
        ("folder", "rename" | "delete" | "move" | "reorder") => {
            parse_item(parser, ItemKind::Folder, &action)
        }
        ("environment", "create") => parse_environment_create(parser),
        ("environment", "set") => parse_environment_set(parser),
        ("environment", "unset") => parse_environment_unset(parser),
        ("environment", "delete") => parse_environment_delete(parser),
        ("environment", "rename") => parse_environment_rename(parser),
        _ => Err(invalid_command()),
    }
}

fn parse_agent_skill(mut parser: Parser) -> Result<Command, CliError> {
    match parser.command_word()?.as_str() {
        "install" => parse_agent_skill_install(parser),
        _ => Err(invalid_command()),
    }
}

fn parse_agent_skill_install(mut parser: Parser) -> Result<Command, CliError> {
    let mut force = false;
    while let Some(argument) = parser.bump() {
        match argument.as_str() {
            "--force" => parser.flag(&mut force, "--force")?,
            other => {
                return Err(CliError::invalid_arguments(format!(
                    "unexpected argument: {other}"
                )));
            }
        }
    }
    Ok(Command::InstallAgentSkill { force })
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
    show_headers: bool,
    secret_provider_env: bool,
    assertions: Vec<Assertion>,
    no_assertions: bool,
    sends: Vec<SessionData>,
    stdin_format: Option<WebSocketInputFormat>,
    timeout: Option<u64>,
    max_messages: Option<u64>,
}

fn parse_run(mut parser: Parser) -> Result<Command, CliError> {
    let mut positionals = Vec::new();
    let mut binary_position = 0;
    let mut options = RunOptions {
        environment: None,
        output: None,
        variables: Vec::new(),
        strict_variables: false,
        dry_run: false,
        show_headers: false,
        secret_provider_env: false,
        assertions: Vec::new(),
        no_assertions: false,
        sends: Vec::new(),
        stdin_format: None,
        timeout: None,
        max_messages: None,
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
            "--show-headers" => parser.flag(&mut options.show_headers, "--show-headers")?,
            "--secret-provider" => {
                let value = parser.value("--secret-provider")?;
                if value != "env" || options.secret_provider_env {
                    return Err(CliError::invalid_arguments(
                        "--secret-provider accepts env once",
                    ));
                }
                options.secret_provider_env = true;
            }
            "--assert" => options.assertions.push(parser.assertion()?),
            "--no-assertions" => parser.flag(&mut options.no_assertions, "--no-assertions")?,
            "--send" => options.sends.push(SessionData::Text(
                parser
                    .bump()
                    .ok_or_else(|| CliError::invalid_arguments("--send requires text"))?,
            )),
            "--send-binary" => {
                binary_position += 1;
                let value = parser.bump().ok_or_else(|| {
                    CliError::invalid_arguments("--send-binary requires standard Base64")
                })?;
                let data = WebSocketInputFormat::Binary.decode(&value).map_err(|_| {
                    CliError::invalid_arguments(format!(
                        "--send-binary value #{binary_position} requires valid standard Base64"
                    ))
                })?;
                options.sends.push(data);
            }
            "--stdin-format" => {
                if options.stdin_format.is_some() {
                    return Err(duplicate_option("--stdin-format"));
                }
                options.stdin_format = Some(match parser.value("--stdin-format")?.as_str() {
                    "text" => WebSocketInputFormat::Text,
                    "binary" => WebSocketInputFormat::Binary,
                    _ => {
                        return Err(CliError::invalid_arguments(
                            "--stdin-format accepts text or binary",
                        ));
                    }
                });
            }
            "--timeout" => {
                options.timeout = Some(positive_option(&mut parser, options.timeout, "--timeout")?)
            }
            "--max-messages" => {
                options.max_messages = Some(positive_option(
                    &mut parser,
                    options.max_messages,
                    "--max-messages",
                )?)
            }
            other => push_positional(&mut positionals, other, 2)?,
        }
    }
    if options.dry_run && options.output.is_some() {
        return Err(CliError::invalid_arguments(
            "--dry-run cannot be combined with --output",
        ));
    }
    if options.dry_run && !options.assertions.is_empty() {
        return Err(CliError::invalid_arguments(
            "--assert cannot be combined with --dry-run",
        ));
    }
    if options.dry_run
        && (!options.sends.is_empty()
            || options.stdin_format.is_some()
            || options.timeout.is_some()
            || options.max_messages.is_some())
    {
        return Err(CliError::invalid_arguments(
            "--send, --send-binary, --stdin-format, --timeout and --max-messages require a live WebSocket session",
        ));
    }
    let (path, selector) = two_paths(&positionals)?;
    if path == "-" && options.stdin_format.is_some() {
        return Err(CliError::invalid_arguments(
            "--stdin-format cannot be used with workspace path '-': stdin is consumed by the workspace",
        ));
    }
    Ok(Command::Run {
        input: input(&path),
        selector,
        environment: options.environment,
        variables: options.variables,
        output: options.output,
        strict_variables: options.strict_variables,
        dry_run: options.dry_run,
        show_headers: options.show_headers,
        secret_provider_env: options.secret_provider_env,
        assertions: options.assertions,
        no_assertions: options.no_assertions,
        sends: options.sends,
        stdin_format: options.stdin_format,
        timeout: options.timeout,
        max_messages: options.max_messages,
    })
}

fn positive_option(
    parser: &mut Parser,
    previous: Option<u64>,
    name: &str,
) -> Result<u64, CliError> {
    if previous.is_some() {
        return Err(CliError::invalid_arguments(format!(
            "{name} may only be specified once"
        )));
    }
    parser
        .value(name)?
        .parse::<u64>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| CliError::invalid_arguments(format!("{name} requires a positive integer")))
}

fn parse_request_set(mut parser: Parser) -> Result<Command, CliError> {
    let mut positionals = Vec::new();
    let mut fields = RequestFields::default();
    let mut description = None;
    let mut description_json = None;
    let mut docs = None;
    while let Some(argument) = parser.bump() {
        match argument.as_str() {
            "--name"
            | "--method"
            | "--url"
            | "--headers"
            | "--query-parameters"
            | "--path-parameters"
            | "--body"
            | "--auth"
            | "--graphql-query"
            | "--graphql-variables"
            | "--graphql-operation-name"
            | "--graphql-extensions" => {
                fields.take(&argument, &mut parser)?;
            }
            "--description" => parser.once(&mut description, "--description")?,
            "--description-json" => parser.once(&mut description_json, "--description-json")?,
            "--docs" => parser.once(&mut docs, "--docs")?,
            "--docs-json" => {
                return Err(CliError::invalid_arguments(
                    "--docs-json is not supported for request docs; use --docs <text>",
                ));
            }
            other => push_positional(&mut positionals, other, 2)?,
        }
    }
    let (path, selector) = two_paths(&positionals)?;
    let mut update = fields.update()?;
    let description = description_edit(description, description_json)?;
    update.docs = docs.map(FieldPatch::Set).unwrap_or_default();
    if update.is_empty() && description.is_unchanged() {
        return Err(invalid_command());
    }
    Ok(Command::Set {
        input: input(&path),
        selector,
        update: Box::new(update),
        description,
    })
}

fn parse_collection_set(mut parser: Parser) -> Result<Command, CliError> {
    let mut path = Vec::new();
    let mut summary = None;
    let mut docs = None;
    let mut docs_json = None;
    while let Some(argument) = parser.bump() {
        match argument.as_str() {
            "--summary" => parser.once(&mut summary, "--summary")?,
            "--docs" => parser.once(&mut docs, "--docs")?,
            "--docs-json" => parser.once(&mut docs_json, "--docs-json")?,
            "--description" | "--description-json" => {
                return Err(CliError::invalid_arguments(
                    "collections have summary and docs, not description",
                ));
            }
            other => push_positional(&mut path, other, 1)?,
        }
    }
    let update = CollectionUpdate {
        summary: summary.map(FieldPatch::Set).unwrap_or_default(),
        docs: documentation_patch(docs, docs_json, "docs")?,
    };
    if update.is_empty() {
        return Err(invalid_command());
    }
    Ok(Command::SetCollection {
        input: input(&one_path(&path)?),
        update,
    })
}

fn parse_folder_get(mut parser: Parser) -> Result<Command, CliError> {
    let mut positionals = Vec::new();
    while let Some(argument) = parser.bump() {
        push_positional(&mut positionals, &argument, 2)?;
    }
    let (path, selector) = two_paths(&positionals)?;
    Ok(Command::GetFolder {
        input: input(&path),
        selector,
    })
}

fn parse_folder_set(mut parser: Parser) -> Result<Command, CliError> {
    let mut positionals = Vec::new();
    let mut description = None;
    let mut description_json = None;
    let mut docs = None;
    let mut docs_json = None;
    while let Some(argument) = parser.bump() {
        match argument.as_str() {
            "--description" => parser.once(&mut description, "--description")?,
            "--description-json" => parser.once(&mut description_json, "--description-json")?,
            "--docs" => parser.once(&mut docs, "--docs")?,
            "--docs-json" => parser.once(&mut docs_json, "--docs-json")?,
            "--summary" => {
                return Err(CliError::invalid_arguments(
                    "folders have description and docs, not summary",
                ));
            }
            other => push_positional(&mut positionals, other, 2)?,
        }
    }
    let (path, selector) = two_paths(&positionals)?;
    let description = description_edit(description, description_json)?;
    let update = FolderUpdate {
        description: FieldPatch::Unchanged,
        docs: documentation_patch(docs, docs_json, "docs")?,
    };
    if update.is_empty() && description.is_unchanged() {
        return Err(invalid_command());
    }
    Ok(Command::SetFolder {
        input: input(&path),
        selector,
        update,
        description,
    })
}

fn parse_collection_unset(mut parser: Parser) -> Result<Command, CliError> {
    let mut path = Vec::new();
    let mut summary = false;
    let mut docs = false;
    while let Some(argument) = parser.bump() {
        match argument.as_str() {
            "--summary" => parser.flag(&mut summary, "--summary")?,
            "--docs" => parser.flag(&mut docs, "--docs")?,
            "--docs-json" => return Err(unset_rejects_json_null()),
            "--description" | "--description-json" => {
                return Err(CliError::invalid_arguments(
                    "collections have summary and docs; use unset --summary or unset --docs",
                ));
            }
            other => push_positional(&mut path, other, 1)?,
        }
    }
    if !summary && !docs {
        return Err(invalid_command());
    }
    Ok(Command::UnsetCollection {
        input: input(&one_path(&path)?),
        update: CollectionUpdate {
            summary: clear_if(summary),
            docs: clear_if(docs),
        },
    })
}

fn parse_folder_unset(mut parser: Parser) -> Result<Command, CliError> {
    let mut positionals = Vec::new();
    let mut description = false;
    let mut docs = false;
    while let Some(argument) = parser.bump() {
        match argument.as_str() {
            "--description" => parser.flag(&mut description, "--description")?,
            "--docs" => parser.flag(&mut docs, "--docs")?,
            "--docs-json" | "--description-json" => return Err(unset_rejects_json_null()),
            "--summary" => {
                return Err(CliError::invalid_arguments(
                    "folders have description and docs; use unset --description or unset --docs",
                ));
            }
            other => push_positional(&mut positionals, other, 2)?,
        }
    }
    if !description && !docs {
        return Err(invalid_command());
    }
    let (path, selector) = two_paths(&positionals)?;
    Ok(Command::UnsetFolder {
        input: input(&path),
        selector,
        update: FolderUpdate {
            description: clear_if(description),
            docs: clear_if(docs),
        },
    })
}

fn parse_request_unset(mut parser: Parser) -> Result<Command, CliError> {
    let mut positionals = Vec::new();
    let mut description = false;
    let mut docs = false;
    while let Some(argument) = parser.bump() {
        match argument.as_str() {
            "--description" => parser.flag(&mut description, "--description")?,
            "--docs" => parser.flag(&mut docs, "--docs")?,
            "--docs-json" => {
                return Err(CliError::invalid_arguments(
                    "request docs must be a string; use unset --docs to remove the field",
                ));
            }
            "--description-json" => return Err(unset_rejects_json_null()),
            "--summary" => {
                return Err(CliError::invalid_arguments(
                    "requests have description and docs; use unset --description or unset --docs",
                ));
            }
            other => push_positional(&mut positionals, other, 2)?,
        }
    }
    if !description && !docs {
        return Err(invalid_command());
    }
    let (path, selector) = two_paths(&positionals)?;
    Ok(Command::UnsetRequest {
        input: input(&path),
        selector,
        update: Box::new(RequestUpdate {
            description: clear_if(description),
            docs: clear_if(docs),
            ..RequestUpdate::default()
        }),
    })
}

fn clear_if<T>(clear: bool) -> FieldPatch<T> {
    if clear {
        FieldPatch::Clear
    } else {
        FieldPatch::Unchanged
    }
}

fn unset_rejects_json_null() -> CliError {
    CliError::invalid_arguments(
        "unset removes the field; use set --docs-json null or set --description-json null to write YAML null",
    )
}

fn documentation_patch(
    text: Option<String>,
    json: Option<String>,
    flag: &str,
) -> Result<FieldPatch<Documentation>, CliError> {
    match (text, json) {
        (None, None) => Ok(FieldPatch::Unchanged),
        (Some(_), Some(_)) => Err(CliError::invalid_arguments(format!(
            "--{flag} and --{flag}-json cannot be combined"
        ))),
        (Some(text), None) => Ok(FieldPatch::Set(Documentation::Text(text))),
        (None, Some(source)) => Ok(FieldPatch::Set(parse_documentation_json(&source, flag)?)),
    }
}

fn description_edit(
    text: Option<String>,
    json: Option<String>,
) -> Result<DescriptionEdit, CliError> {
    match (text, json) {
        (None, None) => Ok(DescriptionEdit::Unchanged),
        (Some(_), Some(_)) => Err(CliError::invalid_arguments(
            "--description and --description-json cannot be combined",
        )),
        (Some(content), None) => Ok(DescriptionEdit::Content(content)),
        (None, Some(source)) => Ok(DescriptionEdit::Replace(parse_documentation_json(
            &source,
            "description",
        )?)),
    }
}

fn parse_documentation_json(source: &str, flag: &str) -> Result<Documentation, CliError> {
    let value: Value = serde_json::from_str(source).map_err(|_| documentation_json_error(flag))?;
    match value {
        Value::Null => Ok(Documentation::Null),
        Value::String(text) => Ok(Documentation::Text(text)),
        Value::Object(object) => {
            if object.len() != 2 {
                return Err(documentation_json_error(flag));
            }
            let content = match object.get("content") {
                Some(Value::String(content)) => content.clone(),
                _ => return Err(documentation_json_error(flag)),
            };
            let media_type = match object.get("type") {
                Some(Value::String(media_type)) => media_type.clone(),
                _ => return Err(documentation_json_error(flag)),
            };
            Ok(Documentation::Content {
                content,
                media_type,
            })
        }
        _ => Err(documentation_json_error(flag)),
    }
}

fn documentation_json_error(flag: &str) -> CliError {
    CliError::invalid_arguments(format!(
        "--{flag}-json must be a JSON string, null, or object with string content and type"
    ))
}

fn parse_curl_import(mut parser: Parser) -> Result<Command, CliError> {
    if parser.command_word()? != "curl" {
        return Err(invalid_command());
    }
    let mut paths = Vec::new();
    let mut name = None;
    let mut parent = None;
    let mut index = None;
    let mut file = None;
    while let Some(argument) = parser.bump() {
        match argument.as_str() {
            "--name" => parser.once(&mut name, "--name")?,
            "--parent" => parser.once(&mut parent, "--parent")?,
            "--index" => parser.once_index(&mut index)?,
            "--file" => parser.once(&mut file, "--file")?,
            other => push_positional(&mut paths, other, 1)?,
        }
    }
    let path = one_path(&paths)?;
    if path == "-" {
        return Err(CliError::invalid_arguments(
            "cURL import requires an existing workspace path",
        ));
    }
    let name = name
        .filter(|name: &String| !name.trim().is_empty())
        .ok_or_else(|| CliError::invalid_arguments("cURL import requires --name <name>"))?;
    Ok(Command::ImportCurl {
        input: input(&path),
        name,
        parent,
        index,
        file: file.map(PathBuf::from),
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
            | "--headers"
            | "--query-parameters"
            | "--path-parameters"
            | "--body"
            | "--auth"
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
    let protocol = request_protocol(options.request_type.as_deref(), &options.fields)?;
    let mut update = options.fields.update()?;
    if protocol == RequestProtocol::Graphql
        && (!update.body.is_unchanged() || !update.body_content.is_unchanged())
    {
        return Err(CliError::invalid_arguments(
            "HTTP body updates cannot be applied to a native GraphQL request",
        ));
    }
    let name = update.name.take().ok_or_else(invalid_command)?;
    let method = std::mem::take(&mut update.method).into_set();
    let url = std::mem::take(&mut update.url).into_set();
    let graphql = update.graphql.take();
    let update = (!update.is_empty()).then_some(update);
    Ok(structure(
        input(&path),
        "create",
        StructureOperation::CreateRequest {
            parent: options.parent,
            index: options.index,
            name,
            method,
            url,
            settings: None,
            protocol,
            graphql,
            update,
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

fn parse_item(mut parser: Parser, kind: ItemKind, action: &str) -> Result<Command, CliError> {
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
    let target = ItemLocator { kind, selector };
    let operation = match action {
        "rename" => StructureOperation::Rename {
            target,
            name: name?,
        },
        "delete" => StructureOperation::Delete { target },
        "move" => StructureOperation::Move {
            target,
            parent,
            index: index.ok(),
        },
        "reorder" => StructureOperation::Reorder {
            target,
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
    description: Option<String>,
    description_json: Option<String>,
}

fn parse_environment_set(mut parser: Parser) -> Result<Command, CliError> {
    let mut path = Vec::new();
    let mut options = EnvironmentSetOptions {
        environment: None,
        name: None,
        value: None,
        description: None,
        description_json: None,
    };
    while let Some(argument) = parser.bump() {
        match argument.as_str() {
            "--environment" => parser.once(&mut options.environment, "--environment")?,
            "--name" => parser.once(&mut options.name, "--name")?,
            "--value" => parser.once(&mut options.value, "--value")?,
            "--description" => parser.once(&mut options.description, "--description")?,
            "--description-json" => {
                parser.once(&mut options.description_json, "--description-json")?;
            }
            other => push_positional(&mut path, other, 1)?,
        }
    }
    let description = description_edit(options.description, options.description_json)?;
    let variable = match (options.name, options.value) {
        (Some(name), Some(value)) => Some((name, value)),
        (None, None) => None,
        _ => return Err(invalid_command()),
    };
    if variable.is_some() && !description.is_unchanged() {
        return Err(CliError::invalid_arguments(
            "environment set accepts either a variable (--name and --value) or a description",
        ));
    }
    if variable.is_none() && description.is_unchanged() {
        return Err(invalid_command());
    }
    Ok(Command::EnvironmentSet {
        input: input(&one_path(&path)?),
        environment: options.environment.ok_or_else(invalid_command)?,
        variable,
        description,
    })
}

fn parse_environment_unset(mut parser: Parser) -> Result<Command, CliError> {
    let mut path = Vec::new();
    let mut environment = None;
    let mut name = None;
    let mut description = false;
    while let Some(argument) = parser.bump() {
        match argument.as_str() {
            "--environment" => parser.once(&mut environment, "--environment")?,
            "--name" => parser.once(&mut name, "--name")?,
            "--description" => parser.flag(&mut description, "--description")?,
            "--description-json" => return Err(unset_rejects_json_null()),
            other => push_positional(&mut path, other, 1)?,
        }
    }
    if name.is_some() && description {
        return Err(CliError::invalid_arguments(
            "environment unset accepts either --name or --description",
        ));
    }
    if name.is_none() && !description {
        return Err(invalid_command());
    }
    Ok(Command::EnvironmentUnset {
        input: input(&one_path(&path)?),
        environment: environment.ok_or_else(invalid_command)?,
        name,
        clear_description: description,
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
    headers: Option<String>,
    query_parameters: Option<String>,
    path_parameters: Option<String>,
    body: Option<String>,
    auth: Option<String>,
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
            "--headers" => &mut self.headers,
            "--query-parameters" => &mut self.query_parameters,
            "--path-parameters" => &mut self.path_parameters,
            "--body" => &mut self.body,
            "--auth" => &mut self.auth,
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
            headers: self.headers.as_deref().map(parse_headers).transpose()?,
            query_parameters: self
                .query_parameters
                .as_deref()
                .map(parse_query_parameters)
                .transpose()?,
            path_parameters: self
                .path_parameters
                .as_deref()
                .map(parse_path_parameters)
                .transpose()?,
            authentication: self
                .auth
                .as_deref()
                .map(parse_authentication)
                .transpose()?
                .unwrap_or_default(),
            ..RequestUpdate::default()
        };
        if let Some(source) = self.body.as_deref() {
            match parse_http_body(source)? {
                HttpBodyWrite::Clear => update.body = FieldPatch::Clear,
                HttpBodyWrite::Content(body) => update.body_content = FieldPatch::Set(body),
            }
        }
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

fn request_protocol(
    request_type: Option<&str>,
    fields: &RequestFields,
) -> Result<RequestProtocol, CliError> {
    match request_type {
        None if fields.graphql_requested() => Ok(RequestProtocol::Graphql),
        None | Some("http") => {
            if fields.graphql_requested() {
                Err(CliError::invalid_arguments(
                    "GraphQL fields cannot be applied to an HTTP request",
                ))
            } else {
                Ok(RequestProtocol::Http)
            }
        }
        Some("graphql") => Ok(RequestProtocol::Graphql),
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

    fn assertion(&mut self) -> Result<Assertion, CliError> {
        let argument = self.value("--assert").map_err(|_| invalid_assertion())?;
        parse_assertion(&argument)
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
    if argument.starts_with("--expect=") {
        return Err(legacy_expect());
    }
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
            | "--secret-provider"
            | "--output"
            | "--file"
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
            | "--show-headers"
            | "--headers"
            | "--query-parameters"
            | "--path-parameters"
            | "--body"
            | "--auth"
            | "--graphql-query"
            | "--graphql-variables"
            | "--graphql-operation-name"
            | "--graphql-extensions"
            | "--type"
            | "--dry-run"
            | "--expect"
            | "--assert"
            | "--no-assertions"
            | "--send"
            | "--send-binary"
            | "--stdin-format"
            | "--timeout"
            | "--max-messages"
            | "--summary"
            | "--description"
            | "--description-json"
            | "--docs"
            | "--docs-json"
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
        "--expect" => legacy_expect(),
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

fn legacy_expect() -> CliError {
    CliError::invalid_arguments(
        "--expect was removed after Probe v1.1.1; migrate to --assert 'response.status equals 200' (repeat --assert for AND checks)",
    )
}

fn invalid_assertion() -> CliError {
    CliError::invalid_arguments(
        "--assert requires EXPRESSION OPERATOR [VALUE]; use a supported operator and provide a value for binary operators",
    )
}

fn parse_assertion(input: &str) -> Result<Assertion, CliError> {
    // Consume just the expression, operator, and one value separator. Everything
    // after that separator is literal value text, including additional whitespace.
    let expression_end = input
        .find(char::is_whitespace)
        .ok_or_else(invalid_assertion)?;
    let expression = &input[..expression_end];
    if expression.is_empty() {
        return Err(invalid_assertion());
    }
    let rest = input[expression_end..].trim_start_matches(char::is_whitespace);
    let operator_end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    let operator = &rest[..operator_end];
    let unary = matches!(operator, "isNull" | "isNotNull");
    if !unary
        && !matches!(
            operator,
            "equals" | "notEquals" | "contains" | "notContains" | "greaterThan" | "lessThan"
        )
    {
        return Err(invalid_assertion());
    }
    let value = rest[operator_end..]
        .chars()
        .next()
        .map(|separator| rest[operator_end + separator.len_utf8()..].to_owned());
    if !unary && value.is_none() {
        return Err(invalid_assertion());
    }
    Ok(Assertion {
        expression: expression.into(),
        operator: operator.into(),
        value,
        disabled: None,
        description: None,
    })
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
    fn agent_skill_install_parses_force_and_rejects_unsupported_arguments() {
        for (args, expected) in [
            (vec!["agent", "skill", "install"], false),
            (vec!["agent", "skill", "install", "--force"], true),
        ] {
            assert!(
                matches!(parse(args.into_iter().map(str::to_owned).collect()).unwrap(),
                super::Command::InstallAgentSkill { force } if force == expected)
            );
        }
        for args in [
            vec!["agent"],
            vec!["agent", "status"],
            vec!["agent", "install"],
            vec!["agent", "skill"],
            vec!["agent", "skill", "status"],
            vec!["agent", "skill", "install", "extra"],
            vec!["agent", "skill", "install", "--unknown"],
            vec!["agent", "skill", "install", "--force", "--force"],
            vec!["agent", "skill", "install", "--name", "probe"],
        ] {
            assert_eq!(
                parse(args.into_iter().map(str::to_owned).collect())
                    .unwrap_err()
                    .category,
                "invalid_arguments"
            );
        }
    }

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

#[cfg(test)]
mod assertion_flags {
    use super::parse_assertion;

    #[test]
    fn separates_tokens_and_preserves_every_value_byte_after_one_separator() {
        for (input, expected) in [
            ("response.body.name equals ", ""),
            ("response.body.name equals   ", "  "),
            ("response.body.name equals \"John Doe\"", "\"John Doe\""),
            (
                "response.body.name equals   spaced text  ",
                "  spaced text  ",
            ),
            (
                "response.status\tequals\t{{expectedStatus}}",
                "{{expectedStatus}}",
            ),
            (
                "response.body.name equals\u{2003}\"quoted\"  ",
                "\"quoted\"  ",
            ),
        ] {
            let parsed = parse_assertion(input).unwrap();
            assert_eq!(parsed.value.as_deref(), Some(expected));
        }
        assert_eq!(parse_assertion("response.body isNull").unwrap().value, None);
        assert_eq!(
            parse_assertion("response.body isNull {{ignored}}")
                .unwrap()
                .value
                .as_deref(),
            Some("{{ignored}}")
        );
    }
}
