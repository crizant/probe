//! OpenCollection YAML adapter for Probe.
//!
//! The adapter retains the source document for loss-preserving serialization and
//! projects the supported subset into serialization-independent domain models.

#![forbid(unsafe_code)]

use std::{error::Error as StdError, fmt};

use probe_core::{Collection, validate_environments};
use serde_yaml_ng::Value;

mod document;
mod projection;
mod repository;
mod structure;

pub use repository::{
    CompletedEnvironmentCreate, CompletedEnvironmentDelete, CompletedEnvironmentReplace,
    CompletedEnvironmentSave, CompletedRequestSave, CreateError, LoadError, LoadedWorkspace,
    LocatedFolder, LocatedRequest, PreparedEnvironmentCreate, PreparedEnvironmentDelete,
    PreparedEnvironmentReplace, PreparedEnvironmentSave, PreparedRequestSave, SaveError,
    create_bundled_workspace, create_bundled_workspace_from_collection, load_workspace,
    load_workspace_from_str,
};
pub use structure::{
    CreatedRequestProtocol, ItemKind, StructureError, StructureOperation, StructureResult,
};

/// An OpenCollection document together with its supported domain projection.
#[derive(Clone, Debug)]
pub struct ParsedCollection {
    collection: Collection,
    document: Value,
    bundled: bool,
    diagnostics: Vec<ProjectionDiagnostic>,
}

impl ParsedCollection {
    pub(crate) const fn document(&self) -> &Value {
        &self.document
    }

    pub(crate) const fn is_bundled(&self) -> bool {
        self.bundled
    }

    /// Returns the serialization-independent collection model.
    #[must_use]
    pub const fn collection(&self) -> &Collection {
        &self.collection
    }

    /// Source values Probe cannot project or execute, while retaining their YAML.
    #[must_use]
    pub fn diagnostics(&self) -> &[ProjectionDiagnostic] {
        &self.diagnostics
    }

    /// Consumes the parsed document and returns its domain model.
    #[must_use]
    pub fn into_collection(self) -> Collection {
        self.collection
    }

    /// Serializes the retained OpenCollection document back to YAML.
    ///
    /// Unsupported fields are emitted from the retained document rather than rebuilt
    /// from the supported domain projection.
    pub fn to_yaml(&self) -> Result<String, ParseError> {
        serde_yaml_ng::to_string(&self.document).map_err(ParseError::new)
    }
}

/// A retained source value that Probe cannot project, or projects but cannot execute.
/// Authentication kinds and properties may remain in the domain while the HTTP
/// engine ignores them. The public diagnostic codes cover both limitations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectionDiagnostic {
    /// Structural path within a bundled document, or a workspace-relative file path.
    pub path: String,
    /// Stable category of unsupported value.
    pub kind: ProjectionDiagnosticKind,
    /// The unsupported type or property name.
    pub value: String,
}

/// Categories of unsupported OpenCollection projection or execution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProjectionDiagnosticKind {
    ItemType,
    BodyType,
    ParameterType,
    AuthenticationKind,
    AuthenticationProperty,
}

impl ProjectionDiagnosticKind {
    /// Stable machine-readable diagnostic code.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ItemType => "unsupported_item_type",
            Self::BodyType => "unsupported_body_type",
            Self::ParameterType => "unsupported_parameter_type",
            Self::AuthenticationKind => "unsupported_authentication_kind",
            Self::AuthenticationProperty => "unsupported_authentication_property",
        }
    }
}

/// An error raised while parsing or serializing OpenCollection YAML.
#[derive(Debug)]
pub struct ParseError {
    source: serde_yaml_ng::Error,
}

impl ParseError {
    fn new(source: serde_yaml_ng::Error) -> Self {
        Self { source }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid OpenCollection YAML: {}", self.source)
    }
}

impl StdError for ParseError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        Some(&self.source)
    }
}

/// Parses a bundled OpenCollection YAML document.
///
/// Unsupported items and fields remain in the retained YAML document. Projection
/// diagnostics identify values that cannot be represented or executed by Probe.
pub fn parse(source: &str) -> Result<ParsedCollection, ParseError> {
    let document: Value = serde_yaml_ng::from_str(source).map_err(ParseError::new)?;
    let wire: document::CollectionDocument =
        serde_yaml_ng::from_value(document.clone()).map_err(ParseError::new)?;
    if wire.opencollection != "1.0.0" {
        return Err(ParseError::new(
            <serde_yaml_ng::Error as serde::de::Error>::custom(format!(
                "unsupported OpenCollection version '{}'; supported version is 1.0.0",
                wire.opencollection
            )),
        ));
    }
    let bundled = wire.bundled;
    let mut diagnostics = Vec::new();
    let collection =
        projection::project_collection(wire, &mut diagnostics).map_err(ParseError::new)?;
    projection::sort_diagnostics(&mut diagnostics);
    validate_environments(&collection.environments).map_err(|error| {
        ParseError::new(<serde_yaml_ng::Error as serde::de::Error>::custom(
            error.to_string(),
        ))
    })?;

    Ok(ParsedCollection {
        collection,
        document,
        bundled,
        diagnostics,
    })
}
