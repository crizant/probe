use std::{io::Read, path::PathBuf};

use probe_application::imports::{ImportConversion, ImportMetadata, ImportSource, inspect_import};
use probe_core::{CollectionUpdate, FieldPatch};
use probe_opencollection::{LoadedWorkspace, create_bundled_workspace};
use serde_json::{Value, json};

use crate::{
    CliError, CommandOutput, WorkspaceInput,
    error::import_diagnostic_json,
    load,
    presentation::{append_documentation, documentation_json, unset_documentation},
};

pub(crate) fn create(path: PathBuf, name: Option<String>) -> Result<CommandOutput, CliError> {
    let loaded =
        create_bundled_workspace(&path, name.as_deref(), false).map_err(CliError::create)?;
    let workspace = loaded.workspace();
    let collection_name = workspace.metadata().name.as_deref().unwrap_or("<unnamed>");
    let created_path = source_path_or(loaded.source_path(), path);
    Ok(CommandOutput {
        human: format!(
            "Created bundled OpenCollection workspace\nName: {collection_name}\nPath: {}\n",
            created_path.display()
        ),
        json: json!({
            "collection": { "name": workspace.metadata().name },
            "counts": workspace_counts(&loaded),
            "created": true,
            "path": created_path,
        }),
    })
}

pub(crate) fn import_collection(
    source_kind: ImportSource,
    source: PathBuf,
    destination: PathBuf,
    workspace_id: Option<&str>,
    allow_partial: bool,
) -> Result<CommandOutput, CliError> {
    let mut conversion = inspect_import(source_kind, &source).map_err(CliError::import)?;
    if let ImportConversion::Yaak {
        workspace_id: selected,
        ..
    } = &mut conversion
    {
        *selected = workspace_id.map(str::to_owned);
    }
    let imported = conversion
        .convert(allow_partial)
        .map_err(CliError::import)?;
    let loaded = imported
        .create_workspace(&destination)
        .map_err(CliError::create)?;
    let path = source_path_or(loaded.source_path(), destination);
    let warning_count = imported.diagnostics.len();
    let projection_summary = projection_summary(&loaded);
    let (label, kind, environment_label, identity_key, environment_key, source_format, identity) =
        match &imported.metadata {
            ImportMetadata::Postman { format, collection } => (
                "Postman",
                "collection",
                "Collection variables environment",
                "collection",
                "collectionVariablesEnvironment",
                format.as_str(),
                json!({ "id": collection.id, "name": collection.name }),
            ),
            ImportMetadata::Yaak { format, workspace } => (
                "Yaak",
                "workspace",
                "Default environment",
                "workspace",
                "defaultEnvironment",
                format.as_str(),
                json!({ "id": workspace.id, "name": workspace.name }),
            ),
        };
    Ok(CommandOutput {
        human: format!(
            "Imported {label} {kind}\nName: {}\nPath: {}\nRequests: {}\nFolders: {}\nEnvironments: {}\n{environment_label}: {}\nWarnings: {warning_count}\n{projection_summary}",
            imported.source_name(),
            path.display(),
            loaded.workspace().request_count(),
            loaded.workspace().folder_count(),
            loaded.workspace().environments().len(),
            imported.selected_environment.as_deref().unwrap_or("none"),
        ),
        json: json!({
            "imported": true,
            "partial": imported.partial,
            "sourceFormat": source_format,
            (identity_key): identity,
            (environment_key): imported.selected_environment,
            "path": path,
            "counts": workspace_counts(&loaded),
            "warnings": imported.diagnostics.iter().map(import_diagnostic_json).collect::<Vec<_>>(),
            "projectionWarnings": projection_warnings(&loaded),
        }),
    })
}

pub(crate) fn get(
    input: &WorkspaceInput,
    stdin: &mut impl Read,
) -> Result<CommandOutput, CliError> {
    let loaded = load(input, stdin)?;
    let metadata = loaded.workspace().metadata();
    let mut human = format!(
        "Name: {}\nSummary: {}\n",
        metadata.name.as_deref().unwrap_or("<unnamed>"),
        metadata.summary.as_deref().unwrap_or("<unset>"),
    );
    append_documentation(&mut human, "Docs", metadata.docs.as_ref());
    Ok(CommandOutput {
        human,
        json: json!({
            "collection": {
                "docs": documentation_json(metadata.docs.as_ref()),
                "name": metadata.name,
                "summary": metadata.summary,
            }
        }),
    })
}

pub(crate) fn set(
    input: &WorkspaceInput,
    update: &CollectionUpdate,
    stdin: &mut impl Read,
) -> Result<CommandOutput, CliError> {
    let mut loaded = load(input, stdin)?;
    loaded
        .update_collection(update)
        .map_err(CliError::persistence)?;
    let metadata = loaded.workspace().metadata();
    let mut human = format!(
        "Updated collection\nName: {}\nSummary: {}\n",
        metadata.name.as_deref().unwrap_or("<unnamed>"),
        metadata.summary.as_deref().unwrap_or("<unset>"),
    );
    append_documentation(&mut human, "Docs", metadata.docs.as_ref());
    Ok(CommandOutput {
        human,
        json: json!({
            "collection": {
                "docs": documentation_json(metadata.docs.as_ref()),
                "name": metadata.name,
                "summary": metadata.summary,
            },
            "updated": true,
        }),
    })
}

pub(crate) fn unset(
    input: &WorkspaceInput,
    update: &CollectionUpdate,
    stdin: &mut impl Read,
) -> Result<CommandOutput, CliError> {
    let mut fields = Vec::new();
    if matches!(update.summary, FieldPatch::Clear) {
        fields.push("summary");
    }
    if matches!(update.docs, FieldPatch::Clear) {
        fields.push("docs");
    }
    let mut loaded = load(input, stdin)?;
    loaded
        .update_collection(update)
        .map_err(CliError::persistence)?;
    Ok(unset_documentation("collection", None, &fields))
}

pub(crate) fn validate(
    input: &WorkspaceInput,
    stdin: &mut impl Read,
) -> Result<CommandOutput, CliError> {
    let loaded = load(input, stdin)?;
    let workspace = loaded.workspace();
    let name = workspace.metadata().name.as_deref().unwrap_or("<unnamed>");
    let mut human = format!(
        "Valid OpenCollection workspace\nName: {name}\nRequests: {}\nFolders: {}\nEnvironments: {}\n",
        workspace.request_count(),
        workspace.folder_count(),
        workspace.environments().len()
    );
    if !loaded.diagnostics().is_empty() {
        human.push_str(&format!(
            "Unsupported values: {}\n",
            loaded.diagnostics().len()
        ));
        for diagnostic in loaded.diagnostics().iter().take(20) {
            human.push_str(&format!(
                "  {}: {} ({})\n",
                diagnostic.path,
                diagnostic.kind.as_str(),
                diagnostic.value
            ));
        }
        if loaded.diagnostics().len() > 20 {
            human.push_str(&format!(
                "  ... and {} more\n",
                loaded.diagnostics().len() - 20
            ));
        }
    }
    Ok(CommandOutput {
        human,
        json: json!({
            "collection": {
                "name": workspace.metadata().name,
                "summary": workspace.metadata().summary,
                "version": workspace.metadata().version,
            },
            "counts": workspace_counts(&loaded),
            "valid": true,
            "warnings": projection_warnings(&loaded),
        }),
    })
}

fn projection_warnings(loaded: &LoadedWorkspace) -> Vec<Value> {
    loaded
        .diagnostics()
        .iter()
        .map(|diagnostic| {
            json!({
                "path": diagnostic.path,
                "code": diagnostic.kind.as_str(),
                "value": diagnostic.value,
            })
        })
        .collect()
}

fn projection_summary(loaded: &LoadedWorkspace) -> String {
    if loaded.diagnostics().is_empty() {
        String::new()
    } else {
        format!(
            "Unsupported OpenCollection runtime values: {}\n",
            loaded.diagnostics().len()
        )
    }
}

fn source_path_or(source_path: Option<&std::path::Path>, fallback: PathBuf) -> PathBuf {
    source_path.map(PathBuf::from).unwrap_or(fallback)
}

fn workspace_counts(loaded: &LoadedWorkspace) -> Value {
    let workspace = loaded.workspace();
    json!({
        "environments": workspace.environments().len(),
        "folders": workspace.folder_count(),
        "requests": workspace.request_count(),
    })
}
