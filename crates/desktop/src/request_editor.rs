use std::collections::BTreeMap;

use probe_core::{
    Authentication, AuthenticationKind, AuthenticationValue, Body, HttpRequest, QueryParameter,
    RawBody, RawBodyKind, RequestBody, RequestKey, synchronize_path_parameters,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum EditorSection {
    #[default]
    Path,
    Query,
    Headers,
    Body,
    Authentication,
}

impl EditorSection {
    pub(crate) const ALL: [Self; 5] = [
        Self::Path,
        Self::Query,
        Self::Headers,
        Self::Body,
        Self::Authentication,
    ];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Query => "Query",
            Self::Path => "Path",
            Self::Headers => "Headers",
            Self::Body => "Body",
            Self::Authentication => "Authentication",
        }
    }
}

pub(crate) fn url_bar_value(request: &HttpRequest) -> String {
    let url = request.url.as_deref().unwrap_or_default();
    let query = request
        .query_parameters
        .iter()
        .filter(|parameter| !parameter.disabled)
        .map(|parameter| {
            format!(
                "{}={}",
                encode_query_component(&parameter.name),
                encode_query_component(&parameter.value)
            )
        })
        .collect::<Vec<_>>()
        .join("&");
    if query.is_empty() {
        return url.to_owned();
    }
    let (before_fragment, fragment) = url.split_once('#').unwrap_or((url, ""));
    let separator = if before_fragment.contains('?') {
        '&'
    } else {
        '?'
    };
    let fragment_separator = if fragment.is_empty() { "" } else { "#" };
    format!("{before_fragment}{separator}{query}{fragment_separator}{fragment}")
}

pub(crate) fn apply_url_bar_value(request: &mut HttpRequest, value: &str) {
    let (without_fragment, fragment) = value.split_once('#').unwrap_or((value, ""));
    let (url, query) = without_fragment
        .split_once('?')
        .unwrap_or((without_fragment, ""));
    request.url = Some(if fragment.is_empty() {
        url.to_owned()
    } else {
        format!("{url}#{fragment}")
    });
    let disabled = request
        .query_parameters
        .iter()
        .filter(|parameter| parameter.disabled)
        .cloned();
    let enabled = query
        .split('&')
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            let (name, value) = entry.split_once('=').unwrap_or((entry, ""));
            QueryParameter {
                name: decode_query_component(name),
                value: decode_query_component(value),
                disabled: false,
            }
        });
    request.query_parameters = enabled.chain(disabled).collect();
    synchronize_path_parameters(request);
}

fn encode_query_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'{' | b'}') {
            encoded.push(char::from(byte));
        } else {
            use std::fmt::Write as _;
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

fn decode_query_component(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) = (hex(bytes[index + 1]), hex(bytes[index + 2]))
        {
            decoded.push(high * 16 + low);
            index += 3;
        } else {
            decoded.push(if bytes[index] == b'+' {
                b' '
            } else {
                bytes[index]
            });
            index += 1;
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

const fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[derive(Debug, Default)]
pub(crate) struct RequestEditorState {
    pub(crate) section: EditorSection,
    body_drafts: BTreeMap<(RequestKey, BodyEditorKind), RequestBody>,
}

impl RequestEditorState {
    pub(crate) fn clear(&mut self) {
        self.section = EditorSection::default();
        self.body_drafts.clear();
    }

    pub(crate) fn remap_requests(&mut self, keys: &BTreeMap<RequestKey, RequestKey>) {
        self.body_drafts = std::mem::take(&mut self.body_drafts)
            .into_iter()
            .filter_map(|((old_key, kind), body)| {
                keys.get(&old_key)
                    .copied()
                    .map(|new_key| ((new_key, kind), body))
            })
            .collect();
    }

    pub(crate) fn switch_body_kind(
        &mut self,
        key: RequestKey,
        request: &mut HttpRequest,
        next_kind: BodyEditorKind,
    ) {
        let previous_body = request.body.take();
        let previous_kind = previous_body.as_ref().and_then(BodyEditorKind::from_body);
        if previous_kind == Some(next_kind) {
            request.body = previous_body;
            return;
        }
        if let (Some(previous_kind), Some(previous_body)) = (previous_kind, previous_body.as_ref())
        {
            self.body_drafts
                .insert((key, previous_kind), previous_body.clone());
        }

        request.body = match next_kind {
            BodyEditorKind::None => None,
            _ => self
                .body_drafts
                .remove(&(key, next_kind))
                .or_else(|| new_body_for_kind(next_kind, previous_body.as_ref())),
        };
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum BodyEditorKind {
    None,
    Json,
    Text,
    Xml,
    Sparql,
    Form,
    Multipart,
    File,
}

impl BodyEditorKind {
    fn from_body(body: &RequestBody) -> Option<Self> {
        match body {
            RequestBody::Single(Body::Raw(raw)) => Some(match raw.kind {
                RawBodyKind::Json => Self::Json,
                RawBodyKind::Text => Self::Text,
                RawBodyKind::Xml => Self::Xml,
                RawBodyKind::Sparql => Self::Sparql,
            }),
            RequestBody::Single(Body::FormUrlEncoded(_)) => Some(Self::Form),
            RequestBody::Single(Body::Multipart(_)) => Some(Self::Multipart),
            RequestBody::Single(Body::File(_)) => Some(Self::File),
            RequestBody::Variants(_) => None,
        }
    }
}

fn new_body_for_kind(
    kind: BodyEditorKind,
    previous_body: Option<&RequestBody>,
) -> Option<RequestBody> {
    let previous_raw_data = match previous_body {
        Some(RequestBody::Single(Body::Raw(raw))) => raw.data.clone(),
        _ => String::new(),
    };
    let body = match kind {
        BodyEditorKind::None => return None,
        BodyEditorKind::Json => Body::Raw(RawBody {
            kind: RawBodyKind::Json,
            data: previous_raw_data,
        }),
        BodyEditorKind::Text => Body::Raw(RawBody {
            kind: RawBodyKind::Text,
            data: previous_raw_data,
        }),
        BodyEditorKind::Xml => Body::Raw(RawBody {
            kind: RawBodyKind::Xml,
            data: previous_raw_data,
        }),
        BodyEditorKind::Sparql => Body::Raw(RawBody {
            kind: RawBodyKind::Sparql,
            data: previous_raw_data,
        }),
        BodyEditorKind::Form => Body::FormUrlEncoded(Vec::new()),
        BodyEditorKind::Multipart => Body::Multipart(Vec::new()),
        BodyEditorKind::File => Body::File(Vec::new()),
    };
    Some(RequestBody::Single(body))
}

pub(crate) fn body_kind(request: &HttpRequest) -> &'static str {
    match request.body.as_ref() {
        None => "None",
        Some(RequestBody::Single(Body::Raw(RawBody {
            kind: RawBodyKind::Json,
            ..
        }))) => "JSON",
        Some(RequestBody::Single(Body::Raw(RawBody {
            kind: RawBodyKind::Text,
            ..
        }))) => "Text",
        Some(RequestBody::Single(Body::Raw(RawBody {
            kind: RawBodyKind::Xml,
            ..
        }))) => "XML",
        Some(RequestBody::Single(Body::Raw(RawBody {
            kind: RawBodyKind::Sparql,
            ..
        }))) => "SPARQL",
        Some(RequestBody::Single(Body::FormUrlEncoded(_))) => "Form",
        Some(RequestBody::Single(Body::Multipart(_))) => "Multipart",
        Some(RequestBody::Single(Body::File(_))) => "File",
        Some(RequestBody::Variants(_)) => "Variants",
    }
}

pub(crate) fn raw_body_mut(request: &mut HttpRequest) -> Option<&mut String> {
    match request.body.as_mut() {
        Some(RequestBody::Single(Body::Raw(raw))) => Some(&mut raw.data),
        _ => None,
    }
}

pub(crate) fn auth_label(kind: &AuthenticationKind) -> &'static str {
    match kind {
        AuthenticationKind::Inherit => "Inherit",
        AuthenticationKind::Basic => "Basic",
        AuthenticationKind::Bearer => "Bearer",
        AuthenticationKind::ApiKey => "API Key",
        AuthenticationKind::OAuth1 => "OAuth 1",
        AuthenticationKind::OAuth2 => "OAuth 2",
        AuthenticationKind::AwsV4 => "AWS v4",
        AuthenticationKind::Wsse => "WSSE",
        AuthenticationKind::Digest => "Digest",
        AuthenticationKind::Ntlm => "NTLM",
        AuthenticationKind::Other(_) => "Other",
    }
}

pub(crate) fn set_authentication(request: &mut HttpRequest, kind: Option<AuthenticationKind>) {
    if request.authentication.as_ref().map(|auth| &auth.kind) == kind.as_ref() {
        return;
    }
    request.authentication = kind.map(|kind| Authentication {
        kind,
        properties: BTreeMap::new(),
    });
}

pub(crate) fn set_auth_property(request: &mut HttpRequest, name: String, value: String) {
    let Some(authentication) = request.authentication.as_mut() else {
        return;
    };
    authentication
        .properties
        .insert(name, AuthenticationValue::String(value));
}

pub(crate) fn auth_value(value: &AuthenticationValue) -> String {
    match value {
        AuthenticationValue::String(value) | AuthenticationValue::Number(value) => value.clone(),
        AuthenticationValue::Boolean(value) => value.to_string(),
        AuthenticationValue::Null => String::new(),
        AuthenticationValue::Sequence(_) => "[complex value]".to_owned(),
        AuthenticationValue::Object(_) => "{complex value}".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use probe_core::{
        AuthenticationKind, Body, HttpRequest, QueryParameter, RawBodyKind, RequestBody,
    };

    use super::{
        BodyEditorKind, RequestEditorState, apply_url_bar_value, raw_body_mut, set_auth_property,
        set_authentication, url_bar_value,
    };

    #[test]
    fn url_bar_includes_enabled_query_values_before_the_fragment() {
        let request = HttpRequest {
            url: Some("https://api.example.com/users/:userId#results".to_owned()),
            query_parameters: vec![
                QueryParameter {
                    name: "search".to_owned(),
                    value: "hello world".to_owned(),
                    disabled: false,
                },
                QueryParameter {
                    name: "hidden".to_owned(),
                    value: "no".to_owned(),
                    disabled: true,
                },
            ],
            ..HttpRequest::default()
        };

        assert_eq!(
            url_bar_value(&request),
            "https://api.example.com/users/:userId?search=hello%20world#results"
        );
    }

    #[test]
    fn editing_the_url_bar_updates_query_values_without_losing_disabled_rows() {
        let mut request = HttpRequest {
            query_parameters: vec![QueryParameter {
                name: "hidden".to_owned(),
                value: "no".to_owned(),
                disabled: true,
            }],
            ..HttpRequest::default()
        };

        apply_url_bar_value(
            &mut request,
            "https://api.example.com/users/:userId?search=hello%20world#results",
        );

        assert_eq!(
            request.url.as_deref(),
            Some("https://api.example.com/users/:userId#results")
        );
        assert_eq!(request.query_parameters.len(), 2);
        assert_eq!(request.query_parameters[0].name, "search");
        assert_eq!(request.query_parameters[0].value, "hello world");
        assert!(request.query_parameters[1].disabled);
        assert_eq!(request.path_parameters.len(), 1);
        assert_eq!(request.path_parameters[0].name, "userId");
        assert_eq!(request.path_parameters[0].value, "");
    }

    #[test]
    fn url_bar_path_variables_reuse_values_deduplicate_and_remove_stale_rows() {
        let mut request = HttpRequest {
            path_parameters: vec![
                QueryParameter {
                    name: "userId".to_owned(),
                    value: "42".to_owned(),
                    disabled: false,
                },
                QueryParameter {
                    name: "stale".to_owned(),
                    value: "old".to_owned(),
                    disabled: false,
                },
                QueryParameter {
                    name: "saved".to_owned(),
                    value: "later".to_owned(),
                    disabled: true,
                },
            ],
            ..HttpRequest::default()
        };

        apply_url_bar_value(
            &mut request,
            "https://api.example.com/users/:userId/posts/:postId/:userId",
        );

        assert_eq!(request.path_parameters.len(), 3);
        assert_eq!(request.path_parameters[0].name, "userId");
        assert_eq!(request.path_parameters[0].value, "42");
        assert_eq!(request.path_parameters[1].name, "postId");
        assert_eq!(request.path_parameters[1].value, "");
        assert_eq!(request.path_parameters[2].name, "saved");
        assert!(request.path_parameters[2].disabled);
    }

    #[test]
    fn raw_body_edits_update_the_request_immediately() {
        let mut request = HttpRequest::default();
        RequestEditorState::default().switch_body_kind(
            request_key(),
            &mut request,
            BodyEditorKind::Json,
        );
        raw_body_mut(&mut request)
            .unwrap()
            .push_str("{\"ok\":true}");
        let Some(RequestBody::Single(Body::Raw(body))) = request.body else {
            panic!("expected a raw body");
        };
        assert_eq!(body.kind, RawBodyKind::Json);
        assert_eq!(body.data, "{\"ok\":true}");
    }

    #[test]
    fn authentication_edits_update_the_request_immediately() {
        let mut request = HttpRequest::default();
        set_authentication(&mut request, Some(AuthenticationKind::Bearer));
        set_auth_property(&mut request, "token".to_owned(), "secret".to_owned());
        let authentication = request.authentication.unwrap();
        assert_eq!(authentication.kind, AuthenticationKind::Bearer);
        assert_eq!(
            authentication.properties.get("token"),
            Some(&probe_core::AuthenticationValue::String(
                "secret".to_owned()
            ))
        );
    }

    #[test]
    fn structured_body_modes_are_created_without_replacing_the_active_mode() {
        let mut request = HttpRequest::default();
        let mut editor = RequestEditorState::default();
        editor.switch_body_kind(request_key(), &mut request, BodyEditorKind::Form);
        assert!(matches!(
            request.body,
            Some(RequestBody::Single(Body::FormUrlEncoded(_)))
        ));
        editor.switch_body_kind(request_key(), &mut request, BodyEditorKind::Multipart);
        assert!(matches!(
            request.body,
            Some(RequestBody::Single(Body::Multipart(_)))
        ));
        editor.switch_body_kind(request_key(), &mut request, BodyEditorKind::File);
        assert!(matches!(
            request.body,
            Some(RequestBody::Single(Body::File(_)))
        ));
    }

    #[test]
    fn switching_body_kinds_restores_each_kinds_content() {
        let key = request_key();
        let mut editor = RequestEditorState::default();
        let mut request = HttpRequest::default();
        editor.switch_body_kind(key, &mut request, BodyEditorKind::Json);
        raw_body_mut(&mut request)
            .unwrap()
            .push_str("{\"json\":true}");
        editor.switch_body_kind(key, &mut request, BodyEditorKind::Form);
        let Some(RequestBody::Single(Body::FormUrlEncoded(fields))) = request.body.as_mut() else {
            panic!("expected form body");
        };
        fields.push(probe_core::FormField {
            name: "name".to_owned(),
            value: "value".to_owned(),
            disabled: false,
        });

        editor.switch_body_kind(key, &mut request, BodyEditorKind::Json);
        assert_eq!(raw_body_mut(&mut request).unwrap(), "{\"json\":true}");
        editor.switch_body_kind(key, &mut request, BodyEditorKind::Form);
        let Some(RequestBody::Single(Body::FormUrlEncoded(fields))) = request.body else {
            panic!("expected restored form body");
        };
        assert_eq!(fields[0].value, "value");
    }

    #[test]
    fn body_drafts_follow_remapped_runtime_keys() {
        let old_key = request_key();
        let new_key = replacement_request_key();
        let mut editor = RequestEditorState::default();
        let mut request = HttpRequest::default();
        editor.switch_body_kind(old_key, &mut request, BodyEditorKind::Json);
        raw_body_mut(&mut request)
            .unwrap()
            .push_str("{\"draft\":true}");
        editor.switch_body_kind(old_key, &mut request, BodyEditorKind::Form);
        editor.remap_requests(&BTreeMap::from([(old_key, new_key)]));

        let mut reloaded = HttpRequest::default();
        editor.switch_body_kind(new_key, &mut reloaded, BodyEditorKind::Json);
        assert_eq!(raw_body_mut(&mut reloaded).unwrap(), "{\"draft\":true}");
    }

    fn request_key() -> probe_core::RequestKey {
        let workspace = probe_core::Workspace::from_collection(probe_core::Collection {
            items: vec![probe_core::CollectionItem::HttpRequest(
                HttpRequest::default(),
            )],
            ..probe_core::Collection::default()
        });
        let [probe_core::WorkspaceItemRef::Request(key)] = workspace.root_items() else {
            panic!("fixture should contain one request");
        };
        *key
    }

    fn replacement_request_key() -> probe_core::RequestKey {
        let mut workspace = probe_core::Workspace::from_collection(probe_core::Collection {
            items: vec![probe_core::CollectionItem::HttpRequest(
                HttpRequest::default(),
            )],
            ..probe_core::Collection::default()
        });
        let [probe_core::WorkspaceItemRef::Request(old_key)] = workspace.root_items() else {
            panic!("fixture should contain one request");
        };
        let old_key = *old_key;
        workspace.remove_request(old_key).unwrap();
        workspace.add_root_request(HttpRequest::default())
    }
}
