use std::{collections::BTreeMap, io::Read};

use probe_core::{FieldPatch, FolderKey, FolderUpdate, WorkspaceItemRef};
use probe_opencollection::{LoadedWorkspace, StructureOperation, StructureResult};
use serde_json::json;

use crate::{
    CliError, CommandOutput, WorkspaceInput, load,
    presentation::{append_documentation, documentation_json, unset_documentation},
};

pub(crate) fn edit(
    input: &WorkspaceInput,
    operation_name: &str,
    operation: StructureOperation,
    stdin: &mut impl Read,
) -> Result<CommandOutput, CliError> {
    let mut loaded = load(input, stdin)?;
    let result = loaded
        .apply_structure(operation)
        .map_err(CliError::structure)?;
    Ok(output(operation_name, &result))
}

pub(crate) fn get_folder(
    input: &WorkspaceInput,
    selector: &str,
    stdin: &mut impl Read,
) -> Result<CommandOutput, CliError> {
    let loaded = load(input, stdin)?;
    let key = loaded
        .folder_key(selector)
        .ok_or_else(|| folder_not_found(selector))?;
    let folder = loaded
        .workspace()
        .folder(key)
        .expect("repository folder key must resolve");
    let mut human = format!(
        "Name: {}\nSelector: {selector}\n",
        folder.metadata.name.as_deref().unwrap_or("<unnamed>"),
    );
    append_documentation(
        &mut human,
        "Description",
        folder.metadata.description.as_ref(),
    );
    append_documentation(&mut human, "Docs", folder.docs.as_ref());
    Ok(CommandOutput {
        human,
        json: json!({
            "description": documentation_json(folder.metadata.description.as_ref()),
            "docs": documentation_json(folder.docs.as_ref()),
            "name": folder.metadata.name,
            "selector": selector,
        }),
    })
}

pub(crate) fn set_folder(
    input: &WorkspaceInput,
    selector: &str,
    update: &FolderUpdate,
    stdin: &mut impl Read,
) -> Result<CommandOutput, CliError> {
    let mut loaded = load(input, stdin)?;
    loaded
        .update_folder(selector, update)
        .map_err(CliError::persistence)?;
    let key = loaded
        .folder_key(selector)
        .expect("successfully updated folder selector must resolve");
    let folder = loaded
        .workspace()
        .folder(key)
        .expect("repository folder key must resolve");
    let mut human = format!(
        "Updated folder\nName: {}\nSelector: {selector}\n",
        folder.metadata.name.as_deref().unwrap_or("<unnamed>"),
    );
    append_documentation(
        &mut human,
        "Description",
        folder.metadata.description.as_ref(),
    );
    append_documentation(&mut human, "Docs", folder.docs.as_ref());
    Ok(CommandOutput {
        human,
        json: json!({
            "description": documentation_json(folder.metadata.description.as_ref()),
            "docs": documentation_json(folder.docs.as_ref()),
            "name": folder.metadata.name,
            "selector": selector,
            "updated": true,
        }),
    })
}

pub(crate) fn unset_folder(
    input: &WorkspaceInput,
    selector: &str,
    update: &FolderUpdate,
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
        .update_folder(selector, update)
        .map_err(CliError::persistence)?;
    Ok(unset_documentation("folder", Some(selector), &fields))
}

fn folder_not_found(selector: &str) -> CliError {
    CliError::persistence(probe_opencollection::SaveError::FolderNotFound(
        selector.to_owned(),
    ))
}

pub(crate) fn list_folders(
    input: &WorkspaceInput,
    stdin: &mut impl Read,
) -> Result<CommandOutput, CliError> {
    let loaded = load(input, stdin)?;
    let mut parents = BTreeMap::new();
    collect_folder_parents(&loaded, loaded.workspace().root_items(), None, &mut parents);
    let mut lines = vec!["SELECTOR\tNAME\tPARENT".to_owned()];
    let mut folders = Vec::with_capacity(loaded.folders().len());
    for located in loaded.folders() {
        let folder = loaded
            .workspace()
            .folder(located.key())
            .expect("repository folder key must resolve");
        let name = folder.metadata.name.as_deref().unwrap_or("");
        let parent = parents
            .get(&located.key())
            .and_then(|parent| parent.as_deref());
        lines.push(format!(
            "{}\t{name}\t{}",
            located.selector(),
            parent.unwrap_or("")
        ));
        folders.push(json!({
            "name": folder.metadata.name,
            "parent": parent,
            "selector": located.selector(),
        }));
    }
    Ok(CommandOutput {
        human: format!("{}\n", lines.join("\n")),
        json: json!({ "folders": folders }),
    })
}

fn output(operation: &str, result: &StructureResult) -> CommandOutput {
    let selector = result.selector.as_deref().unwrap_or("<deleted>");
    CommandOutput {
        human: format!("{} {}: {selector}\n", operation, result.kind.as_str()),
        json: json!({
            "index": result.index,
            "itemType": result.kind.as_str(),
            "operation": operation,
            "parent": result.parent,
            "previousSelector": result.previous_selector,
            "selector": result.selector,
            "selectorRemaps": result.selector_remaps,
        }),
    }
}

fn collect_folder_parents(
    loaded: &LoadedWorkspace,
    items: &[WorkspaceItemRef],
    parent: Option<&str>,
    output: &mut BTreeMap<FolderKey, Option<String>>,
) {
    for item in items {
        if let WorkspaceItemRef::Folder(key) = item {
            output.insert(*key, parent.map(str::to_owned));
            let selector = loaded
                .folder_selector(*key)
                .expect("repository folder key must have a selector");
            let folder = loaded
                .workspace()
                .folder(*key)
                .expect("repository folder key must resolve");
            collect_folder_parents(loaded, &folder.children, Some(selector), output);
        }
    }
}
