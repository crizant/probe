use super::*;
use probe_core::{
    Request, RequestKind, WebSocketMessage, WebSocketMessageKind, WebSocketMessageSet,
};
use request::{
    effective_authentication, effective_headers, effective_settings, folder_ancestors,
    override_setting,
};

pub(super) fn convert_request(
    workspace: &YaakWorkspace,
    request: &YaakWebSocketRequest,
    folders: &BTreeMap<&str, &YaakFolder>,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> CollectionItem {
    let resource_type = "websocket_request";
    diagnose_extra_fields(
        resource_type,
        Some(&request.common.id),
        &request.extra,
        diagnostics,
    );
    let ancestors = folder_ancestors(request.common.folder_id.as_deref(), folders);
    let headers = effective_headers(
        workspace,
        &ancestors,
        &request.common,
        resource_type,
        diagnostics,
    );
    let authentication = effective_authentication(
        workspace,
        &ancestors,
        &request.common,
        resource_type,
        &[
            AuthenticationKind::Basic,
            AuthenticationKind::Bearer,
            AuthenticationKind::ApiKey,
        ],
        diagnostics,
    );
    // Collect compatibility diagnostics; Yaak ignores HTTP timeout and redirects for WebSockets.
    effective_settings(
        workspace,
        &ancestors,
        &request.common,
        resource_type,
        diagnostics,
    );
    let mut message_size = workspace
        .setting_request_message_size
        .unwrap_or(64 * 1024 * 1024);
    for folder in &ancestors {
        override_setting(&folder.setting_request_message_size, &mut message_size);
    }
    override_setting(&request.setting_request_message_size, &mut message_size);
    for (unsupported, field, message) in [
        (
            !request.common.url_parameters.is_empty(),
            "urlParameters",
            "separate WebSocket URL parameters cannot be represented by OpenCollection",
        ),
        (
            message_size != 64 * 1024 * 1024,
            "settingRequestMessageSize",
            "custom WebSocket message size limits cannot be represented by OpenCollection",
        ),
    ] {
        if unsupported {
            diagnostics.push(lossy(
                "unsupported_field",
                resource_type,
                Some(&request.common.id),
                Some(field),
                message,
            ));
        }
    }
    CollectionItem::Request(Request {
        metadata: ItemMetadata {
            name: nonempty(&request.common.name),
            sequence: Some(request.common.sort_priority),
            ..ItemMetadata::default()
        },
        docs: nonempty(&request.common.description),
        url: Some(convert_templates(
            &request.common.url,
            resource_type,
            &request.common.id,
            "url",
            diagnostics,
        )),
        headers,
        authentication,
        kind: RequestKind::WebSocket {
            message: (!request.message.is_empty()).then(|| {
                WebSocketMessageSet::Single(WebSocketMessage {
                    kind: WebSocketMessageKind::Text,
                    data: convert_templates(
                        &request.message,
                        resource_type,
                        &request.common.id,
                        "message",
                        diagnostics,
                    ),
                })
            }),
        },
        ..Request::default()
    })
}
