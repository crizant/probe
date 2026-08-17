use std::collections::BTreeMap;

use probe_core::{
    Authentication, AuthenticationKind, AuthenticationValue, Body, HttpRequest, RawBody,
    RawBodyKind, RequestBody, RequestKey,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum EditorSection {
    #[default]
    Query,
    Headers,
    Body,
    Authentication,
}

impl EditorSection {
    pub(crate) const ALL: [Self; 4] =
        [Self::Query, Self::Headers, Self::Body, Self::Authentication];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Query => "Query",
            Self::Headers => "Headers",
            Self::Body => "Body",
            Self::Authentication => "Authentication",
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct RequestEditorState {
    pub(crate) section: EditorSection,
    selected_environment: Option<String>,
    body_drafts: BTreeMap<(RequestKey, BodyEditorKind), RequestBody>,
}

impl RequestEditorState {
    pub(crate) fn selected_environment(&self) -> Option<&str> {
        self.selected_environment.as_deref()
    }

    pub(crate) fn select_environment(&mut self, environment: Option<String>) {
        self.selected_environment = environment;
    }

    pub(crate) fn clear(&mut self) {
        self.section = EditorSection::default();
        self.selected_environment = None;
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

    use probe_core::{AuthenticationKind, Body, HttpRequest, RawBodyKind, RequestBody};

    use super::{
        BodyEditorKind, RequestEditorState, raw_body_mut, set_auth_property, set_authentication,
    };

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
    fn environment_selection_is_shared_for_the_workspace() {
        let mut editor = RequestEditorState::default();
        editor.select_environment(Some("development".to_owned()));
        assert_eq!(editor.selected_environment(), Some("development"));
        editor.select_environment(None);
        assert_eq!(editor.selected_environment(), None);
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
