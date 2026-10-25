//! Staged portable imports. Inspection and persistence perform blocking filesystem
//! work; callers must schedule these operations away from their UI thread.

use std::{error::Error, fmt, path::Path};

use probe_core::{Collection, ImportDiagnostic};
use probe_opencollection::{
    CreateError, LoadedWorkspace, create_bundled_workspace_from_collection,
};
use probe_postman::{
    PostmanCollectionSummary, PostmanImportError, PostmanImportPreview, PostmanSourceFormat,
    inspect_postman_source,
};
use probe_yaak::{
    YaakImportError, YaakImportPreview, YaakSourceFormat, YaakWorkspaceSummary, inspect_yaak_source,
};

/// Portable source selected by the caller.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImportSource {
    Postman,
    Yaak,
}

/// Inspected source, retained so strict conversion can be retried with consent.
#[derive(Debug)]
pub enum ImportConversion {
    Postman(Box<PostmanImportPreview>),
    Yaak {
        preview: Box<YaakImportPreview>,
        workspace_id: Option<String>,
    },
}

impl ImportConversion {
    #[must_use]
    pub const fn source(&self) -> ImportSource {
        match self {
            Self::Postman(_) => ImportSource::Postman,
            Self::Yaak { .. } => ImportSource::Yaak,
        }
    }

    /// Converts without writing a destination. Yaak selection errors and strict
    /// compatibility diagnostics retain their original provider-specific types.
    pub fn convert(&self, allow_partial: bool) -> Result<CollectionImport, ImportError> {
        match self {
            Self::Postman(preview) => {
                let imported = preview
                    .convert(allow_partial)
                    .map_err(ImportError::Postman)?;
                Ok(CollectionImport {
                    metadata: ImportMetadata::Postman {
                        format: preview.format(),
                        collection: imported.source,
                    },
                    collection: imported.collection,
                    diagnostics: imported.diagnostics,
                    partial: imported.partial,
                    selected_environment: imported.collection_variables_environment,
                })
            }
            Self::Yaak {
                preview,
                workspace_id,
            } => {
                let imported = preview
                    .convert(workspace_id.as_deref(), allow_partial)
                    .map_err(ImportError::Yaak)?;
                Ok(CollectionImport {
                    metadata: ImportMetadata::Yaak {
                        format: preview.format(),
                        workspace: imported.workspace,
                    },
                    collection: imported.collection,
                    diagnostics: imported.diagnostics,
                    partial: imported.partial,
                    selected_environment: imported.default_environment,
                })
            }
        }
    }
}

/// Reads and inspects a source without converting it or creating a destination.
pub fn inspect_import(
    source: ImportSource,
    path: impl AsRef<Path>,
) -> Result<ImportConversion, ImportError> {
    match source {
        ImportSource::Postman => inspect_postman_source(path)
            .map(|preview| ImportConversion::Postman(Box::new(preview)))
            .map_err(ImportError::Postman),
        ImportSource::Yaak => inspect_yaak_source(path)
            .map(|preview| ImportConversion::Yaak {
                preview: Box::new(preview),
                workspace_id: None,
            })
            .map_err(ImportError::Yaak),
    }
}

/// Source identity and exact detected format, without erasing provider metadata.
#[derive(Debug)]
pub enum ImportMetadata {
    Postman {
        format: PostmanSourceFormat,
        collection: PostmanCollectionSummary,
    },
    Yaak {
        format: YaakSourceFormat,
        workspace: YaakWorkspaceSummary,
    },
}

/// Converted collection and complete compatibility report, ready to persist.
#[derive(Debug)]
pub struct CollectionImport {
    pub metadata: ImportMetadata,
    pub collection: Collection,
    pub diagnostics: Vec<ImportDiagnostic>,
    pub partial: bool,
    pub selected_environment: Option<String>,
}

impl CollectionImport {
    #[must_use]
    pub const fn source(&self) -> ImportSource {
        match self.metadata {
            ImportMetadata::Postman { .. } => ImportSource::Postman,
            ImportMetadata::Yaak { .. } => ImportSource::Yaak,
        }
    }

    #[must_use]
    pub fn source_name(&self) -> &str {
        match &self.metadata {
            ImportMetadata::Postman { collection, .. } => &collection.name,
            ImportMetadata::Yaak { workspace, .. } => &workspace.name,
        }
    }

    /// Atomically creates a bundled workspace, refusing to overwrite any destination.
    pub fn create_workspace(
        &self,
        destination: impl AsRef<Path>,
    ) -> Result<LoadedWorkspace, CreateError> {
        create_bundled_workspace_from_collection(destination, &self.collection)
    }
}

/// Original provider error, preserved for frontend error and exit-code mapping.
#[derive(Debug)]
pub enum ImportError {
    Postman(PostmanImportError),
    Yaak(YaakImportError),
}

impl ImportError {
    #[must_use]
    pub fn diagnostics(&self) -> Option<&[ImportDiagnostic]> {
        match self {
            Self::Postman(error) => error.diagnostics(),
            Self::Yaak(error) => error.diagnostics(),
        }
    }
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Postman(error) => error.fmt(f),
            Self::Yaak(error) => error.fmt(f),
        }
    }
}

impl Error for ImportError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(match self {
            Self::Postman(error) => error,
            Self::Yaak(error) => error,
        })
    }
}
