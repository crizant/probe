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
        Some(&request.id),
        &request.extra,
        diagnostics,
    );
    let ancestors = folder_ancestors(request.folder_id.as_deref(), folders);
    let headers = effective_headers(workspace, &ancestors, request, resource_type, diagnostics);
    let mut authentication =
        effective_authentication(workspace, &ancestors, request, resource_type, diagnostics);
    if let Some(auth) = &mut authentication {
        if auth.kind == AuthenticationKind::ApiKey {
            // Yaak calls OpenCollection's placement property location.
            let placement = auth
                .properties
                .remove("location")
                .unwrap_or_else(|| AuthenticationValue::String("header".to_owned()));
            auth.properties.insert("placement".to_owned(), placement);
        }
        if auth.kind == AuthenticationKind::Bearer
            && let Some(prefix) = auth.properties.get("prefix")
            && prefix != &AuthenticationValue::String("Bearer".to_owned())
        {
            diagnostics.push(lossy(
                "unsupported_authentication",
                resource_type,
                Some(&request.id),
                Some("authentication.prefix"),
                "custom bearer prefixes cannot be represented by native Bearer authentication",
            ));
        }
    }
    let mut settings =
        effective_settings(workspace, &ancestors, request, resource_type, diagnostics);
    settings.follow_redirects = None;
    let mut message_size = workspace
        .setting_request_message_size
        .unwrap_or(64 * 1024 * 1024);
    for folder in &ancestors {
        override_setting(&folder.setting_request_message_size, &mut message_size);
    }
    override_setting(&request.setting_request_message_size, &mut message_size);
    for (unsupported, field, message) in [
        (
            !request.url_parameters.is_empty(),
            "urlParameters",
            "separate WebSocket URL parameters cannot be represented by OpenCollection",
        ),
        (
            message_size != 64 * 1024 * 1024,
            "settingRequestMessageSize",
            "custom WebSocket message size limits cannot be represented by OpenCollection",
        ),
        (
            request
                .setting_follow_redirects
                .as_ref()
                .is_some_and(|setting| setting.enabled == Some(true)),
            "settingFollowRedirects",
            "WebSocket redirect settings cannot be represented by OpenCollection",
        ),
    ] {
        if unsupported {
            diagnostics.push(lossy(
                "unsupported_field",
                resource_type,
                Some(&request.id),
                Some(field),
                message,
            ));
        }
    }
    CollectionItem::Request(Request {
        metadata: ItemMetadata {
            name: nonempty(&request.name),
            sequence: Some(request.sort_priority),
            ..ItemMetadata::default()
        },
        docs: nonempty(&request.description),
        url: Some(convert_templates(
            &request.url,
            resource_type,
            &request.id,
            "url",
            diagnostics,
        )),
        headers,
        authentication,
        settings,
        kind: RequestKind::WebSocket {
            message: (!request.message.is_empty()).then(|| {
                WebSocketMessageSet::Single(WebSocketMessage {
                    kind: WebSocketMessageKind::Text,
                    data: convert_templates(
                        &request.message,
                        resource_type,
                        &request.id,
                        "message",
                        diagnostics,
                    ),
                })
            }),
        },
        ..Request::default()
    })
}
