use std::collections::{BTreeMap, HashMap};

use probe_core::{
    Authentication, AuthenticationKind, AuthenticationValue, Body, QueryParameter, RawBody,
    RawBodyKind, Request, RequestBody, RequestKey, RequestKind, synchronize_path_parameters,
};

pub(crate) const ASSERTION_OPERATORS: [&str; 8] = [
    "equals",
    "notEquals",
    "contains",
    "notContains",
    "greaterThan",
    "lessThan",
    "isNull",
    "isNotNull",
];

#[derive(Clone, Copy)]
pub(crate) enum AssertionField {
    Expression,
    Operator,
    Value,
    Description,
}

impl AssertionField {
    pub(crate) const fn id(self) -> &'static str {
        match self {
            Self::Expression => "assertion-expression",
            Self::Operator => "assertion-operator-value",
            Self::Value => "assertion-value",
            Self::Description => "assertion-description",
        }
    }
}

pub(crate) enum AssertionEdit {
    Add,
    Remove(usize),
    Move(usize, isize),
    Enabled(usize, bool),
    Text(usize, AssertionField, String),
}

impl AssertionEdit {
    pub(crate) fn apply(self, request: &mut Request) {
        if request.kind.is_websocket() {
            return;
        }
        if matches!(self, Self::Add) {
            request
                .runtime
                .assertions
                .get_or_insert_default()
                .push(probe_core::Assertion {
                    expression: "response.status".into(),
                    operator: "equals".into(),
                    value: Some("200".into()),
                    disabled: None,
                    description: None,
                });
            return;
        }
        let Some(rows) = request.runtime.assertions.as_mut() else {
            return;
        };
        match self {
            Self::Add => unreachable!(),
            Self::Remove(index) => {
                if index < rows.len() {
                    rows.remove(index);
                }
            }
            Self::Move(index, delta) => {
                if let Some(target) = index.checked_add_signed(delta)
                    && index < rows.len()
                    && target < rows.len()
                {
                    rows.swap(index, target);
                }
            }
            Self::Enabled(index, enabled) => {
                if let Some(row) = rows.get_mut(index)
                    && (row.disabled != Some(true)) != enabled
                {
                    row.disabled = Some(!enabled);
                }
            }
            Self::Text(index, field, text) => {
                let Some(row) = rows.get_mut(index) else {
                    return;
                };
                match field {
                    AssertionField::Expression => row.expression = text,
                    AssertionField::Operator => row.operator = text,
                    AssertionField::Value => {
                        if row.value.as_deref().unwrap_or_default() != text {
                            row.value = Some(text);
                        }
                    }
                    AssertionField::Description => match &mut row.description {
                        Some(probe_core::Documentation::Content { content, .. }) => *content = text,
                        Some(probe_core::Documentation::Text(value)) => *value = text,
                        _ if !text.is_empty() => {
                            row.description = Some(probe_core::Documentation::Text(text))
                        }
                        _ => {}
                    },
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum EditorSection {
    #[default]
    Path,
    Query,
    Headers,
    Body,
    Authentication,
    Docs,
    Assertions,
    GraphqlQuery,
    GraphqlVariables,
    GraphqlOperationName,
    GraphqlExtensions,
    WebSocketMessage,
}

impl EditorSection {
    pub(crate) const ALL_HTTP: [Self; 7] = [
        Self::Path,
        Self::Query,
        Self::Headers,
        Self::Body,
        Self::Authentication,
        Self::Docs,
        Self::Assertions,
    ];

    pub(crate) const ALL_GRAPHQL: [Self; 10] = [
        Self::Path,
        Self::Query,
        Self::Headers,
        Self::GraphqlQuery,
        Self::GraphqlVariables,
        Self::GraphqlOperationName,
        Self::GraphqlExtensions,
        Self::Authentication,
        Self::Docs,
        Self::Assertions,
    ];

    pub(crate) const ALL_WEBSOCKET: [Self; 4] = [
        Self::WebSocketMessage,
        Self::Headers,
        Self::Authentication,
        Self::Docs,
    ];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Query => "Query",
            Self::Path => "Path",
            Self::Headers => "Headers",
            Self::Body => "Body",
            Self::Authentication => "Authentication",
            Self::Docs => "Documentation",
            Self::Assertions => "Assertions",
            Self::GraphqlQuery => "Document",
            Self::GraphqlVariables => "Variables",
            Self::GraphqlOperationName => "Operation name",
            Self::GraphqlExtensions => "Extensions",
            Self::WebSocketMessage => "Message",
        }
    }

    pub(crate) const fn is_graphql(self) -> bool {
        matches!(
            self,
            Self::GraphqlQuery
                | Self::GraphqlVariables
                | Self::GraphqlOperationName
                | Self::GraphqlExtensions
        )
    }

    pub(crate) fn available_for(self, protocol: probe_core::RequestProtocol) -> bool {
        Self::for_protocol(protocol).contains(&self)
    }

    pub(crate) fn for_protocol(protocol: probe_core::RequestProtocol) -> &'static [Self] {
        match protocol {
            probe_core::RequestProtocol::Graphql => &Self::ALL_GRAPHQL,
            probe_core::RequestProtocol::Http => &Self::ALL_HTTP,
            probe_core::RequestProtocol::WebSocket => &Self::ALL_WEBSOCKET,
        }
    }
}

pub(crate) fn url_bar_value(request: &Request) -> String {
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

pub(crate) fn apply_url_bar_value(request: &mut Request, value: &str) {
    if request.kind.is_websocket() {
        request.url = Some(value.to_owned());
        return;
    }
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
    sections: HashMap<RequestKey, EditorSection>,
    body_drafts: BTreeMap<(RequestKey, BodyEditorKind), RequestBody>,
}

impl RequestEditorState {
    pub(crate) fn clear(&mut self) {
        self.sections.clear();
        self.body_drafts.clear();
    }

    pub(crate) fn section(&self, key: RequestKey) -> EditorSection {
        self.sections.get(&key).copied().unwrap_or_default()
    }

    pub(crate) fn set_section(&mut self, key: RequestKey, section: EditorSection) {
        self.sections.insert(key, section);
    }

    pub(crate) fn remove(&mut self, key: RequestKey) {
        self.sections.remove(&key);
    }

    pub(crate) fn ensure_available_section(
        &mut self,
        key: RequestKey,
        protocol: probe_core::RequestProtocol,
    ) {
        if self.section(key).available_for(protocol) {
            return;
        }
        self.set_section(
            key,
            match protocol {
                probe_core::RequestProtocol::Graphql => EditorSection::GraphqlQuery,
                probe_core::RequestProtocol::Http => EditorSection::Body,
                probe_core::RequestProtocol::WebSocket => EditorSection::WebSocketMessage,
            },
        );
    }

    pub(crate) fn remap_requests(&mut self, keys: &BTreeMap<RequestKey, RequestKey>) {
        self.sections = std::mem::take(&mut self.sections)
            .into_iter()
            .filter_map(|(old_key, section)| keys.get(&old_key).map(|new_key| (*new_key, section)))
            .collect();
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
        request: &mut Request,
        next_kind: BodyEditorKind,
    ) {
        let RequestKind::Http { body } = &mut request.kind else {
            return;
        };
        let previous_body = body.take();
        let previous_kind = previous_body.as_ref().and_then(BodyEditorKind::from_body);
        if previous_kind == Some(next_kind) {
            *body = previous_body;
            return;
        }
        if let (Some(previous_kind), Some(previous_body)) = (previous_kind, previous_body.as_ref())
        {
            self.body_drafts
                .insert((key, previous_kind), previous_body.clone());
        }

        *body = match next_kind {
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

pub(crate) fn body_kind(request: &Request) -> &'static str {
    match request.http_body() {
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

pub(crate) fn set_websocket_message(
    request: &mut Request,
    kind: Option<probe_core::WebSocketMessageKind>,
    data: Option<String>,
) {
    use probe_core::{WebSocketMessage, WebSocketMessageSet};
    let RequestKind::WebSocket { message } = &mut request.kind else {
        return;
    };
    match message {
        Some(WebSocketMessageSet::Variants(variants)) => {
            if variants.iter().filter(|v| v.selected).count() != 1 {
                return;
            }
            let Some(kind) = kind else {
                return;
            };
            let selected = variants.iter_mut().find(|v| v.selected).unwrap();
            selected.message.kind = kind;
            if let Some(data) = data {
                selected.message.data = data;
            }
        }
        other => {
            let previous = match other {
                Some(WebSocketMessageSet::Single(m)) => m.data.clone(),
                _ => String::new(),
            };
            *other = kind.map(|kind| {
                WebSocketMessageSet::Single(WebSocketMessage {
                    kind,
                    data: data.unwrap_or(previous),
                })
            });
        }
    }
}

pub(crate) fn raw_body_mut(request: &mut Request) -> Option<&mut String> {
    match request.http_body_mut() {
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

pub(crate) fn set_authentication(request: &mut Request, kind: Option<AuthenticationKind>) {
    if request.authentication.as_ref().map(|auth| &auth.kind) == kind.as_ref() {
        return;
    }
    request.authentication = kind.map(|kind| Authentication {
        kind,
        properties: BTreeMap::new(),
    });
}

pub(crate) fn set_auth_property(request: &mut Request, name: String, value: String) {
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

    use probe_core::{AuthenticationKind, Body, QueryParameter, RawBodyKind, Request, RequestBody};

    use super::{
        BodyEditorKind, EditorSection, RequestEditorState, apply_url_bar_value, raw_body_mut,
        set_auth_property, set_authentication, url_bar_value,
    };

    #[test]
    fn url_bar_includes_enabled_query_values_before_the_fragment() {
        let request = Request {
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
            ..Request::default()
        };

        assert_eq!(
            url_bar_value(&request),
            "https://api.example.com/users/:userId?search=hello%20world#results"
        );
    }

    #[test]
    fn editing_the_url_bar_updates_query_values_without_losing_disabled_rows() {
        let mut request = Request {
            query_parameters: vec![QueryParameter {
                name: "hidden".to_owned(),
                value: "no".to_owned(),
                disabled: true,
            }],
            ..Request::default()
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
        let mut request = Request {
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
            ..Request::default()
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
        let mut request = Request::default();
        RequestEditorState::default().switch_body_kind(
            request_key(),
            &mut request,
            BodyEditorKind::Json,
        );
        raw_body_mut(&mut request)
            .unwrap()
            .push_str("{\"ok\":true}");
        let Some(RequestBody::Single(Body::Raw(body))) = request.http_body() else {
            panic!("expected a raw body");
        };
        assert_eq!(body.kind, RawBodyKind::Json);
        assert_eq!(body.data, "{\"ok\":true}");
    }

    #[test]
    fn authentication_edits_update_the_request_immediately() {
        let mut request = Request::default();
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
        let mut request = Request::default();
        let mut editor = RequestEditorState::default();
        editor.switch_body_kind(request_key(), &mut request, BodyEditorKind::Form);
        assert!(matches!(
            request.http_body(),
            Some(RequestBody::Single(Body::FormUrlEncoded(_)))
        ));
        editor.switch_body_kind(request_key(), &mut request, BodyEditorKind::Multipart);
        assert!(matches!(
            request.http_body(),
            Some(RequestBody::Single(Body::Multipart(_)))
        ));
        editor.switch_body_kind(request_key(), &mut request, BodyEditorKind::File);
        assert!(matches!(
            request.http_body(),
            Some(RequestBody::Single(Body::File(_)))
        ));
    }

    #[test]
    fn switching_body_kinds_restores_each_kinds_content() {
        let key = request_key();
        let mut editor = RequestEditorState::default();
        let mut request = Request::default();
        editor.switch_body_kind(key, &mut request, BodyEditorKind::Json);
        raw_body_mut(&mut request)
            .unwrap()
            .push_str("{\"json\":true}");
        editor.switch_body_kind(key, &mut request, BodyEditorKind::Form);
        let Some(RequestBody::Single(Body::FormUrlEncoded(fields))) = request.http_body_mut()
        else {
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
        let Some(RequestBody::Single(Body::FormUrlEncoded(fields))) = request.http_body() else {
            panic!("expected restored form body");
        };
        assert_eq!(fields[0].value, "value");
    }

    #[test]
    fn body_drafts_follow_remapped_runtime_keys() {
        let old_key = request_key();
        let new_key = replacement_request_key();
        let mut editor = RequestEditorState::default();
        let mut request = Request::default();
        editor.switch_body_kind(old_key, &mut request, BodyEditorKind::Json);
        raw_body_mut(&mut request)
            .unwrap()
            .push_str("{\"draft\":true}");
        editor.switch_body_kind(old_key, &mut request, BodyEditorKind::Form);
        editor.remap_requests(&BTreeMap::from([(old_key, new_key)]));

        let mut reloaded = Request::default();
        editor.switch_body_kind(new_key, &mut reloaded, BodyEditorKind::Json);
        assert_eq!(raw_body_mut(&mut reloaded).unwrap(), "{\"draft\":true}");
    }

    #[test]
    fn closing_a_tab_preserves_its_unsaved_body_kind_drafts() {
        let key = request_key();
        let mut editor = RequestEditorState::default();
        let mut request = Request::default();
        editor.switch_body_kind(key, &mut request, BodyEditorKind::Json);
        raw_body_mut(&mut request)
            .unwrap()
            .push_str("{\"draft\":true}");
        editor.switch_body_kind(key, &mut request, BodyEditorKind::Form);
        editor.set_section(key, EditorSection::Body);

        editor.remove(key);
        assert_eq!(editor.section(key), EditorSection::Path);
        editor.switch_body_kind(key, &mut request, BodyEditorKind::Json);
        assert_eq!(raw_body_mut(&mut request).unwrap(), "{\"draft\":true}");
    }

    #[test]
    fn ensure_available_section_resets_http_only_sections_for_graphql() {
        let key = request_key();
        let mut editor = RequestEditorState::default();
        editor.set_section(key, EditorSection::Body);
        editor.ensure_available_section(key, probe_core::RequestProtocol::Graphql);
        assert_eq!(editor.section(key), EditorSection::GraphqlQuery);
    }

    #[test]
    fn ensure_available_section_resets_graphql_only_sections_for_http() {
        let key = request_key();
        let mut editor = RequestEditorState::default();
        editor.set_section(key, EditorSection::GraphqlVariables);
        editor.ensure_available_section(key, probe_core::RequestProtocol::Http);
        assert_eq!(editor.section(key), EditorSection::Body);
    }

    #[test]
    fn websocket_requests_offer_only_shared_sections_and_keep_raw_urls() {
        let key = request_key();
        let mut editor = RequestEditorState::default();
        for section in [
            EditorSection::Body,
            EditorSection::Query,
            EditorSection::Path,
        ] {
            editor.set_section(key, section);
            editor.ensure_available_section(key, probe_core::RequestProtocol::WebSocket);
            assert_eq!(editor.section(key), EditorSection::WebSocketMessage);
        }
        editor.set_section(key, EditorSection::Authentication);
        editor.ensure_available_section(key, probe_core::RequestProtocol::WebSocket);
        assert_eq!(editor.section(key), EditorSection::Authentication);

        let mut request = Request {
            kind: probe_core::RequestKind::WebSocket { message: None },
            ..Request::default()
        };
        let url = "wss://{{host}}/rooms/:room?token={{token}}#live";
        apply_url_bar_value(&mut request, url);
        assert_eq!(request.url.as_deref(), Some(url));
        assert!(request.query_parameters.is_empty());
        assert!(request.path_parameters.is_empty());
        assert_eq!(url_bar_value(&request), url);
    }

    #[test]
    fn ensure_available_section_keeps_shared_sections() {
        let key = request_key();
        let mut editor = RequestEditorState::default();
        editor.set_section(key, EditorSection::Headers);
        editor.ensure_available_section(key, probe_core::RequestProtocol::Graphql);
        assert_eq!(editor.section(key), EditorSection::Headers);
        editor.ensure_available_section(key, probe_core::RequestProtocol::Http);
        assert_eq!(editor.section(key), EditorSection::Headers);
        editor.set_section(key, EditorSection::Docs);
        editor.ensure_available_section(key, probe_core::RequestProtocol::Graphql);
        editor.ensure_available_section(key, probe_core::RequestProtocol::Http);
        assert_eq!(editor.section(key), EditorSection::Docs);
    }

    #[test]
    fn sections_follow_open_request_keys_and_clear_on_close() {
        let first = request_key();
        let second = replacement_request_key();
        let mut editor = RequestEditorState::default();
        editor.set_section(first, EditorSection::Headers);
        editor.set_section(second, EditorSection::Query);
        assert_eq!(editor.section(first), EditorSection::Headers);
        assert_eq!(editor.section(second), EditorSection::Query);

        editor.remap_requests(&BTreeMap::from([(first, second)]));
        assert_eq!(editor.section(second), EditorSection::Headers);
        editor.remove(second);
        assert_eq!(editor.section(second), EditorSection::Path);
    }

    #[test]
    fn websocket_message_edits_preserve_variants_and_invalid_selection() {
        use probe_core::{
            RequestKind, WebSocketMessage, WebSocketMessageKind as Kind, WebSocketMessageSet,
            WebSocketMessageVariant,
        };
        let mut request = Request {
            kind: RequestKind::WebSocket { message: None },
            ..Default::default()
        };
        for kind in [Kind::Text, Kind::Json, Kind::Xml, Kind::Binary] {
            super::set_websocket_message(&mut request, Some(kind), Some("{{token}}".into()));
            assert_eq!(
                request.selected_websocket_message().unwrap().unwrap().kind,
                kind
            );
            assert_eq!(
                request.selected_websocket_message().unwrap().unwrap().data,
                "{{token}}"
            );
        }
        super::set_websocket_message(&mut request, Some(Kind::Text), None);
        assert_eq!(
            request.selected_websocket_message().unwrap().unwrap().data,
            "{{token}}",
            "changing message type preserves configured content"
        );
        super::set_websocket_message(&mut request, None, None);
        assert!(request.websocket_message().is_none());
        request.kind = RequestKind::WebSocket {
            message: Some(WebSocketMessageSet::Variants(vec![
                WebSocketMessageVariant {
                    title: "First".into(),
                    selected: false,
                    message: WebSocketMessage {
                        kind: Kind::Text,
                        data: "untouched".into(),
                    },
                },
                WebSocketMessageVariant {
                    title: "Selected".into(),
                    selected: true,
                    message: WebSocketMessage {
                        kind: Kind::Xml,
                        data: "<ping/>".into(),
                    },
                },
            ])),
        };
        let saved = request.clone();
        super::set_websocket_message(&mut request, Some(Kind::Json), Some("{}".into()));
        let patch = probe_core::RequestUpdate::between(Some(&saved), &request).unwrap();
        let mut persisted = saved.clone();
        patch.apply(&mut persisted).unwrap();
        assert_eq!(persisted, request);
        super::set_websocket_message(&mut request, None, None);
        assert_eq!(request, persisted, "None cannot delete a variant set");
        let RequestKind::WebSocket {
            message: Some(WebSocketMessageSet::Variants(variants)),
        } = &mut request.kind
        else {
            panic!();
        };
        variants[0].selected = true;
        let invalid = request.clone();
        super::set_websocket_message(&mut request, Some(Kind::Text), Some("destructive".into()));
        assert_eq!(request, invalid);
        assert!(request.selected_websocket_message().is_err());
    }

    #[test]
    fn websocket_message_dirty_patch_round_trips_native_yaml() {
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/opencollection/websocket.yml");
        let directory = tempfile::TempDir::new().unwrap();
        let path = directory.path().join("collection.yml");
        std::fs::copy(fixture, &path).unwrap();
        let mut loaded = probe_opencollection::load_workspace(&path).unwrap();
        let selector = loaded.requests()[1].selector().to_owned();
        let key = loaded.requests()[1].key();
        let saved = loaded.workspace().request(key).unwrap().clone();
        let mut edited = saved.clone();
        super::set_websocket_message(
            &mut edited,
            Some(probe_core::WebSocketMessageKind::Json),
            Some("{\"changed\":true}".into()),
        );
        let patch = probe_core::RequestUpdate::between(Some(&saved), &edited).unwrap();
        assert!(!patch.is_empty());
        loaded.update_request(&selector, &patch).unwrap();
        let reloaded = probe_opencollection::load_workspace(&path).unwrap();
        assert_eq!(
            reloaded
                .workspace()
                .request(reloaded.request_key(&selector).unwrap())
                .unwrap(),
            &edited
        );
        let yaml = std::fs::read_to_string(path).unwrap();
        assert!(yaml.contains("title: Greeting"));
        assert!(yaml.contains("title: Payload"));
        assert!(yaml.contains("title: Future"));
        assert!(yaml.contains("type: cbor"));
        assert!(yaml.contains("x-vendor: retained"));
    }

    #[test]
    fn assertion_edits_preserve_absence_optional_values_and_description_structure() {
        use super::{AssertionEdit as Edit, AssertionField as Field};
        use probe_core::{Assertion, Documentation};
        let mut request = Request::default();
        Edit::Text(0, Field::Value, String::new()).apply(&mut request);
        Edit::Remove(0).apply(&mut request);
        assert_eq!(request.runtime.assertions, None);
        request.runtime.assertions = Some(Vec::new());
        let empty = request.clone();
        Edit::Move(0, 1).apply(&mut request);
        assert_eq!(request, empty);
        let description = Documentation::Content {
            content: "original".into(),
            media_type: "text/markdown".into(),
        };
        request.runtime.assertions = Some(vec![Assertion {
            expression: "response.status".into(),
            operator: "futureOperator".into(),
            value: None,
            disabled: None,
            description: Some(description.clone()),
        }]);
        let original = request.clone();
        Edit::Enabled(0, true).apply(&mut request);
        Edit::Text(0, Field::Value, String::new()).apply(&mut request);
        assert_eq!(request, original);
        Edit::Text(0, Field::Expression, "response.body.value".into()).apply(&mut request);
        let row = &request.runtime.assertions.as_ref().unwrap()[0];
        assert_eq!(row.operator, "futureOperator");
        assert_eq!(row.description, Some(description));
        Edit::Text(0, Field::Value, "{{retained}}".into()).apply(&mut request);
        Edit::Text(0, Field::Operator, "isNull".into()).apply(&mut request);
        Edit::Text(0, Field::Description, "changed".into()).apply(&mut request);
        let row = &request.runtime.assertions.as_ref().unwrap()[0];
        assert_eq!(row.value.as_deref(), Some("{{retained}}"));
        assert!(
            matches!(&row.description, Some(Documentation::Content { content, media_type }) if content == "changed" && media_type == "text/markdown")
        );
        request.kind = probe_core::RequestKind::WebSocket { message: None };
        let websocket = request.clone();
        Edit::Add.apply(&mut request);
        Edit::Remove(0).apply(&mut request);
        assert_eq!(request, websocket);
        assert!(
            !super::EditorSection::Assertions.available_for(probe_core::RequestProtocol::WebSocket)
        );
    }

    #[test]
    fn assertion_remove_and_move_respect_both_index_bounds() {
        use super::{AssertionEdit as Edit, AssertionField as Field};
        let mut request = Request::default();
        Edit::Add.apply(&mut request);
        Edit::Text(0, Field::Expression, "first".into()).apply(&mut request);
        Edit::Add.apply(&mut request);
        Edit::Text(1, Field::Expression, "second".into()).apply(&mut request);
        let original = request.clone();
        for edit in [
            Edit::Remove(2),
            Edit::Remove(usize::MAX),
            Edit::Move(2, -1),
            Edit::Move(1, 1),
            Edit::Move(0, -1),
            Edit::Move(usize::MAX, 1),
        ] {
            edit.apply(&mut request);
            assert_eq!(request, original);
        }
        Edit::Move(0, 1).apply(&mut request);
        assert_eq!(
            request.runtime.assertions.as_ref().unwrap()[0].expression,
            "second"
        );
        Edit::Move(1, -1).apply(&mut request);
        assert_eq!(request, original);
        Edit::Remove(1).apply(&mut request);
        let rows = request.runtime.assertions.as_ref().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].expression, "first");
    }

    #[test]
    fn assertion_plain_text_descriptions_can_be_replaced_and_cleared() {
        use super::{AssertionEdit as Edit, AssertionField as Field};
        let mut request = Request::default();
        Edit::Add.apply(&mut request);
        request.runtime.assertions.as_mut().unwrap()[0].description =
            Some(probe_core::Documentation::Text("original".into()));
        for text in ["edited {{ordinaryText}}", ""] {
            Edit::Text(0, Field::Description, text.into()).apply(&mut request);
            assert_eq!(
                request.runtime.assertions.as_ref().unwrap()[0].description,
                Some(probe_core::Documentation::Text(text.into()))
            );
        }
    }

    #[test]
    fn blank_assertion_descriptions_preserve_absence_until_text_is_entered() {
        use super::{AssertionEdit as Edit, AssertionField as Field};
        use probe_core::Documentation;

        for original in [None, Some(Documentation::Null)] {
            let mut request = Request::default();
            Edit::Add.apply(&mut request);
            request.runtime.assertions.as_mut().unwrap()[0].description = original.clone();
            Edit::Text(0, Field::Description, String::new()).apply(&mut request);
            assert_eq!(
                request.runtime.assertions.as_ref().unwrap()[0].description,
                original
            );
            Edit::Text(0, Field::Description, "New description".into()).apply(&mut request);
            assert_eq!(
                request.runtime.assertions.as_ref().unwrap()[0].description,
                Some(Documentation::Text("New description".into()))
            );
        }
    }

    #[test]
    fn assertions_section_is_named_and_selectable_for_http_and_graphql() {
        let key = request_key();
        let mut editor = RequestEditorState::default();
        for protocol in [
            probe_core::RequestProtocol::Http,
            probe_core::RequestProtocol::Graphql,
        ] {
            editor.set_section(key, EditorSection::Assertions);
            editor.ensure_available_section(key, protocol);
            assert_eq!(editor.section(key), EditorSection::Assertions);
            assert_eq!(editor.section(key).label(), "Assertions");
        }
    }

    fn request_key() -> probe_core::RequestKey {
        let workspace = probe_core::Workspace::from_collection(probe_core::Collection {
            items: vec![probe_core::CollectionItem::Request(Request::default())],
            ..probe_core::Collection::default()
        });
        let [probe_core::WorkspaceItemRef::Request(key)] = workspace.root_items() else {
            panic!("fixture should contain one request");
        };
        *key
    }

    fn replacement_request_key() -> probe_core::RequestKey {
        let mut workspace = probe_core::Workspace::from_collection(probe_core::Collection {
            items: vec![probe_core::CollectionItem::Request(Request::default())],
            ..probe_core::Collection::default()
        });
        let [probe_core::WorkspaceItemRef::Request(old_key)] = workspace.root_items() else {
            panic!("fixture should contain one request");
        };
        let old_key = *old_key;
        workspace.remove_request(old_key).unwrap();
        workspace.add_root_request(Request::default())
    }
}
