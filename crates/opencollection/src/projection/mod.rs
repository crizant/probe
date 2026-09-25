use crate::{ProjectionDiagnostic, ProjectionDiagnosticKind};

mod authentication;
mod body;
mod request;

pub(crate) use request::{project_item, project_items};

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
