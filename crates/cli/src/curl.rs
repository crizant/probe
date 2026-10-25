//! cURL input and atomic creation through the shared parser and repository.

use std::{fs, io::Read, path::Path};

use probe_application::parse_curl;
use probe_core::{FieldPatch, RequestProtocol, RequestUpdate};
use probe_opencollection::StructureOperation;

use crate::{CliError, CommandOutput, WorkspaceInput, error::invalid_import, structure};

pub(crate) fn import(
    input: &WorkspaceInput,
    name: String,
    parent: Option<String>,
    index: Option<usize>,
    file: Option<&Path>,
    stdin: &mut impl Read,
) -> Result<CommandOutput, CliError> {
    // Never evaluate the command or include input/read-error contents in diagnostics.
    let source = if let Some(file) = file {
        let bytes = fs::read(file)
            .map_err(|_| invalid_import("cannot read cURL command file".to_owned()))?;
        String::from_utf8(bytes)
            .map_err(|_| invalid_import("cURL command file is not valid UTF-8".to_owned()))?
    } else {
        let mut source = String::new();
        stdin.read_to_string(&mut source).map_err(|_| CliError {
            category: "stdin_error",
            message: "cannot read cURL command from stdin as UTF-8".to_owned(),
            exit_code: crate::INVALID_WORKSPACE_EXIT_CODE,
            details: None,
        })?;
        source
    };
    let request = parse_curl(&source).map_err(|error| invalid_import(error.to_string()))?;
    let update = RequestUpdate {
        headers: Some(request.headers.clone()),
        body: FieldPatch::from_optional(request.http_body().cloned()),
        authentication: FieldPatch::from_optional(request.authentication.clone()),
        ..Default::default()
    };
    structure::edit(
        input,
        "import",
        StructureOperation::CreateRequest {
            parent,
            index,
            name,
            method: request.method,
            url: request.url,
            settings: Some(request.settings),
            protocol: RequestProtocol::Http,
            graphql: None,
            update: Some(update),
        },
        stdin,
    )
}
