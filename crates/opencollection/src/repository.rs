use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{
        Arc, Weak,
        atomic::{AtomicU64, Ordering},
    },
};

use atomic_write_file::AtomicWriteFile;
use probe_core::{
    Body, CollectionItem, CollectionUpdate, Documentation, Environment, EnvironmentResolutionError,
    EnvironmentVariable, FieldPatch, FolderKey, FolderUpdate, RequestKey, RequestProtocol,
    RequestUpdate, Variable, VariableValue, VariableValueSet, VariableValueVariant,
    WebSocketMessage, Workspace, WorkspaceItemRef, validate_environments,
    validate_unique_variable_names,
};
use serde_yaml_ng::Value;

use super::{ParseError, ProjectionDiagnostic, parse};
use crate::{
    document::{EnvironmentDocument, NativeItemType},
    projection::{project_item, project_items, sort_diagnostics},
};

mod create;
mod environment;
mod errors;
mod loading;
mod lock;
mod new_file;
mod yaml;

use create::environment_value;
pub use create::{create_bundled_workspace, create_bundled_workspace_from_collection};
use environment::*;
pub use errors::{CreateError, LoadError, SaveError};
pub(crate) use loading::relative_selector;
pub use loading::{load_workspace, load_workspace_from_str};
pub(crate) use lock::SaveLock;
use new_file::{NewFileError, write_new_file};
use yaml::*;

/// A request and its repository-backed selector.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocatedRequest {
    selector: Arc<str>,
    key: RequestKey,
    persistence: Option<RequestPersistence>,
}

impl LocatedRequest {
    /// Returns the selector accepted by CLI and repository operations.
    #[must_use]
    pub fn selector(&self) -> &str {
        &self.selector
    }

    /// Returns the request's session-only workspace key.
    #[must_use]
    pub const fn key(&self) -> RequestKey {
        self.key
    }
}

/// A folder and its repository-backed selector.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocatedFolder {
    selector: Arc<str>,
    key: FolderKey,
    persistence: Option<FolderPersistence>,
}

impl LocatedFolder {
    /// Returns the stable selector used to restore presentation state.
    #[must_use]
    pub fn selector(&self) -> &str {
        &self.selector
    }

    /// Returns the folder's session-only workspace key.
    #[must_use]
    pub const fn key(&self) -> FolderKey {
        self.key
    }
}

/// A loaded OpenCollection workspace and its persistence-locator index.
///
/// Deliberately not `Clone`: background work captures a narrow prepared operation.
#[derive(Debug, PartialEq)]
pub struct LoadedWorkspace {
    workspace: Workspace,
    diagnostics: Vec<super::ProjectionDiagnostic>,
    // Loaded records are in fresh arena slot order. Structural edits reload the
    // repository; detached drafts occupy later slots and never enter these vectors.
    requests: Vec<LocatedRequest>,
    folders: Vec<LocatedFolder>,
    request_indices_by_selector: BTreeMap<Arc<str>, usize>,
    folder_indices_by_selector: BTreeMap<Arc<str>, usize>,
    environment_persistence: BTreeMap<String, EnvironmentPersistence>,
    pub(crate) documents: BTreeMap<PathBuf, SourceDocument>,
    pub(crate) source: WorkspaceSource,
    pub(crate) baseline: WorkspaceBaseline,
    pub(crate) live_baseline: Arc<LiveBaseline>,
}

/// Runtime identity of a loaded repository baseline. Never written to OpenCollection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkspaceBaseline(u64);

impl WorkspaceBaseline {
    pub(crate) fn fresh() -> Self {
        static NEXT_BASELINE: AtomicU64 = AtomicU64::new(1);
        Self(
            NEXT_BASELINE
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
                    next.checked_add(1)
                })
                .expect("repository baseline identifiers exhausted"),
        )
    }
}

#[derive(Debug)]
pub(crate) struct LiveBaseline(AtomicU64);

impl LiveBaseline {
    pub(crate) fn new(baseline: WorkspaceBaseline) -> Self {
        Self(AtomicU64::new(baseline.0))
    }

    fn current(&self) -> WorkspaceBaseline {
        WorkspaceBaseline(self.0.load(Ordering::Acquire))
    }
}

impl PartialEq for LiveBaseline {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

#[derive(Debug)]
struct PreparedBaseline {
    expected: WorkspaceBaseline,
    live: Weak<LiveBaseline>,
}

impl PreparedBaseline {
    fn check_live(&self) -> Result<(), SaveError> {
        if self
            .live
            .upgrade()
            .is_some_and(|live| live.current() == self.expected)
        {
            Ok(())
        } else {
            Err(SaveError::StaleCompletion)
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum WorkspaceSource {
    Bundled(PathBuf),
    Unbundled(PathBuf),
    Memory,
}

impl LoadedWorkspace {
    fn prepared_baseline(&self) -> PreparedBaseline {
        PreparedBaseline {
            expected: self.baseline,
            live: Arc::downgrade(&self.live_baseline),
        }
    }

    /// Identifies this loaded repository and its current retained source baseline.
    #[must_use]
    pub const fn baseline(&self) -> WorkspaceBaseline {
        self.baseline
    }

    fn check_baseline(&self, baseline: WorkspaceBaseline) -> Result<(), SaveError> {
        if self.baseline == baseline {
            Ok(())
        } else {
            Err(SaveError::CommittedButNotIntegrated)
        }
    }

    fn advance_baseline(&mut self) {
        self.baseline = WorkspaceBaseline::fresh();
        self.live_baseline
            .0
            .store(self.baseline.0, Ordering::Release);
    }

    /// Returns the in-memory domain workspace.
    #[must_use]
    pub const fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    /// Values retained in source YAML but unsupported by Probe's runtime.
    #[must_use]
    pub fn diagnostics(&self) -> &[super::ProjectionDiagnostic] {
        &self.diagnostics
    }

    /// Returns whether repository locators are workspace-relative filesystem paths.
    ///
    /// Unbundled collections use paths. Bundled and in-memory documents use structural
    /// item selectors, which do not collide by file name.
    #[must_use]
    pub const fn uses_path_locators(&self) -> bool {
        matches!(self.source, WorkspaceSource::Unbundled(_))
    }

    /// Returns the filesystem path this workspace was loaded from, if any.
    #[must_use]
    pub fn source_path(&self) -> Option<&Path> {
        match &self.source {
            WorkspaceSource::Bundled(path) | WorkspaceSource::Unbundled(path) => Some(path),
            WorkspaceSource::Memory => None,
        }
    }

    /// Mutably looks up a request in the loaded in-memory workspace.
    ///
    /// Desktop editors use this fast path to apply draft changes immediately. Saving
    /// remains an explicit, separate repository operation.
    pub fn request_mut(&mut self, key: RequestKey) -> Option<&mut probe_core::Request> {
        self.workspace.request_mut(key)
    }

    /// Adds an in-memory editor draft with no persistence locator.
    pub fn add_detached_request(&mut self, request: probe_core::Request) -> RequestKey {
        self.workspace.add_detached_request(request)
    }

    /// Removes an in-memory editor draft after its tab closes.
    pub fn remove_detached_request(&mut self, key: RequestKey) -> Option<probe_core::Request> {
        if self.request_selector(key).is_some() {
            return None;
        }
        self.workspace.remove_request(key)
    }

    /// Updates a plain environment variable in the in-memory workspace.
    pub fn set_environment_variable(
        &mut self,
        environment_name: &str,
        variable_name: &str,
        value: String,
    ) -> Result<(), EnvironmentResolutionError> {
        self.workspace
            .set_environment_variable(environment_name, variable_name, value)
    }

    /// Applies a variable set in memory and atomically persists its OpenCollection document.
    ///
    /// The save is rejected if the source file no longer exactly matches the bytes
    /// loaded by this repository instance. On persistence failure, the in-memory
    /// environment remains updated so callers can report or retry the dirty state.
    pub fn update_environment_variable(
        &mut self,
        environment_name: &str,
        variable_name: &str,
        value: String,
    ) -> Result<(), SaveError> {
        self.workspace
            .set_environment_variable(environment_name, variable_name, value)
            .map_err(SaveError::Environment)?;
        let variable = self
            .plain_environment_variable(environment_name, variable_name)
            .cloned()
            .ok_or_else(|| {
                SaveError::InvalidDocument(format!(
                    "environment '{environment_name}' is missing variable '{variable_name}' after update"
                ))
            })?;
        self.persist_environment_mutation(
            environment_name,
            EnvironmentYamlMutation::Set { variable },
        )
    }

    /// Sets or removes an environment description and atomically persists the document.
    ///
    /// `Set` writes the documentation value, including explicit null. `Clear` removes
    /// the YAML key. An unchanged patch is rejected. A description change that must
    /// be written together with other environment edits uses
    /// [`Self::prepare_environment_replace_with_description`].
    pub fn update_environment_description(
        &mut self,
        environment_name: &str,
        description: &FieldPatch<Documentation>,
    ) -> Result<(), SaveError> {
        if description.is_unchanged() {
            return Err(SaveError::EmptyUpdate);
        }
        self.workspace
            .set_environment_description(environment_name, description)
            .map_err(SaveError::Environment)?;
        self.persist_environment_mutation(
            environment_name,
            EnvironmentYamlMutation::Description {
                description: description.clone(),
            },
        )
    }

    /// Removes a variable from the named environment and atomically persists the document.
    pub fn unset_environment_variable(
        &mut self,
        environment_name: &str,
        variable_name: &str,
    ) -> Result<(), SaveError> {
        self.workspace
            .unset_environment_variable(environment_name, variable_name)
            .map_err(SaveError::Environment)?;
        self.persist_environment_mutation(
            environment_name,
            EnvironmentYamlMutation::Unset {
                name: variable_name.to_owned(),
            },
        )
    }

    /// Creates a new environment in memory and atomically persists its OpenCollection document.
    pub fn create_environment(
        &mut self,
        name: String,
        extends: Option<String>,
    ) -> Result<(), SaveError> {
        let prepared = self.prepare_environment_create(name, extends)?;
        let name = prepared.environment_name().to_owned();
        match prepared.execute() {
            Ok(saved) => {
                self.complete_environment_create(saved)?;
                Ok(())
            }
            Err(error) => {
                self.revert_created_environment(&name);
                Err(error)
            }
        }
    }

    /// Creates an environment in memory and captures a filesystem persist for background execution.
    ///
    /// The in-memory workspace contains the new environment after this returns. Call
    /// [`PreparedEnvironmentCreate::execute`] away from the UI thread, then
    /// [`Self::complete_environment_create`] or [`Self::revert_created_environment`].
    pub fn prepare_environment_create(
        &mut self,
        name: String,
        extends: Option<String>,
    ) -> Result<PreparedEnvironmentCreate, SaveError> {
        self.workspace
            .create_environment(name.clone(), extends)
            .map_err(SaveError::Environment)?;
        let environment = self
            .workspace
            .environments()
            .iter()
            .find(|environment| environment.name == name)
            .cloned()
            .expect("created environment must be present");
        match &self.source {
            WorkspaceSource::Memory => {
                self.revert_created_environment(&name);
                Err(SaveError::ReadOnlySource)
            }
            WorkspaceSource::Bundled(document_path) => {
                let document_path = document_path.clone();
                let original_source = self
                    .documents
                    .get(&document_path)
                    .expect("bundled workspace must retain its source document")
                    .original_source
                    .clone();
                Ok(PreparedEnvironmentCreate {
                    baseline: self.prepared_baseline(),
                    environment,
                    kind: EnvironmentCreateKind::Bundled {
                        document_path,
                        original_source,
                        bundled_index: self.workspace.environments().len() - 1,
                    },
                })
            }
            WorkspaceSource::Unbundled(root) => Ok(PreparedEnvironmentCreate {
                baseline: self.prepared_baseline(),
                environment,
                kind: EnvironmentCreateKind::Unbundled { root: root.clone() },
            }),
        }
    }

    /// Records persistence metadata after a prepared environment create succeeds.
    pub fn complete_environment_create(
        &mut self,
        saved: CompletedEnvironmentCreate,
    ) -> Result<(), SaveError> {
        self.check_baseline(saved.baseline)?;
        self.environment_persistence.insert(
            saved.name,
            EnvironmentPersistence {
                document_path: saved.document_path.clone(),
                bundled_index: saved.bundled_index,
            },
        );
        self.documents.insert(
            saved.document_path,
            SourceDocument {
                original_source: saved.serialized_source,
            },
        );
        self.advance_baseline();
        Ok(())
    }

    /// Drops an in-memory environment that was created but not persisted.
    pub fn revert_created_environment(&mut self, name: &str) {
        self.workspace.revert_created_environment(name);
        self.environment_persistence.remove(name);
        self.advance_baseline();
    }

    /// Captures an environment-variable save that can be executed away from the UI thread.
    ///
    /// The in-memory workspace must already contain the updated variable. Preparing is
    /// in-memory only. [`PreparedEnvironmentSave::execute`] performs the conflict check
    /// and atomic filesystem write.
    pub fn prepare_environment_variable_save(
        &self,
        environment_name: &str,
        variable_name: &str,
    ) -> Result<PreparedEnvironmentSave, SaveError> {
        let variable = self
            .plain_environment_variable(environment_name, variable_name)
            .cloned()
            .ok_or_else(|| {
                SaveError::Environment(EnvironmentResolutionError::VariableNotFound {
                    environment: environment_name.to_owned(),
                    variable: variable_name.to_owned(),
                })
            })?;
        self.prepare_environment_mutation(
            environment_name,
            EnvironmentYamlMutation::Set { variable },
        )
    }

    /// Refreshes the retained conflict baseline after a prepared environment save succeeds.
    pub fn complete_environment_save(
        &mut self,
        saved: CompletedEnvironmentSave,
    ) -> Result<(), SaveError> {
        self.check_baseline(saved.baseline)?;
        self.documents.insert(
            saved.document_path,
            SourceDocument {
                original_source: saved.serialized_source,
            },
        );
        self.advance_baseline();
        Ok(())
    }

    /// Replaces one environment in memory and atomically persists the OpenCollection document.
    pub fn replace_environment(
        &mut self,
        original_name: &str,
        replacement: Environment,
    ) -> Result<(), SaveError> {
        let prepared = self.prepare_environment_replace(original_name, replacement)?;
        let saved = prepared.execute()?;
        self.complete_environment_replace(saved)?;
        Ok(())
    }

    /// Captures a validated replacement of one environment for background persistence.
    ///
    /// Secret variables are retained from the source document. The replacement may edit
    /// the environment name, parent, and plain variables. The existing description is
    /// kept. Description-only edits use [`Self::update_environment_description`]. A
    /// replacement and a description change in one write use
    /// [`Self::prepare_environment_replace_with_description`]. Renaming a parent
    /// environment is rejected because it would require a multi-document transaction.
    pub fn prepare_environment_replace(
        &self,
        original_name: &str,
        replacement: Environment,
    ) -> Result<PreparedEnvironmentReplace, SaveError> {
        if replacement.name != original_name
            && self
                .workspace
                .environments()
                .iter()
                .any(|environment| environment.extends.as_deref() == Some(original_name))
        {
            return Err(SaveError::Environment(
                EnvironmentResolutionError::EnvironmentInUse(original_name.to_owned()),
            ));
        }
        let original = self
            .workspace
            .environments()
            .iter()
            .find(|environment| environment.name == original_name)
            .cloned()
            .ok_or_else(|| {
                SaveError::Environment(EnvironmentResolutionError::EnvironmentNotFound(
                    original_name.to_owned(),
                ))
            })?;
        let replacement = validate_environment_replacement(&original, replacement)?;
        let mut candidate = self.workspace.environments().to_vec();
        probe_core::replace_environment(&mut candidate, original_name, replacement.clone())
            .map_err(SaveError::Environment)?;
        let persistence = self
            .environment_persistence
            .get(original_name)
            .cloned()
            .ok_or(SaveError::ReadOnlySource)?;
        let original_source = self
            .documents
            .get(&persistence.document_path)
            .expect("filesystem environment must retain its source document")
            .original_source
            .clone();
        Ok(PreparedEnvironmentReplace {
            baseline: self.prepared_baseline(),
            persistence,
            original_source,
            original_name: original_name.to_owned(),
            replacement,
            description: FieldPatch::Unchanged,
        })
    }

    /// Captures an environment replacement and an optional description change for one write.
    ///
    /// `Unchanged` keeps the stored description, as [`Self::prepare_environment_replace`] does.
    /// `Set` writes the documentation value, including explicit null and an empty string.
    /// `Clear` removes the YAML key. Structural edits and the description are persisted
    /// together; a conflict leaves the previous document unchanged.
    pub fn prepare_environment_replace_with_description(
        &self,
        original_name: &str,
        replacement: Environment,
        description: &FieldPatch<Documentation>,
    ) -> Result<PreparedEnvironmentReplace, SaveError> {
        let mut prepared = self.prepare_environment_replace(original_name, replacement)?;
        prepared.description = description.clone();
        Ok(prepared)
    }

    /// Applies a successfully persisted environment replacement to the in-memory workspace.
    pub fn complete_environment_replace(
        &mut self,
        saved: CompletedEnvironmentReplace,
    ) -> Result<(), SaveError> {
        self.check_baseline(saved.baseline)?;
        self.workspace
            .replace_environment(&saved.original_name, saved.replacement.clone())
            .expect("prepared environment replacement must remain valid");
        if !saved.description.is_unchanged() {
            self.workspace
                .set_environment_description(&saved.replacement.name, &saved.description)
                .expect("prepared environment description must remain valid");
        }
        let mut persistence = self
            .environment_persistence
            .remove(&saved.original_name)
            .expect("replaced environment must retain persistence metadata");
        if persistence.document_path != saved.document_path {
            self.documents.remove(&persistence.document_path);
            persistence.document_path = saved.document_path.clone();
        }
        self.environment_persistence
            .insert(saved.replacement.name, persistence);
        self.documents.insert(
            saved.document_path,
            SourceDocument {
                original_source: saved.serialized_source,
            },
        );
        self.advance_baseline();
        Ok(())
    }

    /// Deletes an environment in memory and atomically persists the OpenCollection document.
    pub fn delete_environment(&mut self, name: &str) -> Result<(), SaveError> {
        let prepared = self.prepare_environment_delete(name)?;
        let saved = prepared.execute()?;
        self.complete_environment_delete(saved)?;
        Ok(())
    }

    /// Captures deletion of an environment that has no children.
    pub fn prepare_environment_delete(
        &self,
        name: &str,
    ) -> Result<PreparedEnvironmentDelete, SaveError> {
        let mut candidate = self.workspace.environments().to_vec();
        probe_core::delete_environment(&mut candidate, name).map_err(SaveError::Environment)?;
        let persistence = self
            .environment_persistence
            .get(name)
            .cloned()
            .ok_or(SaveError::ReadOnlySource)?;
        let original_source = self
            .documents
            .get(&persistence.document_path)
            .expect("filesystem environment must retain its source document")
            .original_source
            .clone();
        Ok(PreparedEnvironmentDelete {
            baseline: self.prepared_baseline(),
            name: name.to_owned(),
            persistence,
            original_source,
        })
    }

    /// Applies a successfully persisted environment deletion in memory.
    pub fn complete_environment_delete(
        &mut self,
        saved: CompletedEnvironmentDelete,
    ) -> Result<(), SaveError> {
        self.check_baseline(saved.baseline)?;
        self.workspace
            .delete_environment(&saved.name)
            .expect("prepared environment deletion must remain valid");
        self.environment_persistence.remove(&saved.name);
        if let Some(serialized_source) = saved.serialized_source {
            self.documents.insert(
                saved.document_path.clone(),
                SourceDocument {
                    original_source: serialized_source,
                },
            );
            if let Some(removed_index) = saved.bundled_index {
                for persistence in self.environment_persistence.values_mut() {
                    if persistence.document_path == saved.document_path
                        && persistence
                            .bundled_index
                            .is_some_and(|index| index > removed_index)
                    {
                        persistence.bundled_index =
                            persistence.bundled_index.map(|index| index - 1);
                    }
                }
            }
        } else {
            self.documents.remove(&saved.document_path);
        }
        self.advance_baseline();
        Ok(())
    }

    fn persist_environment_mutation(
        &mut self,
        environment_name: &str,
        mutation: EnvironmentYamlMutation,
    ) -> Result<(), SaveError> {
        let prepared = self.prepare_environment_mutation(environment_name, mutation)?;
        let saved = prepared.execute()?;
        self.complete_environment_save(saved)?;
        Ok(())
    }

    fn prepare_environment_mutation(
        &self,
        environment_name: &str,
        mutation: EnvironmentYamlMutation,
    ) -> Result<PreparedEnvironmentSave, SaveError> {
        let persistence = self
            .environment_persistence
            .get(environment_name)
            .cloned()
            .ok_or(SaveError::ReadOnlySource)?;
        let original_source = self
            .documents
            .get(&persistence.document_path)
            .expect("filesystem environment must retain its source document")
            .original_source
            .clone();
        Ok(PreparedEnvironmentSave {
            baseline: self.prepared_baseline(),
            persistence,
            original_source,
            mutation,
        })
    }

    fn plain_environment_variable(
        &self,
        environment_name: &str,
        variable_name: &str,
    ) -> Option<&Variable> {
        self.workspace
            .environments()
            .iter()
            .find(|environment| environment.name == environment_name)?
            .variables
            .iter()
            .find_map(|variable| match variable {
                EnvironmentVariable::Plain(variable)
                    if variable.name.as_deref() == Some(variable_name) =>
                {
                    Some(variable)
                }
                _ => None,
            })
    }

    /// Returns requests in collection traversal order.
    #[must_use]
    pub fn requests(&self) -> &[LocatedRequest] {
        &self.requests
    }

    /// Returns folders in collection traversal order.
    #[must_use]
    pub fn folders(&self) -> &[LocatedFolder] {
        &self.folders
    }

    /// Resolves a repository-backed selector to a request key.
    #[must_use]
    pub fn request_key(&self, selector: &str) -> Option<RequestKey> {
        self.request_indices_by_selector
            .get(selector)
            .map(|index| self.requests[*index].key)
    }

    /// Resolves a repository-backed selector to a folder key.
    #[must_use]
    pub fn folder_key(&self, selector: &str) -> Option<FolderKey> {
        self.folder_indices_by_selector
            .get(selector)
            .map(|index| self.folders[*index].key)
    }

    /// Returns the stable selector for a request key.
    #[must_use]
    pub fn request_selector(&self, key: RequestKey) -> Option<&str> {
        self.requests
            .get(key.slot())
            .filter(|located| located.key == key)
            .map(LocatedRequest::selector)
    }

    /// Returns the stable selector for a folder key.
    #[must_use]
    pub fn folder_selector(&self, key: FolderKey) -> Option<&str> {
        self.folders
            .get(key.slot())
            .filter(|located| located.key == key)
            .map(LocatedFolder::selector)
    }

    /// Resolves a session item to its persistent selector.
    #[must_use]
    pub fn item_selector(&self, item: WorkspaceItemRef) -> Option<&str> {
        match item {
            WorkspaceItemRef::Request(key) => self.request_selector(key),
            WorkspaceItemRef::Folder(key) => self.folder_selector(key),
        }
    }

    /// Resolves a persistent selector with an expected kind to a session item.
    #[must_use]
    pub fn item_key(&self, kind: probe_core::ItemKind, selector: &str) -> Option<WorkspaceItemRef> {
        match kind {
            probe_core::ItemKind::Request => {
                self.request_key(selector).map(WorkspaceItemRef::Request)
            }
            probe_core::ItemKind::Folder => self.folder_key(selector).map(WorkspaceItemRef::Folder),
        }
    }

    /// Applies an update in memory and atomically persists its OpenCollection document.
    ///
    /// The save is rejected if the source file no longer exactly matches the bytes
    /// loaded by this repository instance. On persistence failure, the in-memory
    /// request remains updated so callers can report or retry the dirty state.
    pub fn update_request(
        &mut self,
        selector: &str,
        update: &RequestUpdate,
    ) -> Result<(), SaveError> {
        if update.is_empty() {
            return Err(SaveError::EmptyUpdate);
        }

        let located = self
            .requests
            .iter()
            .find(|request| request.selector.as_ref() == selector)
            .cloned()
            .ok_or_else(|| SaveError::RequestNotFound(selector.to_owned()))?;
        let request = self
            .workspace
            .request_mut(located.key)
            .expect("repository request key must resolve");
        let mut updated = request.clone();
        update.apply(&mut updated).map_err(SaveError::Protocol)?;
        *request = updated;

        let persistence = located.persistence.ok_or(SaveError::ReadOnlySource)?;
        let source = self
            .documents
            .get(&persistence.document_path)
            .expect("filesystem request must retain its source document");
        let diagnostic_prefix = self.request_diagnostic_prefix(&persistence.document_path);
        let mut refreshed_diagnostics = None;
        let serialized = mutate_existing_document(
            &persistence.document_path,
            &source.original_source,
            |document| {
                let request_document = request_document_mut(document, &persistence.item_path)?;
                apply_request_update(request_document, update)?;
                refreshed_diagnostics = Some(diagnostics_for_request_document(
                    document,
                    diagnostic_prefix.as_deref(),
                )?);
                Ok(())
            },
        )?;

        self.refresh_request_diagnostics(
            diagnostic_prefix.as_deref(),
            refreshed_diagnostics.unwrap(),
        );
        self.documents.insert(
            persistence.document_path,
            SourceDocument {
                original_source: serialized.into(),
            },
        );
        self.advance_baseline();
        Ok(())
    }

    /// Persists collection summary and docs without rewriting unrelated fields.
    pub fn update_collection(&mut self, update: &CollectionUpdate) -> Result<(), SaveError> {
        if update.is_empty() {
            return Err(SaveError::EmptyUpdate);
        }
        let document_path = self.collection_document_path()?;
        update.apply(self.workspace.metadata_mut());
        let original_source = self
            .documents
            .get(&document_path)
            .expect("collection document must retain its source")
            .original_source
            .clone();
        let serialized = mutate_existing_document(&document_path, &original_source, |document| {
            apply_collection_update(document, update)
        })?;
        self.documents.insert(
            document_path,
            SourceDocument {
                original_source: serialized.into(),
            },
        );
        self.advance_baseline();
        Ok(())
    }

    /// Persists folder description and docs without rewriting unrelated fields.
    pub fn update_folder(
        &mut self,
        selector: &str,
        update: &FolderUpdate,
    ) -> Result<(), SaveError> {
        if update.is_empty() {
            return Err(SaveError::EmptyUpdate);
        }
        let located = self
            .folders
            .iter()
            .find(|folder| folder.selector.as_ref() == selector)
            .cloned()
            .ok_or_else(|| SaveError::FolderNotFound(selector.to_owned()))?;
        let folder = self
            .workspace
            .folder_mut(located.key)
            .expect("repository folder key must resolve");
        update.description.apply(&mut folder.metadata.description);
        update.docs.apply(&mut folder.docs);

        let persistence = located.persistence.ok_or(SaveError::ReadOnlySource)?;
        let original_source = self
            .documents
            .get(&persistence.document_path)
            .expect("filesystem folder must retain its source document")
            .original_source
            .clone();
        let serialized =
            mutate_existing_document(&persistence.document_path, &original_source, |document| {
                let folder_document = request_document_mut(document, &persistence.item_path)?;
                apply_folder_update(folder_document, update)
            })?;
        self.documents.insert(
            persistence.document_path,
            SourceDocument {
                original_source: serialized.into(),
            },
        );
        self.advance_baseline();
        Ok(())
    }

    /// Captures a collection documentation write for background execution.
    pub fn prepare_collection_save(
        &self,
        update: CollectionUpdate,
    ) -> Result<PreparedDocumentationSave, SaveError> {
        if update.is_empty() {
            return Err(SaveError::EmptyUpdate);
        }
        let document_path = self.collection_document_path()?;
        Ok(PreparedDocumentationSave {
            baseline: self.prepared_baseline(),
            original_source: self.documents[&document_path].original_source.clone(),
            document_path,
            item_path: Vec::new(),
            update: DocumentationUpdate::Collection(update),
        })
    }

    /// Captures a folder documentation write for background execution.
    pub fn prepare_folder_save(
        &self,
        selector: &str,
        update: FolderUpdate,
    ) -> Result<PreparedDocumentationSave, SaveError> {
        if update.is_empty() {
            return Err(SaveError::EmptyUpdate);
        }
        let located = self
            .folders
            .iter()
            .find(|folder| folder.selector.as_ref() == selector)
            .ok_or_else(|| SaveError::FolderNotFound(selector.to_owned()))?;
        let persistence = located
            .persistence
            .as_ref()
            .ok_or(SaveError::ReadOnlySource)?;
        Ok(PreparedDocumentationSave {
            baseline: self.prepared_baseline(),
            document_path: persistence.document_path.clone(),
            original_source: self.documents[&persistence.document_path]
                .original_source
                .clone(),
            item_path: persistence.item_path.clone(),
            update: DocumentationUpdate::Folder(located.key, update),
        })
    }

    /// Integrates a successful documentation write without replacing request drafts.
    pub fn complete_documentation_save(
        &mut self,
        saved: CompletedDocumentationSave,
    ) -> Result<(), SaveError> {
        self.check_baseline(saved.baseline)?;
        match saved.update {
            DocumentationUpdate::Collection(update) => update.apply(self.workspace.metadata_mut()),
            DocumentationUpdate::Folder(key, update) => {
                let folder = self
                    .workspace
                    .folder_mut(key)
                    .expect("saved folder belongs to this baseline");
                update.description.apply(&mut folder.metadata.description);
                update.docs.apply(&mut folder.docs);
            }
        }
        self.documents.insert(
            saved.document_path,
            SourceDocument {
                original_source: saved.serialized_source,
            },
        );
        self.advance_baseline();
        Ok(())
    }

    fn collection_document_path(&self) -> Result<PathBuf, SaveError> {
        match &self.source {
            WorkspaceSource::Bundled(path) => Ok(path.clone()),
            WorkspaceSource::Unbundled(root) => self
                .documents
                .keys()
                .find(|path| {
                    path.parent() == Some(root.as_path())
                        && path.file_stem().and_then(|stem| stem.to_str()) == Some("opencollection")
                })
                .cloned()
                .ok_or_else(|| {
                    SaveError::InvalidDocument("collection root document is missing".to_owned())
                }),
            WorkspaceSource::Memory => Err(SaveError::ReadOnlySource),
        }
    }

    /// Captures a request save that can be executed away from the UI thread.
    ///
    /// Preparing is in-memory only. [`PreparedRequestSave::execute`] performs the
    /// conflict check and atomic filesystem write.
    pub fn prepare_request_save(
        &self,
        selector: &str,
        update: RequestUpdate,
    ) -> Result<PreparedRequestSave, SaveError> {
        if update.is_empty() {
            return Err(SaveError::EmptyUpdate);
        }
        let persistence = self
            .requests
            .iter()
            .find(|request| request.selector.as_ref() == selector)
            .ok_or_else(|| SaveError::RequestNotFound(selector.to_owned()))?
            .persistence
            .clone()
            .ok_or(SaveError::ReadOnlySource)?;
        let original_source = self
            .documents
            .get(&persistence.document_path)
            .expect("filesystem request must retain its source document")
            .original_source
            .clone();
        Ok(PreparedRequestSave {
            baseline: self.prepared_baseline(),
            diagnostic_prefix: self.request_diagnostic_prefix(&persistence.document_path),
            persistence,
            original_source,
            update,
        })
    }

    /// Refreshes the retained conflict baseline after a prepared save succeeds.
    pub fn complete_request_save(&mut self, saved: CompletedRequestSave) -> Result<(), SaveError> {
        self.check_baseline(saved.baseline)?;
        self.refresh_request_diagnostics(saved.diagnostic_prefix.as_deref(), saved.diagnostics);
        self.documents.insert(
            saved.document_path,
            SourceDocument {
                original_source: saved.serialized_source,
            },
        );
        self.advance_baseline();
        Ok(())
    }

    fn request_diagnostic_prefix(&self, document_path: &Path) -> Option<String> {
        match &self.source {
            WorkspaceSource::Unbundled(root) => {
                Some(loading::relative_selector(root, document_path))
            }
            WorkspaceSource::Bundled(_) | WorkspaceSource::Memory => None,
        }
    }

    fn refresh_request_diagnostics(
        &mut self,
        prefix: Option<&str>,
        refreshed: Vec<ProjectionDiagnostic>,
    ) {
        if let Some(prefix) = prefix {
            let document_prefix = format!("{prefix}/");
            self.diagnostics
                .retain(|diagnostic| !diagnostic.path.starts_with(&document_prefix));
            self.diagnostics.extend(refreshed);
            sort_diagnostics(&mut self.diagnostics);
        } else {
            self.diagnostics = refreshed;
        }
    }
}

#[derive(Debug)]
enum DocumentationUpdate {
    Collection(CollectionUpdate),
    Folder(FolderKey, FolderUpdate),
}

/// A documentation save captured for execution away from the UI thread.
#[derive(Debug)]
pub struct PreparedDocumentationSave {
    baseline: PreparedBaseline,
    document_path: PathBuf,
    original_source: Arc<[u8]>,
    item_path: Vec<usize>,
    update: DocumentationUpdate,
}

impl PreparedDocumentationSave {
    /// Checks the retained source and writes documentation atomically.
    pub fn execute(self) -> Result<CompletedDocumentationSave, SaveError> {
        self.baseline.check_live()?;
        let serialized =
            mutate_existing_document(&self.document_path, &self.original_source, |document| {
                match &self.update {
                    DocumentationUpdate::Collection(update) => {
                        apply_collection_update(document, update)
                    }
                    DocumentationUpdate::Folder(_, update) => apply_folder_update(
                        request_document_mut(document, &self.item_path)?,
                        update,
                    ),
                }
            })?;
        Ok(CompletedDocumentationSave {
            baseline: self.baseline.expected,
            document_path: self.document_path,
            serialized_source: serialized.into(),
            update: self.update,
        })
    }
}

/// Repository state returned by a successful documentation write.
#[derive(Debug)]
pub struct CompletedDocumentationSave {
    baseline: WorkspaceBaseline,
    document_path: PathBuf,
    serialized_source: Arc<[u8]>,
    update: DocumentationUpdate,
}

/// A filesystem save captured from a loaded workspace for background execution.
#[derive(Debug)]
pub struct PreparedRequestSave {
    baseline: PreparedBaseline,
    diagnostic_prefix: Option<String>,
    persistence: RequestPersistence,
    original_source: Arc<[u8]>,
    update: RequestUpdate,
}

impl PreparedRequestSave {
    /// Performs the exact-source check and atomic write.
    pub fn execute(self) -> Result<CompletedRequestSave, SaveError> {
        self.baseline.check_live()?;
        let mut refreshed_diagnostics = None;
        let serialized = mutate_existing_document(
            &self.persistence.document_path,
            &self.original_source,
            |document| {
                let request = request_document_mut(document, &self.persistence.item_path)?;
                apply_request_update(request, &self.update)?;
                refreshed_diagnostics = Some(diagnostics_for_request_document(
                    document,
                    self.diagnostic_prefix.as_deref(),
                )?);
                Ok(())
            },
        )?;
        Ok(CompletedRequestSave {
            baseline: self.baseline.expected,
            document_path: self.persistence.document_path,
            serialized_source: serialized.into(),
            diagnostic_prefix: self.diagnostic_prefix,
            diagnostics: refreshed_diagnostics.unwrap(),
        })
    }
}

/// The refreshed repository baseline produced by a successful prepared save.
#[derive(Debug)]
pub struct CompletedRequestSave {
    baseline: WorkspaceBaseline,
    document_path: PathBuf,
    serialized_source: Arc<[u8]>,
    diagnostic_prefix: Option<String>,
    diagnostics: Vec<ProjectionDiagnostic>,
}

/// A filesystem environment-variable save captured for background execution.
#[derive(Debug)]
pub struct PreparedEnvironmentSave {
    baseline: PreparedBaseline,
    persistence: EnvironmentPersistence,
    original_source: Arc<[u8]>,
    mutation: EnvironmentYamlMutation,
}

impl PreparedEnvironmentSave {
    /// Performs the exact-source check and atomic write.
    pub fn execute(self) -> Result<CompletedEnvironmentSave, SaveError> {
        self.baseline.check_live()?;
        let serialized =
            persist_environment_yaml(&self.persistence, &self.original_source, &self.mutation)?;
        Ok(CompletedEnvironmentSave {
            baseline: self.baseline.expected,
            document_path: self.persistence.document_path,
            serialized_source: serialized.into(),
        })
    }
}

/// The refreshed repository baseline produced by a successful environment save.
#[derive(Debug)]
pub struct CompletedEnvironmentSave {
    baseline: WorkspaceBaseline,
    document_path: PathBuf,
    serialized_source: Arc<[u8]>,
}

/// A validated environment replacement captured for background persistence.
#[derive(Debug)]
pub struct PreparedEnvironmentReplace {
    baseline: PreparedBaseline,
    persistence: EnvironmentPersistence,
    original_source: Arc<[u8]>,
    original_name: String,
    replacement: Environment,
    description: FieldPatch<Documentation>,
}

impl PreparedEnvironmentReplace {
    /// Performs the exact-source check and atomic write.
    pub fn execute(self) -> Result<CompletedEnvironmentReplace, SaveError> {
        self.baseline.check_live()?;
        let destination = unbundled_rename_destination(
            &self.persistence,
            &self.original_name,
            &self.replacement.name,
        );
        let (serialized, document_path) = match destination {
            Some(new_path) => persist_unbundled_environment_rename(
                &self.persistence.document_path,
                &new_path,
                &self.original_source,
                &self.replacement,
                &self.description,
            )?,
            None => {
                let serialized = persist_environment_replacement(
                    &self.persistence,
                    &self.original_source,
                    &self.replacement,
                    &self.description,
                )?;
                (serialized, self.persistence.document_path)
            }
        };
        Ok(CompletedEnvironmentReplace {
            baseline: self.baseline.expected,
            original_name: self.original_name,
            replacement: self.replacement,
            description: self.description,
            document_path,
            serialized_source: serialized.into(),
        })
    }
}

/// The refreshed repository baseline produced by an environment replacement.
#[derive(Debug)]
pub struct CompletedEnvironmentReplace {
    baseline: WorkspaceBaseline,
    original_name: String,
    replacement: Environment,
    description: FieldPatch<Documentation>,
    document_path: PathBuf,
    serialized_source: Arc<[u8]>,
}

/// A validated environment deletion captured for background persistence.
#[derive(Debug)]
pub struct PreparedEnvironmentDelete {
    baseline: PreparedBaseline,
    name: String,
    persistence: EnvironmentPersistence,
    original_source: Arc<[u8]>,
}

impl PreparedEnvironmentDelete {
    /// Performs the exact-source check and removes the environment.
    pub fn execute(self) -> Result<CompletedEnvironmentDelete, SaveError> {
        self.baseline.check_live()?;
        let document_path = self.persistence.document_path.clone();
        let (serialized_source, bundled_index) = match self.persistence.bundled_index {
            Some(index) => (
                Some(
                    persist_bundled_environment_delete(
                        &document_path,
                        &self.original_source,
                        index,
                    )?
                    .into(),
                ),
                Some(index),
            ),
            None => {
                persist_unbundled_environment_delete(&document_path, &self.original_source)?;
                (None, None)
            }
        };
        Ok(CompletedEnvironmentDelete {
            baseline: self.baseline.expected,
            name: self.name,
            document_path,
            serialized_source,
            bundled_index,
        })
    }
}

/// The refreshed repository baseline produced by an environment deletion.
#[derive(Debug)]
pub struct CompletedEnvironmentDelete {
    baseline: WorkspaceBaseline,
    name: String,
    document_path: PathBuf,
    serialized_source: Option<Arc<[u8]>>,
    bundled_index: Option<usize>,
}

/// A filesystem environment-create save captured for background execution.
#[derive(Debug)]
pub struct PreparedEnvironmentCreate {
    baseline: PreparedBaseline,
    environment: Environment,
    kind: EnvironmentCreateKind,
}

#[derive(Debug)]
enum EnvironmentCreateKind {
    Bundled {
        document_path: PathBuf,
        original_source: Arc<[u8]>,
        bundled_index: usize,
    },
    Unbundled {
        root: PathBuf,
    },
}

impl PreparedEnvironmentCreate {
    fn environment_name(&self) -> &str {
        &self.environment.name
    }

    /// Performs the conflict check and atomic write for a new environment document.
    pub fn execute(self) -> Result<CompletedEnvironmentCreate, SaveError> {
        self.baseline.check_live()?;
        let name = self.environment.name.clone();
        let (document_path, serialized, bundled_index) = match self.kind {
            EnvironmentCreateKind::Bundled {
                document_path,
                original_source,
                bundled_index,
            } => {
                let serialized = persist_bundled_environment_create(
                    &document_path,
                    &original_source,
                    &self.environment,
                )?;
                (document_path, serialized, Some(bundled_index))
            }
            EnvironmentCreateKind::Unbundled { root } => {
                let (document_path, serialized) =
                    persist_unbundled_environment_create(&root, &self.environment)?;
                (document_path, serialized, None)
            }
        };
        Ok(CompletedEnvironmentCreate {
            baseline: self.baseline.expected,
            name,
            document_path,
            serialized_source: serialized.into(),
            bundled_index,
        })
    }
}

/// The refreshed repository baseline produced by a successful environment create.
#[derive(Debug)]
pub struct CompletedEnvironmentCreate {
    baseline: WorkspaceBaseline,
    name: String,
    document_path: PathBuf,
    serialized_source: Arc<[u8]>,
    bundled_index: Option<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct EnvironmentPersistence {
    document_path: PathBuf,
    bundled_index: Option<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum EnvironmentYamlMutation {
    Set {
        variable: Variable,
    },
    Unset {
        name: String,
    },
    Description {
        description: FieldPatch<Documentation>,
    },
    Replace {
        environment: Environment,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RequestPersistence {
    document_path: PathBuf,
    item_path: Vec<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct FolderPersistence {
    document_path: PathBuf,
    item_path: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SourceDocument {
    pub(crate) original_source: Arc<[u8]>,
}

/// Loads a bundled OpenCollection file or an unbundled collection directory.
fn request_document_mut<'a>(
    document: &'a mut Value,
    item_path: &[usize],
) -> Result<&'a mut Value, SaveError> {
    let mut current = document;
    for index in item_path {
        let mapping = current.as_mapping_mut().ok_or_else(|| {
            SaveError::InvalidDocument("an item parent is not a mapping".to_owned())
        })?;
        let items = mapping
            .get_mut(Value::String("items".to_owned()))
            .and_then(Value::as_sequence_mut)
            .ok_or_else(|| {
                SaveError::InvalidDocument("an item parent has no items sequence".to_owned())
            })?;
        current = items.get_mut(*index).ok_or_else(|| {
            SaveError::InvalidDocument(format!("item index {index} is out of bounds"))
        })?;
    }
    Ok(current)
}

pub(crate) fn apply_request_update(
    document: &mut Value,
    update: &RequestUpdate,
) -> Result<(), SaveError> {
    let item_type = NativeItemType::from_value(document);
    let request = document.as_mapping_mut().ok_or_else(|| {
        SaveError::InvalidDocument("the request item is not a mapping".to_owned())
    })?;

    let Some(NativeItemType::Request(protocol)) = item_type else {
        let item_type = request
            .get("info")
            .and_then(|info| info.get("type"))
            .and_then(Value::as_str);
        return Err(SaveError::InvalidDocument(format!(
            "the request item has an unsupported or missing type: {}",
            item_type.unwrap_or("<missing>")
        )));
    };

    if let Some(name) = &update.name {
        let info = mapping_child(request, "info")?;
        info.insert(
            Value::String("name".to_owned()),
            Value::String(name.clone()),
        );
    }
    if !update.description.is_unchanged() {
        let info = mapping_child(request, "info")?;
        set_documentation(info, "description", &update.description);
    }
    if !update.docs.is_unchanged() {
        set_optional(
            request,
            "docs",
            match &update.docs {
                FieldPatch::Set(docs) => Some(Value::String(docs.clone())),
                FieldPatch::Clear => None,
                FieldPatch::Unchanged => unreachable!("docs patch was checked"),
            },
        );
    }
    let websocket = protocol == RequestProtocol::WebSocket;
    validate_websocket_fields(websocket, update)?;
    let details_name = protocol.as_str();
    if !update.method.is_unchanged()
        || !update.url.is_unchanged()
        || update.headers.is_some()
        || update.query_parameters.is_some()
        || update.path_parameters.is_some()
        || !update.body.is_unchanged()
        || !update.body_content.is_unchanged()
        || !update.authentication.is_unchanged()
        || update.graphql.is_some()
        || !update.websocket_message.is_unchanged()
    {
        let details = mapping_child(request, details_name)?;
        if !update.method.is_unchanged() {
            set_optional(
                details,
                "method",
                match &update.method {
                    FieldPatch::Set(method) => Some(Value::String(method.clone())),
                    FieldPatch::Clear => None,
                    FieldPatch::Unchanged => unreachable!(),
                },
            );
        }
        if !update.url.is_unchanged() {
            set_optional(
                details,
                "url",
                match &update.url {
                    FieldPatch::Set(url) => Some(Value::String(url.clone())),
                    FieldPatch::Clear => None,
                    FieldPatch::Unchanged => unreachable!(),
                },
            );
        }
        if let Some(headers) = &update.headers {
            merge_sequence_preserving(
                details,
                "headers",
                headers.iter().map(header_value).collect(),
                &[],
            );
        }
        if (update.query_parameters.is_some() || update.path_parameters.is_some()) && !websocket {
            merge_parameters(
                details,
                update.query_parameters.as_deref(),
                update.path_parameters.as_deref(),
            );
        }
        if !update.body.is_unchanged() {
            if protocol == RequestProtocol::Graphql {
                return Err(SaveError::InvalidDocument(
                    "HTTP body updates cannot be applied to a native GraphQL request".to_owned(),
                ));
            }
            let value = match &update.body {
                FieldPatch::Set(body) => Some(request_body_value(body)),
                FieldPatch::Clear => None,
                FieldPatch::Unchanged => unreachable!(),
            };
            set_optional_merged(details, "body", value);
        }
        if !update.body_content.is_unchanged() {
            if protocol == RequestProtocol::Graphql {
                return Err(SaveError::Protocol(
                    probe_core::RequestProtocolError::NotHttp,
                ));
            }
            match &update.body_content {
                FieldPatch::Set(body) => apply_http_body_content(details, body)?,
                FieldPatch::Clear => set_optional(details, "body", None),
                FieldPatch::Unchanged => unreachable!(),
            }
        }
        if !update.authentication.is_unchanged() {
            let value = match &update.authentication {
                FieldPatch::Set(authentication) => Some(authentication_value(authentication)),
                FieldPatch::Clear => None,
                FieldPatch::Unchanged => unreachable!(),
            };
            set_optional(details, "auth", value);
        }
        if let Some(graphql) = &update.graphql {
            if protocol != RequestProtocol::Graphql {
                return Err(SaveError::Protocol(
                    probe_core::RequestProtocolError::NotGraphql,
                ));
            }
            apply_graphql_update(details, graphql)?;
        }
        match &update.websocket_message {
            FieldPatch::Unchanged => {}
            FieldPatch::Set(message) => apply_websocket_message(details, message)?,
            FieldPatch::Clear => set_optional(details, "message", None),
        }
    }
    Ok(())
}

fn validate_websocket_fields(websocket: bool, update: &RequestUpdate) -> Result<(), SaveError> {
    if !websocket {
        return if update.websocket_message.is_unchanged() {
            Ok(())
        } else {
            Err(SaveError::Protocol(
                probe_core::RequestProtocolError::NotWebSocket,
            ))
        };
    }
    let field = if matches!(update.method, FieldPatch::Set(_)) {
        "an HTTP method"
    } else if update
        .query_parameters
        .as_ref()
        .is_some_and(|p| !p.is_empty())
    {
        "query parameters"
    } else if update
        .path_parameters
        .as_ref()
        .is_some_and(|p| !p.is_empty())
    {
        "path parameters"
    } else if !update.body.is_unchanged() || !update.body_content.is_unchanged() {
        "an HTTP body"
    } else {
        return Ok(());
    };
    Err(SaveError::Protocol(
        probe_core::RequestProtocolError::UnsupportedField {
            protocol: RequestProtocol::WebSocket,
            field,
        },
    ))
}

fn apply_websocket_message(
    details: &mut serde_yaml_ng::Mapping,
    message: &WebSocketMessage,
) -> Result<(), SaveError> {
    let key = string_key("message");
    let Some(Value::Sequence(variants)) = details.get_mut(&key) else {
        set_optional_merged(details, "message", Some(websocket_message_value(message)));
        return Ok(());
    };
    let mut selected = variants.iter_mut().filter(|variant| {
        variant
            .as_mapping()
            .and_then(|variant| variant.get(string_key("selected")))
            .and_then(Value::as_bool)
            .unwrap_or(false)
    });
    let variant = selected.next().ok_or_else(|| {
        SaveError::Protocol(probe_core::RequestProtocolError::InvalidBodySelection(
            "WebSocket message variants have no selected value".to_owned(),
        ))
    })?;
    if selected.next().is_some() {
        return Err(SaveError::Protocol(
            probe_core::RequestProtocolError::InvalidBodySelection(
                "WebSocket message variants have multiple selected values".to_owned(),
            ),
        ));
    }
    let variant = variant.as_mapping_mut().ok_or_else(|| {
        SaveError::InvalidDocument("WebSocket message variant is not a mapping".to_owned())
    })?;
    set_optional_merged(variant, "message", Some(websocket_message_value(message)));
    Ok(())
}

fn apply_http_body_content(
    details: &mut serde_yaml_ng::Mapping,
    body: &Body,
) -> Result<(), SaveError> {
    let key = string_key("body");
    match details.get(&key) {
        None => {
            details.insert(key, body_value(body));
            Ok(())
        }
        Some(Value::Mapping(_)) => {
            set_optional_merged(details, "body", Some(body_value(body)));
            Ok(())
        }
        Some(Value::Sequence(_)) => {
            let Some(Value::Sequence(variants)) = details.get_mut(&key) else {
                return Err(SaveError::InvalidDocument(
                    "HTTP body must be a mapping or variants".to_owned(),
                ));
            };
            let mut selected = variants.iter_mut().filter(|variant| {
                variant
                    .as_mapping()
                    .and_then(|variant| variant.get(string_key("selected")))
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
            });
            let variant = selected.next().ok_or_else(|| {
                SaveError::Protocol(probe_core::RequestProtocolError::InvalidBodySelection(
                    "request body variants have no selected value".to_owned(),
                ))
            })?;
            if selected.next().is_some() {
                return Err(SaveError::Protocol(
                    probe_core::RequestProtocolError::InvalidBodySelection(
                        "request body variants have multiple selected values".to_owned(),
                    ),
                ));
            }
            let variant = variant.as_mapping_mut().ok_or_else(|| {
                SaveError::InvalidDocument("HTTP body variant is not a mapping".to_owned())
            })?;
            set_optional_merged(variant, "body", Some(body_value(body)));
            Ok(())
        }
        Some(_) => Err(SaveError::InvalidDocument(
            "HTTP body must be a mapping or variants".to_owned(),
        )),
    }
}

fn apply_collection_update(
    document: &mut Value,
    update: &CollectionUpdate,
) -> Result<(), SaveError> {
    let root = document.as_mapping_mut().ok_or_else(|| {
        SaveError::InvalidDocument("the collection document is not a mapping".to_owned())
    })?;
    if !update.summary.is_unchanged() {
        let info = mapping_child(root, "info")?;
        set_optional(
            info,
            "summary",
            match &update.summary {
                FieldPatch::Set(summary) => Some(Value::String(summary.clone())),
                FieldPatch::Clear => None,
                FieldPatch::Unchanged => unreachable!("summary patch was checked"),
            },
        );
    }
    if !update.docs.is_unchanged() {
        set_documentation(root, "docs", &update.docs);
    }
    Ok(())
}

fn apply_folder_update(document: &mut Value, update: &FolderUpdate) -> Result<(), SaveError> {
    let folder = document
        .as_mapping_mut()
        .ok_or_else(|| SaveError::InvalidDocument("the folder item is not a mapping".to_owned()))?;
    if !update.description.is_unchanged() {
        let info = mapping_child(folder, "info")?;
        set_documentation(info, "description", &update.description);
    }
    if !update.docs.is_unchanged() {
        set_documentation(folder, "docs", &update.docs);
    }
    Ok(())
}

fn apply_graphql_update(
    graphql: &mut serde_yaml_ng::Mapping,
    update: &probe_core::GraphqlUpdate,
) -> Result<(), SaveError> {
    let body = graphql_body_mapping(graphql)?;
    if !update.query.is_unchanged() {
        set_optional(
            body,
            "query",
            match &update.query {
                FieldPatch::Set(query) => Some(Value::String(query.clone())),
                FieldPatch::Clear => None,
                FieldPatch::Unchanged => unreachable!(),
            },
        );
    }
    if !update.variables.is_unchanged() {
        let value = match &update.variables {
            FieldPatch::Set(variables) => Some(Value::String(
                serde_json::Value::Object(variables.clone()).to_string(),
            )),
            FieldPatch::Clear => None,
            FieldPatch::Unchanged => unreachable!(),
        };
        set_optional(body, "variables", value);
    }
    if !update.operation_name.is_unchanged() {
        let value = match &update.operation_name {
            FieldPatch::Set(operation_name) => Some(Value::String(operation_name.clone())),
            FieldPatch::Clear => None,
            FieldPatch::Unchanged => unreachable!(),
        };
        set_optional(body, "operationName", value);
    }
    if !update.extensions.is_unchanged() {
        let value = match &update.extensions {
            FieldPatch::Set(extensions) => Some(Value::String(
                serde_json::Value::Object(extensions.clone()).to_string(),
            )),
            FieldPatch::Clear => None,
            FieldPatch::Unchanged => unreachable!(),
        };
        set_optional(body, "extensions", value);
    }
    Ok(())
}

fn graphql_body_mapping(
    graphql: &mut serde_yaml_ng::Mapping,
) -> Result<&mut serde_yaml_ng::Mapping, SaveError> {
    let key = Value::String("body".to_owned());
    if !graphql.contains_key(&key) {
        graphql.insert(key.clone(), Value::Mapping(serde_yaml_ng::Mapping::new()));
    }
    let body = graphql.get_mut(&key).expect("GraphQL body was initialized");
    let variants = match body {
        Value::Mapping(body) => return Ok(body),
        Value::Sequence(variants) => variants,
        _ => {
            return Err(SaveError::InvalidDocument(
                "GraphQL body must be a mapping or variants".to_owned(),
            ));
        }
    };
    let mut selected = variants.iter_mut().filter(|variant| {
        variant
            .as_mapping()
            .and_then(|variant| variant.get(Value::String("selected".to_owned())))
            .and_then(Value::as_bool)
            .unwrap_or(false)
    });
    let variant = selected.next().ok_or_else(|| {
        SaveError::InvalidDocument("GraphQL body variants have no selected value".to_owned())
    })?;
    if selected.next().is_some() {
        return Err(SaveError::InvalidDocument(
            "GraphQL body variants have multiple selected values".to_owned(),
        ));
    }
    mapping_child(
        variant.as_mapping_mut().ok_or_else(|| {
            SaveError::InvalidDocument("GraphQL body variant is not a mapping".to_owned())
        })?,
        "body",
    )
}

fn mapping_child<'a>(
    parent: &'a mut serde_yaml_ng::Mapping,
    name: &str,
) -> Result<&'a mut serde_yaml_ng::Mapping, SaveError> {
    let key = Value::String(name.to_owned());
    if !parent.contains_key(&key) {
        parent.insert(key.clone(), Value::Mapping(serde_yaml_ng::Mapping::new()));
    }
    parent
        .get_mut(&key)
        .and_then(Value::as_mapping_mut)
        .ok_or_else(|| SaveError::InvalidDocument(format!("'{name}' is not a mapping")))
}

fn diagnostics_for_request_document(
    document: &Value,
    prefix: Option<&str>,
) -> Result<Vec<ProjectionDiagnostic>, SaveError> {
    let mut diagnostics = Vec::new();
    if let Some(prefix) = prefix {
        project_item(document.clone(), "item", &mut diagnostics)
            .map_err(|error| SaveError::InvalidDocument(error.to_string()))?;
        for diagnostic in &mut diagnostics {
            diagnostic.path = format!("{prefix}/{}", diagnostic.path);
        }
    } else if let Some(items) = document.get("items").and_then(Value::as_sequence) {
        project_items(items.clone(), "items", &mut diagnostics)
            .map_err(|error| SaveError::InvalidDocument(error.to_string()))?;
    }
    sort_diagnostics(&mut diagnostics);
    Ok(diagnostics)
}

fn mutate_existing_document(
    path: &Path,
    original_source: &[u8],
    mutate: impl FnOnce(&mut Value) -> Result<(), SaveError>,
) -> Result<Vec<u8>, SaveError> {
    let _save_lock = SaveLock::acquire(path)?;
    let current = fs::read(path).map_err(|source| SaveError::Io {
        path: path.to_owned(),
        source,
    })?;
    if current != original_source {
        return Err(SaveError::ConcurrentModification(path.to_owned()));
    }
    let mut document: Value = serde_yaml_ng::from_slice(original_source).map_err(|error| {
        SaveError::InvalidDocument(format!("retained source cannot be parsed: {error}"))
    })?;
    mutate(&mut document)?;
    let serialized = serde_yaml_ng::to_string(&document)
        .map_err(SaveError::Serialize)?
        .into_bytes();
    atomic_write(path, &serialized, original_source)?;
    Ok(serialized)
}

pub(crate) fn atomic_write(
    path: &Path,
    contents: &[u8],
    expected_source: &[u8],
) -> Result<(), SaveError> {
    let map_io = |source| SaveError::Io {
        path: path.to_owned(),
        source,
    };
    let mut file = AtomicWriteFile::open(path).map_err(map_io)?;
    file.write_all(contents).map_err(map_io)?;
    file.sync_all().map_err(map_io)?;
    let current = fs::read(path).map_err(map_io)?;
    if current != expected_source {
        return Err(SaveError::ConcurrentModification(path.to_owned()));
    }
    file.commit().map_err(map_io)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        LiveBaseline, SaveError, WorkspaceBaseline, create_bundled_workspace, load_workspace,
    };
    use std::{
        fs, process,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn request_updates_reject_non_request_item_types_without_mutating_document() {
        use probe_core::{Documentation, FieldPatch, RequestUpdate};
        use serde_yaml_ng::Value;

        let update = RequestUpdate {
            name: Some("Renamed".into()),
            description: FieldPatch::Set(Documentation::Text("New description".into())),
            docs: FieldPatch::Set("New docs".into()),
            url: FieldPatch::Set("https://example.com/updated".into()),
            ..RequestUpdate::default()
        };
        for item_type in ["type: grpc", "type: folder", "", "type: 42"] {
            let mut document: Value = serde_yaml_ng::from_str(&format!(
                "info: {{ name: Original, {item_type} }}\nx-unknown: retained\n"
            ))
            .unwrap();
            let original = document.clone();
            assert!(matches!(
                super::apply_request_update(&mut document, &update),
                Err(SaveError::InvalidDocument(_))
            ));
            assert_eq!(document, original);
        }
    }

    #[test]
    fn live_repository_baselines_have_distinct_runtime_identity() {
        let baseline = WorkspaceBaseline::fresh();
        let live = LiveBaseline::new(baseline);
        let separate = LiveBaseline::new(baseline);
        assert_eq!(live, live);
        assert_ne!(live, separate);
    }

    #[test]
    fn stale_environment_create_does_not_change_reloaded_workspace() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "probe-stale-environment-create-{}-{unique}.yml",
            process::id()
        ));
        let mut original = create_bundled_workspace(&path, Some("Stale create"), false).unwrap();
        let prepared = original
            .prepare_environment_create("staging".to_owned(), None)
            .unwrap();
        assert_eq!(original.workspace().environments()[0].name, "staging");

        let mut reloaded = load_workspace(&path).unwrap();
        let environments_before = reloaded.workspace().environments().to_vec();
        let persistence_before = reloaded.environment_persistence.clone();
        let documents_before = reloaded.documents.clone();
        let baseline_before = reloaded.baseline;

        let completed = prepared.execute().unwrap();
        assert!(matches!(
            reloaded.complete_environment_create(completed),
            Err(SaveError::CommittedButNotIntegrated)
        ));
        assert_eq!(reloaded.workspace().environments(), environments_before);
        assert_eq!(reloaded.environment_persistence, persistence_before);
        assert_eq!(reloaded.documents, documents_before);
        assert_eq!(reloaded.baseline, baseline_before);

        fs::remove_file(path).unwrap();
    }

    #[test]
    fn http_body_content_rejects_unusable_variant_lists() {
        use probe_core::{Body, RawBody, RawBodyKind};

        let body = Body::Raw(RawBody {
            kind: RawBodyKind::Text,
            data: "next".to_owned(),
        });
        let mut missing = serde_yaml_ng::Mapping::new();
        missing.insert(
            super::string_key("body"),
            serde_yaml_ng::from_str(
                "- { title: One, body: { type: text, data: a } }\n- { title: Two, body: { type: text, data: b } }\n",
            )
            .unwrap(),
        );
        let error = super::apply_http_body_content(&mut missing, &body).unwrap_err();
        assert!(error.to_string().contains("no selected value"));

        let mut several = serde_yaml_ng::Mapping::new();
        several.insert(
            super::string_key("body"),
            serde_yaml_ng::from_str(
                "- { title: One, selected: true, body: { type: text, data: a } }\n- { title: Two, selected: true, body: { type: text, data: b } }\n",
            )
            .unwrap(),
        );
        let error = super::apply_http_body_content(&mut several, &body).unwrap_err();
        assert!(error.to_string().contains("multiple selected values"));

        let mut scalar = serde_yaml_ng::Mapping::new();
        scalar.insert(super::string_key("body"), serde_yaml_ng::Value::Bool(true));
        let error = super::apply_http_body_content(&mut scalar, &body).unwrap_err();
        assert!(error.to_string().contains("mapping or variants"));
    }
}
