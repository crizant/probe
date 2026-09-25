use probe_core::Collection;

use crate::{
    ProjectionDiagnostic, ProjectionDiagnosticKind,
    document::{CollectionDocument, EnvironmentDocument},
};

mod authentication;
mod body;
mod request;

pub(crate) use request::{project_item, project_items};

pub(crate) fn project_collection(
    document: CollectionDocument,
    diagnostics: &mut Vec<ProjectionDiagnostic>,
) -> Result<Collection, serde_yaml_ng::Error> {
    Ok(Collection {
        metadata: document.info.into_domain(),
        items: project_items(document.items, "items", diagnostics)?,
        environments: document
            .config
            .environments
            .into_iter()
            .map(EnvironmentDocument::into_domain)
            .collect(),
    })
}

pub(super) fn diagnostic(
    diagnostics: &mut Vec<ProjectionDiagnostic>,
    path: String,
    kind: ProjectionDiagnosticKind,
    value: impl Into<String>,
) {
    diagnostics.push(ProjectionDiagnostic {
        path,
        kind,
        value: value.into(),
    });
}

pub(crate) fn sort_diagnostics(diagnostics: &mut [ProjectionDiagnostic]) {
    diagnostics.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.kind.as_str().cmp(right.kind.as_str()))
            .then_with(|| left.value.cmp(&right.value))
    });
}
