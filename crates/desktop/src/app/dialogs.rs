use std::{borrow::Cow, collections::BTreeMap, path::PathBuf};

use gpui::Action;
use probe_core::{Environment, ImportDiagnostic, ImportDiagnosticSeverity, ItemKind, RequestKey};
use probe_yaak::{YaakImportPreview, YaakWorkspaceSummary};

use crate::{components, session::SessionState};

use super::{
    documentation::OverviewTarget,
    imports::{ImportConversion, ImportSource},
};

pub(crate) const IMPORT_DIAGNOSTIC_GROUP_LIMIT: usize = 8;

#[derive(Clone, Debug)]
pub(crate) struct EnvironmentManagerDialog {
    pub(crate) original_name: String,
    draft: Environment,
    pub(crate) secret_statuses: BTreeMap<String, SecretUiStatus>,
    pub(crate) variable_row_ids: Vec<u64>,
    pub(crate) next_variable_row_id: u64,
    // Invalidated by draft edits and workspace reconciliation; secret presence is rendered separately.
    effective_rows: Option<std::rc::Rc<Vec<probe_core::EffectiveEnvironmentVariable>>>,
    #[cfg(test)]
    pub(crate) effective_row_builds: usize,
    pub(crate) active_field: Option<(
        EnvironmentVariableRowId,
        EnvironmentFieldKind,
        gpui::Entity<components::FieldInput>,
    )>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum EnvironmentVariableRowId {
    Direct(u64),
    Inherited { defined_in: String, name: String },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EnvironmentFieldKind {
    Name,
    Value,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SecretUiStatus {
    Loading,
    Stored,
    /// A trusted operation learned that the credential is not stored.
    NotStored,
    /// Probe has not learned whether the credential exists.
    Unknown,
}

impl EnvironmentManagerDialog {
    pub(crate) fn new(environment: &Environment) -> Self {
        let next_variable_row_id = environment.variables.len() as u64;
        Self {
            original_name: environment.name.clone(),
            draft: environment.clone(),
            secret_statuses: BTreeMap::new(),
            variable_row_ids: (0..next_variable_row_id).collect(),
            next_variable_row_id,
            effective_rows: None,
            #[cfg(test)]
            effective_row_builds: 0,
            active_field: None,
        }
    }

    pub(crate) fn draft(&self) -> &Environment {
        &self.draft
    }

    /// Invalidate before exposing mutable access, including edits outside the app adapter.
    pub(crate) fn draft_mut(&mut self) -> &mut Environment {
        self.effective_rows = None;
        &mut self.draft
    }

    pub(crate) fn effective_rows(
        &mut self,
        environments: &[Environment],
    ) -> std::rc::Rc<Vec<probe_core::EffectiveEnvironmentVariable>> {
        self.effective_rows
            .get_or_insert_with(|| {
                #[cfg(test)]
                {
                    self.effective_row_builds += 1;
                }
                std::rc::Rc::new(probe_core::effective_environment_variables(
                    environments,
                    &self.draft,
                ))
            })
            .clone()
    }

    /// Inherited rows depend on the workspace even when the direct draft is retained.
    pub(crate) fn rebind_workspace(&mut self) {
        self.effective_rows = None;
    }

    #[cfg(test)]
    pub(crate) fn cached_effective_rows(
        &self,
    ) -> &Option<std::rc::Rc<Vec<probe_core::EffectiveEnvironmentVariable>>> {
        &self.effective_rows
    }

    pub(crate) fn add_variable(&mut self, variable: probe_core::EnvironmentVariable) {
        self.draft_mut().variables.push(variable);
        self.variable_row_ids.push(self.next_variable_row_id);
        self.next_variable_row_id += 1;
    }

    pub(crate) fn sync_variable_row_ids(&mut self) {
        while self.variable_row_ids.len() < self.draft.variables.len() {
            self.variable_row_ids.push(self.next_variable_row_id);
            self.next_variable_row_id += 1;
        }
    }

    pub(crate) fn remove_variable(&mut self, index: usize) {
        if index < self.draft.variables.len() {
            if self.active_field.as_ref().is_some_and(|(id, _, _)| {
                *id == EnvironmentVariableRowId::Direct(self.variable_row_ids[index])
            }) {
                self.active_field = None;
            }
            self.draft_mut().variables.remove(index);
            self.variable_row_ids.remove(index);
        }
    }
}

pub(crate) enum PendingClose {
    Tab(RequestKey),
    Overview(OverviewTarget),
    OtherTabs {
        keep: crate::shell::OpenTab,
    },
    Workspace,
    Window,
    Quit,
    Open {
        path: PathBuf,
        restored_state: Option<Box<SessionState>>,
    },
    Create {
        path: PathBuf,
    },
    Import(ImportSource),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum StoredSecretRename {
    Environment,
    Variable { from: String, to: String },
    Variables,
}

pub(crate) enum ApplicationDialog {
    About,
    Unsaved {
        keys: Vec<RequestKey>,
        documentation: bool,
        pending: PendingClose,
    },
    Delete {
        kind: ItemKind,
        selector: String,
        name: String,
        detail: String,
    },
    DeleteEnvironment {
        name: String,
        detail: String,
    },
    DeleteStoredSecret {
        name: String,
        environment: String,
        detail: String,
    },
    RenameStoredSecrets {
        kind: StoredSecretRename,
    },
    UnsavedEnvironment,
    FilesystemConflict {
        path: Option<PathBuf>,
        detail: String,
    },
    SelectYaakWorkspace {
        preview: Box<YaakImportPreview>,
        workspaces: Vec<YaakWorkspaceSummary>,
    },
    SelectCollectionFile {
        candidates: Vec<PathBuf>,
    },
    ConfirmPartialImport {
        conversion: ImportConversion,
        detail: String,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DesktopMenu {
    File,
    Edit,
    View,
    Help,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DesktopSubmenu {
    Import,
    EditorLayout,
}

pub(crate) struct DesktopMenuDefinition {
    pub(crate) id: &'static str,
    pub(crate) label: &'static str,
    pub(crate) width: f32,
    pub(crate) items: Vec<DesktopMenuItem>,
}

pub(crate) enum DesktopMenuItem {
    Action(&'static str, Box<dyn Action>, Option<bool>),
    Submenu(&'static str, DesktopSubmenu, DesktopMenuDefinition),
    Separator,
}

impl DesktopMenuItem {
    pub(crate) fn action(label: &'static str, action: impl Action + 'static) -> Self {
        Self::Action(label, Box::new(action), None)
    }

    pub(crate) fn checked_action(
        label: &'static str,
        checked: bool,
        action: impl Action + 'static,
    ) -> Self {
        Self::Action(label, Box::new(action), Some(checked))
    }

    pub(crate) fn submenu(
        label: &'static str,
        state: DesktopSubmenu,
        popup: DesktopMenuDefinition,
    ) -> Self {
        Self::Submenu(label, state, popup)
    }
}

impl ApplicationDialog {
    pub(crate) fn title(&self) -> Cow<'_, str> {
        match self {
            Self::About => Cow::Borrowed("Probe"),
            Self::Unsaved {
                keys,
                documentation: true,
                ..
            } if !keys.is_empty() => Cow::Borrowed("Save request and documentation changes?"),
            Self::Unsaved { keys, .. } if keys.is_empty() => {
                Cow::Borrowed("Save documentation changes?")
            }
            Self::Unsaved { keys, .. } => {
                let noun = if keys.len() == 1 {
                    "request"
                } else {
                    "requests"
                };
                Cow::Owned(format!("Save changes to {} {noun}?", keys.len()))
            }
            Self::UnsavedEnvironment => Cow::Borrowed("Save changes to this environment?"),
            Self::Delete { name, .. } | Self::DeleteEnvironment { name, .. } => {
                Cow::Owned(format!("Delete “{name}”?"))
            }
            Self::DeleteStoredSecret { .. } => Cow::Borrowed("Delete stored value?"),
            Self::RenameStoredSecrets {
                kind: StoredSecretRename::Environment,
            } => Cow::Borrowed("Rename environment?"),
            Self::RenameStoredSecrets {
                kind: StoredSecretRename::Variable { .. },
            } => Cow::Borrowed("Rename secret variable?"),
            Self::RenameStoredSecrets {
                kind: StoredSecretRename::Variables,
            } => Cow::Borrowed("Rename secret variables?"),
            Self::FilesystemConflict { .. } => {
                Cow::Borrowed("Collection changes conflict with local edits")
            }
            Self::SelectYaakWorkspace { .. } => Cow::Borrowed("Select a Yaak workspace"),
            Self::SelectCollectionFile { .. } => Cow::Borrowed("Select a collection"),
            Self::ConfirmPartialImport { conversion, .. } => Cow::Owned(format!(
                "Some {} data cannot be represented",
                conversion.source().label()
            )),
        }
    }

    pub(crate) fn description(&self) -> Cow<'_, str> {
        match self {
            Self::About => Cow::Borrowed(concat!(
                "Version ",
                env!("CARGO_PKG_VERSION"),
                "\n\nA fast, native, local-first API client."
            )),
            Self::Unsaved { .. } | Self::UnsavedEnvironment => {
                Cow::Borrowed("Unsaved changes will be lost if you discard them.")
            }
            Self::Delete { detail, .. }
            | Self::DeleteEnvironment { detail, .. }
            | Self::DeleteStoredSecret { detail, .. }
            | Self::FilesystemConflict { detail, .. }
            | Self::ConfirmPartialImport { detail, .. } => Cow::Borrowed(detail),
            Self::RenameStoredSecrets {
                kind: StoredSecretRename::Environment,
            } => Cow::Borrowed(ENVIRONMENT_SECRET_RENAME_DETAIL),
            Self::RenameStoredSecrets {
                kind: StoredSecretRename::Variables,
            } => Cow::Borrowed(SECRET_VARIABLES_RENAME_DETAIL),
            Self::RenameStoredSecrets {
                kind: StoredSecretRename::Variable { from, to },
            } => Cow::Owned(format!(
                "Stored secret values are associated with the variable name.\nAfter renaming {from} to {to}, its value will need to be stored again."
            )),
            Self::SelectYaakWorkspace { .. } => {
                Cow::Borrowed("Choose the workspace to import into a new Probe collection.")
            }
            Self::SelectCollectionFile { .. } => Cow::Borrowed(
                "This folder contains multiple bundled OpenCollection files. Choose one to open.",
            ),
        }
    }

    pub(crate) const fn width(&self) -> f32 {
        match self {
            Self::SelectYaakWorkspace { .. }
            | Self::SelectCollectionFile { .. }
            | Self::ConfirmPartialImport { .. } => components::WIDE_DIALOG_WIDTH,
            _ => components::COMPACT_DIALOG_WIDTH,
        }
    }

    pub(crate) const fn action_specs(&self) -> Option<&'static [DialogActionSpec]> {
        match self {
            Self::About => Some(ABOUT_DIALOG_ACTIONS),
            Self::Unsaved { .. } | Self::UnsavedEnvironment => Some(UNSAVED_DIALOG_ACTIONS),
            Self::Delete { .. } | Self::DeleteEnvironment { .. } => Some(DELETE_DIALOG_ACTIONS),
            Self::DeleteStoredSecret { .. } => Some(DELETE_STORED_VALUE_DIALOG_ACTIONS),
            Self::RenameStoredSecrets { .. } => Some(RENAME_STORED_SECRETS_DIALOG_ACTIONS),
            Self::FilesystemConflict { .. } => Some(FILESYSTEM_CONFLICT_DIALOG_ACTIONS),
            Self::SelectYaakWorkspace { .. } => None,
            Self::SelectCollectionFile { .. } => None,
            Self::ConfirmPartialImport { .. } => Some(PARTIAL_IMPORT_DIALOG_ACTIONS),
        }
    }

    pub(crate) fn primary_action(&self) -> Option<ApplicationDialogAction> {
        self.action_specs()?.iter().find_map(|spec| {
            (spec.style == components::DialogActionStyle::Primary).then_some(spec.action)
        })
    }

    pub(crate) fn destructive_action(&self) -> Option<ApplicationDialogAction> {
        self.action_specs()?.iter().find_map(|spec| {
            (spec.style == components::DialogActionStyle::Destructive).then_some(spec.action)
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ApplicationDialogAction {
    Cancel,
    Save,
    Discard,
    Delete,
    UseDisk,
    KeepLocal,
    Rename,
    SelectWorkspace(usize),
    SelectCollectionFile(usize),
    ImportSupportedData,
}

#[derive(Clone, Copy)]
pub(crate) struct DialogActionSpec {
    pub(crate) id: &'static str,
    pub(crate) label: &'static str,
    pub(crate) style: components::DialogActionStyle,
    pub(crate) action: ApplicationDialogAction,
}

impl DialogActionSpec {
    const fn new(
        id: &'static str,
        label: &'static str,
        style: components::DialogActionStyle,
        action: ApplicationDialogAction,
    ) -> Self {
        Self {
            id,
            label,
            style,
            action,
        }
    }
}

pub(crate) const CANCEL_DIALOG_ACTION: DialogActionSpec = DialogActionSpec::new(
    "application-dialog-cancel",
    "Cancel",
    components::DialogActionStyle::Secondary,
    ApplicationDialogAction::Cancel,
);
const ABOUT_DIALOG_ACTIONS: &[DialogActionSpec] = &[DialogActionSpec::new(
    "application-dialog-done",
    "Done",
    components::DialogActionStyle::Primary,
    ApplicationDialogAction::Cancel,
)];
const UNSAVED_DIALOG_ACTIONS: &[DialogActionSpec] = &[
    CANCEL_DIALOG_ACTION,
    DialogActionSpec::new(
        "application-dialog-discard",
        "Discard",
        components::DialogActionStyle::Destructive,
        ApplicationDialogAction::Discard,
    ),
    DialogActionSpec::new(
        "application-dialog-save",
        "Save",
        components::DialogActionStyle::Primary,
        ApplicationDialogAction::Save,
    ),
];
const ENVIRONMENT_SECRET_RENAME_DETAIL: &str = "Stored secret values are associated with the environment name.\nAfter renaming, affected secrets will need to be stored again.\n\nThe existing stored credentials will not be migrated.";
const SECRET_VARIABLES_RENAME_DETAIL: &str = "Stored secret values are associated with the variable name.\nAfter renaming, affected secrets will need to be stored again.\n\nThe existing stored credentials will not be migrated.";
const RENAME_STORED_SECRETS_DIALOG_ACTIONS: &[DialogActionSpec] = &[
    CANCEL_DIALOG_ACTION,
    DialogActionSpec::new(
        "application-dialog-rename",
        "Rename",
        components::DialogActionStyle::Primary,
        ApplicationDialogAction::Rename,
    ),
];
const DELETE_DIALOG_ACTIONS: &[DialogActionSpec] = &[
    CANCEL_DIALOG_ACTION,
    DialogActionSpec::new(
        "application-dialog-delete",
        "Delete",
        components::DialogActionStyle::Destructive,
        ApplicationDialogAction::Delete,
    ),
];
const DELETE_STORED_VALUE_DIALOG_ACTIONS: &[DialogActionSpec] = &[
    CANCEL_DIALOG_ACTION,
    DialogActionSpec::new(
        "application-dialog-delete-stored-value",
        "Delete Stored Value",
        components::DialogActionStyle::Destructive,
        ApplicationDialogAction::Delete,
    ),
];
const FILESYSTEM_CONFLICT_DIALOG_ACTIONS: &[DialogActionSpec] = &[
    DialogActionSpec::new(
        "application-dialog-keep-local",
        "Keep Local",
        components::DialogActionStyle::Secondary,
        ApplicationDialogAction::KeepLocal,
    ),
    DialogActionSpec::new(
        "application-dialog-use-disk",
        "Use Disk",
        components::DialogActionStyle::Destructive,
        ApplicationDialogAction::UseDisk,
    ),
];
const PARTIAL_IMPORT_DIALOG_ACTIONS: &[DialogActionSpec] = &[
    CANCEL_DIALOG_ACTION,
    DialogActionSpec::new(
        "application-dialog-import-supported",
        "Import Supported Data",
        components::DialogActionStyle::Primary,
        ApplicationDialogAction::ImportSupportedData,
    ),
];

pub(crate) fn suggested_collection_filename(name: &str) -> String {
    let stem = name
        .trim()
        .chars()
        .map(|character| {
            if matches!(character, '/' | '\\' | ':' | '\0') {
                '-'
            } else {
                character
            }
        })
        .collect::<String>();
    let stem = stem.trim_matches([' ', '.', '-']);
    format!("{}.yml", if stem.is_empty() { "Imported" } else { stem })
}

pub(crate) fn format_import_diagnostics(diagnostics: &[ImportDiagnostic]) -> String {
    if diagnostics.is_empty() {
        return "No compatibility issues found.".to_owned();
    }

    let lossy_count = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == ImportDiagnosticSeverity::Lossy)
        .count();
    let warning_count = diagnostics.len() - lossy_count;
    let mut groups = BTreeMap::new();
    for diagnostic in diagnostics {
        *groups
            .entry((
                diagnostic.severity,
                diagnostic.resource_type.as_str(),
                diagnostic.field.as_deref(),
                diagnostic.code,
                diagnostic.message.as_str(),
            ))
            .or_insert(0_usize) += 1;
    }

    let mut lines = vec![format!(
        "Found {} compatibility issue(s): {lossy_count} lossy, {warning_count} warning(s).",
        diagnostics.len()
    )];
    lines.push(String::new());
    for ((severity, resource_type, field, _, message), count) in
        groups.iter().take(IMPORT_DIAGNOSTIC_GROUP_LIMIT)
    {
        let resource = field
            .map(|field| format!("{resource_type}.{field}"))
            .unwrap_or_else(|| (*resource_type).to_owned());
        lines.push(format!(
            "• {count} {} — {resource}: {message}",
            severity.as_str()
        ));
    }
    if groups.len() > IMPORT_DIAGNOSTIC_GROUP_LIMIT {
        let hidden_group_count = groups.len() - IMPORT_DIAGNOSTIC_GROUP_LIMIT;
        let hidden_issue_count = groups
            .values()
            .skip(IMPORT_DIAGNOSTIC_GROUP_LIMIT)
            .sum::<usize>();
        lines.push(format!(
            "• {hidden_issue_count} more issue(s) across {hidden_group_count} additional type(s)"
        ));
    }
    if lossy_count > 0 {
        lines.push(String::new());
        lines.push(
            "Import Supported Data will omit or change the lossy fields listed above.".to_owned(),
        );
    }
    lines.join("\n")
}
