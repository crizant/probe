//! Native request, body, authentication, and edit models.

use std::{collections::BTreeMap, time::Duration};

use serde_json::{Map, Value};

use crate::{Documentation, ItemMetadata};

/// A native API request definition.
///
/// Common fields are stored once; protocol-specific state belongs to [`RequestKind`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Request {
    /// Request metadata.
    pub metadata: ItemMetadata,
    /// Request documentation. OpenCollection request `docs` is a plain string.
    pub docs: Option<String>,
    /// HTTP method as written in the collection.
    pub method: Option<String>,
    /// Request URL, which may contain variables.
    pub url: Option<String>,
    /// HTTP request headers.
    pub headers: Vec<Header>,
    /// Query parameters.
    pub query_parameters: Vec<QueryParameter>,
    /// Path parameters.
    pub path_parameters: Vec<QueryParameter>,
    /// Protocol identity and protocol-specific body.
    pub kind: RequestKind,
    /// Request authentication configuration.
    pub authentication: Option<Authentication>,
    /// Execution settings.
    pub settings: RequestSettings,
}

impl Request {
    /// Reconciles local and incoming edits against a common baseline.
    ///
    /// Equal edits and changes on only one side merge. Differing edits to
    /// the same field conflict, leaving that field at its baseline value.
    /// Metadata fields merge separately; lists, settings, and protocol/body
    /// state each remain one conflict unit.
    #[must_use]
    pub fn reconcile(baseline: &Self, local: &Self, incoming: &Self) -> (Self, Vec<&'static str>) {
        let mut conflicts = Vec::new();
        // Exhaustive construction requires new request fields to have a merge rule.
        let merged = Self {
            metadata: reconcile_metadata(
                &baseline.metadata,
                &local.metadata,
                &incoming.metadata,
                &mut conflicts,
            ),
            docs: reconcile_field(
                &baseline.docs,
                &local.docs,
                &incoming.docs,
                "docs",
                &mut conflicts,
            ),
            method: reconcile_field(
                &baseline.method,
                &local.method,
                &incoming.method,
                "method",
                &mut conflicts,
            ),
            url: reconcile_field(
                &baseline.url,
                &local.url,
                &incoming.url,
                "URL",
                &mut conflicts,
            ),
            headers: reconcile_field(
                &baseline.headers,
                &local.headers,
                &incoming.headers,
                "headers",
                &mut conflicts,
            ),
            query_parameters: reconcile_field(
                &baseline.query_parameters,
                &local.query_parameters,
                &incoming.query_parameters,
                "query parameters",
                &mut conflicts,
            ),
            path_parameters: reconcile_field(
                &baseline.path_parameters,
                &local.path_parameters,
                &incoming.path_parameters,
                "path parameters",
                &mut conflicts,
            ),
            kind: reconcile_field(
                &baseline.kind,
                &local.kind,
                &incoming.kind,
                "body",
                &mut conflicts,
            ),
            authentication: reconcile_field(
                &baseline.authentication,
                &local.authentication,
                &incoming.authentication,
                "authentication",
                &mut conflicts,
            ),
            settings: reconcile_field(
                &baseline.settings,
                &local.settings,
                &incoming.settings,
                "settings",
                &mut conflicts,
            ),
        };
        (merged, conflicts)
    }
}

fn reconcile_metadata(
    baseline: &ItemMetadata,
    local: &ItemMetadata,
    incoming: &ItemMetadata,
    conflicts: &mut Vec<&'static str>,
) -> ItemMetadata {
    // Exhaustive construction requires new metadata fields to have a merge rule.
    ItemMetadata {
        name: reconcile_field(
            &baseline.name,
            &local.name,
            &incoming.name,
            "name",
            conflicts,
        ),
        sequence: reconcile_field(
            &baseline.sequence,
            &local.sequence,
            &incoming.sequence,
            "sequence",
            conflicts,
        ),
        description: reconcile_field(
            &baseline.description,
            &local.description,
            &incoming.description,
            "description",
            conflicts,
        ),
    }
}

fn reconcile_field<T: Clone + PartialEq>(
    baseline: &T,
    local: &T,
    incoming: &T,
    name: &'static str,
    conflicts: &mut Vec<&'static str>,
) -> T {
    if local == baseline {
        incoming.clone()
    } else if incoming == baseline || local == incoming {
        local.clone()
    } else {
        conflicts.push(name);
        baseline.clone()
    }
}

/// A change to an optional request field.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum FieldPatch<T> {
    /// Keep the current value.
    #[default]
    Unchanged,
    /// Replace the current value.
    Set(T),
    /// Remove the current value.
    Clear,
}

impl<T> FieldPatch<T> {
    /// Converts a replacement optional value into a set or clear patch.
    pub fn from_optional(value: Option<T>) -> Self {
        match value {
            Some(value) => Self::Set(value),
            None => Self::Clear,
        }
    }

    /// Returns whether this patch leaves the field unchanged.
    pub const fn is_unchanged(&self) -> bool {
        matches!(self, Self::Unchanged)
    }

    /// Consumes a set patch, returning its value if present.
    pub fn into_set(self) -> Option<T> {
        match self {
            Self::Set(value) => Some(value),
            Self::Unchanged | Self::Clear => None,
        }
    }

    pub fn apply(&self, target: &mut Option<T>)
    where
        T: Clone,
    {
        match self {
            Self::Unchanged => {}
            Self::Set(value) => *target = Some(value.clone()),
            Self::Clear => *target = None,
        }
    }
}

/// A non-interactive partial update to a native request.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RequestUpdate {
    /// Replacement request name. Removing a name is unsupported and rejected by `between`.
    pub name: Option<String>,
    /// Replacement description.
    pub description: FieldPatch<Documentation>,
    /// Replacement request documentation string.
    pub docs: FieldPatch<String>,
    /// Replacement HTTP method.
    pub method: FieldPatch<String>,
    /// Replacement URL.
    pub url: FieldPatch<String>,
    /// Replacement headers.
    pub headers: Option<Vec<Header>>,
    /// Replacement query parameters.
    pub query_parameters: Option<Vec<QueryParameter>>,
    /// Replacement path parameters.
    pub path_parameters: Option<Vec<QueryParameter>>,
    /// Replacement for the entire HTTP body, including any variant list.
    pub body: FieldPatch<RequestBody>,
    /// Replacement for HTTP body content.
    ///
    /// A single body is written directly. An existing variant list keeps every
    /// variant title and selected flag; only the selected variant's body changes.
    pub body_content: FieldPatch<Body>,
    /// Authentication change.
    pub authentication: FieldPatch<Authentication>,
    /// Partial native GraphQL body update.
    pub graphql: Option<GraphqlUpdate>,
    /// Replacement for the selected native WebSocket message.
    ///
    /// A single message is written directly. An existing variant list keeps every
    /// variant title and selected flag; only the selected variant's message changes.
    /// Clearing removes the whole message, including a variant list.
    pub websocket_message: FieldPatch<WebSocketMessage>,
}

impl RequestUpdate {
    /// Builds the supported field changes from a saved request to its current draft.
    /// A missing `base` treats the request as new.
    pub fn between(base: Option<&Request>, current: &Request) -> Result<Self, RequestDiffError> {
        if base.is_some_and(|saved| saved.kind.as_str() != current.kind.as_str()) {
            return Err(RequestDiffError::UnsupportedChange("request protocol"));
        }
        if base.is_some_and(|saved| saved.settings != current.settings)
            || base.is_none() && current.settings != RequestSettings::default()
        {
            return Err(RequestDiffError::UnsupportedChange("request settings"));
        }
        if base.is_some_and(|saved| saved.metadata.sequence != current.metadata.sequence)
            || base.is_none() && current.metadata.sequence.is_some()
        {
            return Err(RequestDiffError::UnsupportedChange("request sequence"));
        }
        if base
            .is_some_and(|saved| saved.metadata.name.is_some() && current.metadata.name.is_none())
        {
            return Err(RequestDiffError::UnsupportedChange("request name removal"));
        }
        let base_operation = base.map(Request::selected_graphql).transpose()?.flatten();
        let current_operation = current.selected_graphql()?;
        if base.is_none() && matches!(current.graphql(), Some(GraphqlBody::Variants(_))) {
            return Err(RequestDiffError::UnsupportedChange("GraphQL body variants"));
        }
        let websocket_message_changed =
            base.and_then(Request::websocket_message) != current.websocket_message();
        let (base_message, current_message) = if websocket_message_changed {
            (
                base.map(Request::selected_websocket_message)
                    .transpose()?
                    .flatten(),
                current.selected_websocket_message()?,
            )
        } else {
            (None, None)
        };
        if base.is_none()
            && matches!(
                current.websocket_message(),
                Some(WebSocketMessageSet::Variants(_))
            )
        {
            return Err(RequestDiffError::UnsupportedChange(
                "WebSocket message variants",
            ));
        }
        let update = Self {
            name: (base.and_then(|request| request.metadata.name.as_ref())
                != current.metadata.name.as_ref())
            .then(|| current.metadata.name.clone())
            .flatten(),
            description: if base.and_then(|request| request.metadata.description.as_ref())
                != current.metadata.description.as_ref()
            {
                FieldPatch::from_optional(current.metadata.description.clone())
            } else {
                FieldPatch::Unchanged
            },
            docs: if base.and_then(|request| request.docs.as_ref()) != current.docs.as_ref() {
                FieldPatch::from_optional(current.docs.clone())
            } else {
                FieldPatch::Unchanged
            },
            method: if base.and_then(|request| request.method.as_ref()) != current.method.as_ref() {
                FieldPatch::from_optional(current.method.clone())
            } else {
                FieldPatch::Unchanged
            },
            url: if base.and_then(|request| request.url.as_ref()) != current.url.as_ref() {
                FieldPatch::from_optional(current.url.clone())
            } else {
                FieldPatch::Unchanged
            },
            headers: (base.map(|request| &request.headers) != Some(&current.headers))
                .then(|| current.headers.clone()),
            query_parameters: (base.map(|request| &request.query_parameters)
                != Some(&current.query_parameters))
            .then(|| current.query_parameters.clone()),
            path_parameters: (base.map(|request| &request.path_parameters)
                != Some(&current.path_parameters))
            .then(|| current.path_parameters.clone()),
            body: if base.and_then(Request::http_body) != current.http_body() {
                FieldPatch::from_optional(current.http_body().cloned())
            } else {
                FieldPatch::Unchanged
            },
            body_content: FieldPatch::Unchanged,
            authentication: if base.and_then(|request| request.authentication.as_ref())
                != current.authentication.as_ref()
            {
                FieldPatch::from_optional(current.authentication.clone())
            } else {
                FieldPatch::Unchanged
            },
            graphql: match &current.kind {
                RequestKind::Graphql { .. } if base_operation != current_operation => {
                    Some(GraphqlUpdate {
                        query: current_operation
                            .map(|operation| FieldPatch::from_optional(operation.query.clone()))
                            .unwrap_or(FieldPatch::Clear),
                        variables: current_operation
                            .map(|operation| FieldPatch::from_optional(operation.variables.clone()))
                            .unwrap_or_default(),
                        operation_name: current_operation
                            .map(|operation| {
                                FieldPatch::from_optional(operation.operation_name.clone())
                            })
                            .unwrap_or_default(),
                        extensions: current_operation
                            .map(|operation| {
                                FieldPatch::from_optional(operation.extensions.clone())
                            })
                            .unwrap_or_default(),
                    })
                }
                _ => None,
            },
            websocket_message: match &current.kind {
                RequestKind::WebSocket { .. }
                    if websocket_message_changed && base_message != current_message =>
                {
                    FieldPatch::from_optional(current_message.cloned())
                }
                _ => FieldPatch::Unchanged,
            },
        };
        if let Some(base) = base {
            let mut reconstructed = base.clone();
            update.apply(&mut reconstructed)?;
            if reconstructed != *current {
                return Err(RequestDiffError::UnsupportedChange(
                    if current.kind.is_websocket() {
                        "WebSocket message variants"
                    } else {
                        "GraphQL body variants"
                    },
                ));
            }
        }
        Ok(update)
    }

    /// Returns whether every field is unchanged.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.description.is_unchanged()
            && self.docs.is_unchanged()
            && self.method.is_unchanged()
            && self.url.is_unchanged()
            && self.headers.is_none()
            && self.query_parameters.is_none()
            && self.path_parameters.is_none()
            && self.body.is_unchanged()
            && self.body_content.is_unchanged()
            && self.authentication.is_unchanged()
            && self.graphql.as_ref().is_none_or(GraphqlUpdate::is_empty)
            && self.websocket_message.is_unchanged()
    }

    /// Applies the update to a domain request, including native GraphQL fields.
    ///
    /// On error the request is left unchanged.
    pub fn apply(&self, request: &mut Request) -> Result<(), RequestProtocolError> {
        let mut updated = request.clone();
        self.apply_to_candidate(&mut updated)?;
        *request = updated;
        Ok(())
    }

    fn apply_to_candidate(&self, request: &mut Request) -> Result<(), RequestProtocolError> {
        let graphql = self.graphql.as_ref().filter(|update| !update.is_empty());
        let http_body_change = !self.body.is_unchanged() || !self.body_content.is_unchanged();
        if graphql.is_some() && !request.kind.is_graphql() {
            return Err(RequestProtocolError::NotGraphql);
        }
        if http_body_change && request.kind.is_graphql() {
            return Err(RequestProtocolError::NotHttp);
        }
        if !self.websocket_message.is_unchanged() && !request.kind.is_websocket() {
            return Err(RequestProtocolError::NotWebSocket);
        }
        if request.kind.is_websocket() {
            self.reject_websocket_unsupported_fields()?;
        }
        match &mut request.kind {
            RequestKind::Http { body } => {
                self.body.apply(body);
                apply_body_content(body, &self.body_content)?;
            }
            RequestKind::Graphql { .. } => {}
            RequestKind::WebSocket { message } => {
                apply_websocket_message(message, &self.websocket_message)?;
            }
        }
        // Must precede common fields: variant selection can still fail here.
        if let Some(graphql) = graphql {
            request.apply_graphql_update(graphql)?;
        }
        if let Some(name) = &self.name {
            request.metadata.name = Some(name.clone());
        }
        self.description.apply(&mut request.metadata.description);
        self.docs.apply(&mut request.docs);
        self.method.apply(&mut request.method);
        self.url.apply(&mut request.url);
        if let Some(headers) = &self.headers {
            request.headers.clone_from(headers);
        }
        if let Some(parameters) = &self.query_parameters {
            request.query_parameters.clone_from(parameters);
        }
        if let Some(parameters) = &self.path_parameters {
            request.path_parameters.clone_from(parameters);
        }
        self.authentication.apply(&mut request.authentication);
        Ok(())
    }

    /// OpenCollection WebSocket details have no method, parameters, or HTTP body.
    fn reject_websocket_unsupported_fields(&self) -> Result<(), RequestProtocolError> {
        let unsupported = |field| RequestProtocolError::UnsupportedField {
            protocol: RequestProtocol::WebSocket,
            field,
        };
        if matches!(self.method, FieldPatch::Set(_)) {
            return Err(unsupported("an HTTP method"));
        }
        if self
            .query_parameters
            .as_ref()
            .is_some_and(|p| !p.is_empty())
        {
            return Err(unsupported("query parameters"));
        }
        if self.path_parameters.as_ref().is_some_and(|p| !p.is_empty()) {
            return Err(unsupported("path parameters"));
        }
        if !self.body.is_unchanged() || !self.body_content.is_unchanged() {
            return Err(unsupported("an HTTP body"));
        }
        Ok(())
    }
}

/// Protocol identity independent of a request's body.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RequestProtocol {
    /// A native OpenCollection HTTP request.
    #[default]
    Http,
    /// A native OpenCollection GraphQL request.
    Graphql,
    /// A native OpenCollection WebSocket request.
    WebSocket,
}

impl RequestProtocol {
    /// Returns the stable lowercase protocol name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Graphql => "graphql",
            Self::WebSocket => "websocket",
        }
    }

    /// Parses a stable lowercase protocol name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "http" => Some(Self::Http),
            "graphql" => Some(Self::Graphql),
            "websocket" => Some(Self::WebSocket),
            _ => None,
        }
    }

    /// Returns the human-readable protocol name.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Http => "HTTP",
            Self::Graphql => "GraphQL",
            Self::WebSocket => "WebSocket",
        }
    }

    /// Returns the default HTTP method, or `None` for protocols without one.
    #[must_use]
    pub const fn default_method(self) -> Option<&'static str> {
        match self {
            Self::Http => Some("GET"),
            Self::Graphql => Some("POST"),
            Self::WebSocket => None,
        }
    }
}

/// The protocol of a native request and the body that belongs to that protocol.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RequestKind {
    /// A native OpenCollection HTTP request.
    Http {
        /// HTTP request body definition.
        body: Option<RequestBody>,
    },
    /// A native OpenCollection GraphQL request.
    Graphql {
        /// Native GraphQL body definition.
        body: Option<GraphqlBody>,
    },
    /// A native OpenCollection WebSocket request.
    WebSocket {
        /// Message definition sent after the connection opens.
        message: Option<WebSocketMessageSet>,
    },
}

impl Default for RequestKind {
    fn default() -> Self {
        Self::Http { body: None }
    }
}

impl RequestKind {
    /// Returns the protocol identity without its body data.
    #[must_use]
    pub const fn protocol(&self) -> RequestProtocol {
        match self {
            Self::Http { .. } => RequestProtocol::Http,
            Self::Graphql { .. } => RequestProtocol::Graphql,
            Self::WebSocket { .. } => RequestProtocol::WebSocket,
        }
    }

    /// Returns an empty request kind for a protocol.
    #[must_use]
    pub const fn empty(protocol: RequestProtocol) -> Self {
        match protocol {
            RequestProtocol::Http => Self::Http { body: None },
            RequestProtocol::Graphql => Self::Graphql { body: None },
            RequestProtocol::WebSocket => Self::WebSocket { message: None },
        }
    }

    /// Returns the stable lowercase protocol name.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        self.protocol().as_str()
    }

    /// Returns whether this is a native GraphQL request.
    #[must_use]
    pub const fn is_graphql(&self) -> bool {
        matches!(self, Self::Graphql { .. })
    }

    /// Returns whether this is a native WebSocket request.
    #[must_use]
    pub const fn is_websocket(&self) -> bool {
        matches!(self, Self::WebSocket { .. })
    }
}

/// Request execution settings.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RequestSettings {
    /// HTTP total timeout or WebSocket connection timeout; zero means no timeout.
    pub timeout: Option<Duration>,
    /// Whether redirects are followed.
    pub follow_redirects: Option<bool>,
    /// Maximum redirect hops.
    pub max_redirects: Option<usize>,
    /// WebSocket keep-alive interval.
    pub keep_alive_interval: Option<Duration>,
}

/// An HTTP request header.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Header {
    /// Header name.
    pub name: String,
    /// Header value.
    pub value: String,
    /// Whether the header is disabled.
    pub disabled: bool,
}

/// An HTTP query or path parameter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueryParameter {
    /// Parameter name.
    pub name: String,
    /// Parameter value.
    pub value: String,
    /// Whether the parameter is disabled.
    pub disabled: bool,
}

/// A request body represented directly or as selectable variants.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RequestBody {
    /// One body definition.
    Single(Body),
    /// Multiple named body definitions.
    Variants(Vec<BodyVariant>),
}

/// A named request-body variant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BodyVariant {
    /// Variant title.
    pub title: String,
    /// Whether the variant is selected.
    pub selected: bool,
    /// Variant body.
    pub body: Body,
}

/// A supported OpenCollection HTTP body.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Body {
    /// JSON, text, XML, or SPARQL data.
    Raw(RawBody),
    /// URL-encoded form fields.
    FormUrlEncoded(Vec<FormField>),
    /// Multipart form parts.
    Multipart(Vec<MultipartPart>),
    /// One or more file-body variants.
    File(Vec<FileReference>),
}

/// A native GraphQL body represented directly or as selectable variants.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GraphqlBody {
    /// One GraphQL operation definition.
    Single(GraphqlOperation),
    /// Multiple named operation definitions.
    Variants(Vec<GraphqlBodyVariant>),
}

/// A GraphQL operation and GraphQL-over-HTTP request parameters.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GraphqlOperation {
    /// GraphQL operation document.
    pub query: Option<String>,
    /// Optional GraphQL variables object.
    pub variables: Option<Map<String, Value>>,
    /// Optional operation name for documents with multiple operations.
    pub operation_name: Option<String>,
    /// Optional GraphQL-over-HTTP extensions object.
    pub extensions: Option<Map<String, Value>>,
}

/// A selectable native GraphQL body variant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphqlBodyVariant {
    /// Variant title.
    pub title: String,
    /// Whether this variant is selected.
    pub selected: bool,
    /// Variant operation.
    pub body: GraphqlOperation,
}

/// A partial update to the selected native GraphQL operation.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GraphqlUpdate {
    /// Replacement query.
    pub query: FieldPatch<String>,
    /// Variables change.
    pub variables: FieldPatch<Map<String, Value>>,
    /// Operation name change.
    pub operation_name: FieldPatch<String>,
    /// Extensions change.
    pub extensions: FieldPatch<Map<String, Value>>,
}

impl GraphqlUpdate {
    /// Returns whether every GraphQL field is unchanged.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.query.is_unchanged()
            && self.variables.is_unchanged()
            && self.operation_name.is_unchanged()
            && self.extensions.is_unchanged()
    }

    /// Applies this update to one operation.
    pub fn apply(&self, operation: &mut GraphqlOperation) {
        self.query.apply(&mut operation.query);
        self.variables.apply(&mut operation.variables);
        self.operation_name.apply(&mut operation.operation_name);
        self.extensions.apply(&mut operation.extensions);
    }
}

/// A native WebSocket message represented directly or as selectable variants.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WebSocketMessageSet {
    /// One message definition.
    Single(WebSocketMessage),
    /// Multiple named message definitions.
    Variants(Vec<WebSocketMessageVariant>),
}

/// A WebSocket message definition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebSocketMessage {
    /// Message content type.
    pub kind: WebSocketMessageKind,
    /// Message data, which may contain variables.
    pub data: String,
}

/// WebSocket message types defined by OpenCollection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WebSocketMessageKind {
    /// Plain text.
    Text,
    /// JSON text.
    Json,
    /// XML text.
    Xml,
    /// Binary data as written in the collection.
    Binary,
}

impl WebSocketMessageKind {
    /// Returns the OpenCollection message type name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Json => "json",
            Self::Xml => "xml",
            Self::Binary => "binary",
        }
    }
}

/// A selectable native WebSocket message variant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebSocketMessageVariant {
    /// Variant title.
    pub title: String,
    /// Whether this variant is selected.
    pub selected: bool,
    /// Variant message.
    pub message: WebSocketMessage,
}

/// A request or update that is invalid for the request protocol.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RequestProtocolError {
    /// A body variant selection is missing or ambiguous.
    InvalidBodySelection(String),
    /// GraphQL-only fields were requested for an HTTP request.
    NotGraphql,
    /// HTTP body fields were requested for a native GraphQL request.
    NotHttp,
    /// WebSocket-only fields were requested for a non-WebSocket request.
    NotWebSocket,
    /// A field was requested that the request protocol does not define.
    UnsupportedField {
        /// Request protocol.
        protocol: RequestProtocol,
        /// Field description.
        field: &'static str,
    },
    /// The request protocol cannot be prepared for HTTP execution.
    UnsupportedExecution(RequestProtocol),
}

impl std::fmt::Display for RequestProtocolError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidBodySelection(message) => formatter.write_str(message),
            Self::NotGraphql => formatter.write_str("request is not a native GraphQL request"),
            Self::NotHttp => formatter
                .write_str("HTTP body updates cannot be applied to a native GraphQL request"),
            Self::NotWebSocket => formatter.write_str("request is not a native WebSocket request"),
            Self::UnsupportedField { protocol, field } => write!(
                formatter,
                "native {} requests do not support {field}",
                protocol.label()
            ),
            Self::UnsupportedExecution(protocol) => write!(
                formatter,
                "native {} requests cannot be executed yet",
                protocol.label()
            ),
        }
    }
}

impl std::error::Error for RequestProtocolError {}

/// A request edit that cannot be represented by a persistence update.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RequestDiffError {
    /// The request body, selection, or protocol fields are invalid.
    Protocol(RequestProtocolError),
    /// A changed field has no supported persistence operation.
    UnsupportedChange(&'static str),
}

impl From<RequestProtocolError> for RequestDiffError {
    fn from(error: RequestProtocolError) -> Self {
        Self::Protocol(error)
    }
}

impl std::fmt::Display for RequestDiffError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Protocol(error) => error.fmt(formatter),
            Self::UnsupportedChange(field) => write!(formatter, "cannot save changed {field}"),
        }
    }
}

impl std::error::Error for RequestDiffError {}

fn selected_http_variant_index(variants: &[BodyVariant]) -> Result<usize, RequestProtocolError> {
    let mut selected = variants
        .iter()
        .enumerate()
        .filter(|(_, variant)| variant.selected);
    let (index, _) = selected.next().ok_or_else(|| {
        RequestProtocolError::InvalidBodySelection(
            "request body variants have no selected value".to_owned(),
        )
    })?;
    if selected.next().is_some() {
        return Err(RequestProtocolError::InvalidBodySelection(
            "request body variants have multiple selected values".to_owned(),
        ));
    }
    Ok(index)
}

fn apply_body_content(
    target: &mut Option<RequestBody>,
    patch: &FieldPatch<Body>,
) -> Result<(), RequestProtocolError> {
    match patch {
        FieldPatch::Unchanged => Ok(()),
        FieldPatch::Clear => {
            *target = None;
            Ok(())
        }
        FieldPatch::Set(content) => match target {
            Some(RequestBody::Variants(variants)) => {
                let index = selected_http_variant_index(variants)?;
                variants[index].body = content.clone();
                Ok(())
            }
            other => {
                *other = Some(RequestBody::Single(content.clone()));
                Ok(())
            }
        },
    }
}

fn selected_websocket_variant_index(
    variants: &[WebSocketMessageVariant],
) -> Result<usize, RequestProtocolError> {
    let mut selected = variants
        .iter()
        .enumerate()
        .filter(|(_, variant)| variant.selected);
    let (index, _) = selected.next().ok_or_else(|| {
        RequestProtocolError::InvalidBodySelection(
            "WebSocket message variants have no selected value".to_owned(),
        )
    })?;
    if selected.next().is_some() {
        return Err(RequestProtocolError::InvalidBodySelection(
            "WebSocket message variants have multiple selected values".to_owned(),
        ));
    }
    Ok(index)
}

fn apply_websocket_message(
    target: &mut Option<WebSocketMessageSet>,
    patch: &FieldPatch<WebSocketMessage>,
) -> Result<(), RequestProtocolError> {
    match patch {
        FieldPatch::Unchanged => {}
        FieldPatch::Clear => *target = None,
        FieldPatch::Set(message) => match target {
            Some(WebSocketMessageSet::Variants(variants)) => {
                let index = selected_websocket_variant_index(variants)?;
                variants[index].message = message.clone();
            }
            other => *other = Some(WebSocketMessageSet::Single(message.clone())),
        },
    }
    Ok(())
}

impl Request {
    /// Returns the HTTP body when this is an HTTP request with a body.
    #[must_use]
    pub const fn http_body(&self) -> Option<&RequestBody> {
        match &self.kind {
            RequestKind::Http { body } => body.as_ref(),
            RequestKind::Graphql { .. } | RequestKind::WebSocket { .. } => None,
        }
    }

    /// Returns the mutable HTTP body when this is an HTTP request with a body.
    pub const fn http_body_mut(&mut self) -> Option<&mut RequestBody> {
        match &mut self.kind {
            RequestKind::Http { body } => body.as_mut(),
            RequestKind::Graphql { .. } | RequestKind::WebSocket { .. } => None,
        }
    }

    /// Returns the native GraphQL body when this is a GraphQL request with a body.
    #[must_use]
    pub const fn graphql(&self) -> Option<&GraphqlBody> {
        match &self.kind {
            RequestKind::Graphql { body } => body.as_ref(),
            RequestKind::Http { .. } | RequestKind::WebSocket { .. } => None,
        }
    }

    /// Returns the native WebSocket message when this is a WebSocket request with one.
    #[must_use]
    pub const fn websocket_message(&self) -> Option<&WebSocketMessageSet> {
        match &self.kind {
            RequestKind::WebSocket { message } => message.as_ref(),
            RequestKind::Http { .. } | RequestKind::Graphql { .. } => None,
        }
    }

    /// Returns the selected native WebSocket message.
    pub fn selected_websocket_message(
        &self,
    ) -> Result<Option<&WebSocketMessage>, RequestProtocolError> {
        match self.websocket_message() {
            None => Ok(None),
            Some(WebSocketMessageSet::Single(message)) => Ok(Some(message)),
            Some(WebSocketMessageSet::Variants(variants)) => {
                selected_websocket_variant_index(variants)
                    .map(|index| Some(&variants[index].message))
            }
        }
    }

    /// Finds the single selected operation in a GraphQL variant list.
    fn selected_graphql_variant_index(
        variants: &[GraphqlBodyVariant],
    ) -> Result<usize, RequestProtocolError> {
        let mut selected = variants
            .iter()
            .enumerate()
            .filter(|(_, variant)| variant.selected);
        let (index, _) = selected.next().ok_or_else(|| {
            RequestProtocolError::InvalidBodySelection(
                "GraphQL body variants have no selected value".to_owned(),
            )
        })?;
        if selected.next().is_some() {
            return Err(RequestProtocolError::InvalidBodySelection(
                "GraphQL body variants have multiple selected values".to_owned(),
            ));
        }
        Ok(index)
    }

    /// Returns the selected native GraphQL operation.
    pub fn selected_graphql(&self) -> Result<Option<&GraphqlOperation>, RequestProtocolError> {
        match self.graphql() {
            None => Ok(None),
            Some(GraphqlBody::Single(operation)) => Ok(Some(operation)),
            Some(GraphqlBody::Variants(variants)) => Self::selected_graphql_variant_index(variants)
                .map(|index| Some(&variants[index].body)),
        }
    }

    /// Applies a GraphQL-only partial update to the selected operation.
    pub fn apply_graphql_update(
        &mut self,
        update: &GraphqlUpdate,
    ) -> Result<(), RequestProtocolError> {
        let RequestKind::Graphql { body } = &mut self.kind else {
            return Err(RequestProtocolError::NotGraphql);
        };
        let operation =
            match body.get_or_insert_with(|| GraphqlBody::Single(GraphqlOperation::default())) {
                GraphqlBody::Single(operation) => operation,
                GraphqlBody::Variants(variants) => {
                    let index = Self::selected_graphql_variant_index(variants)?;
                    &mut variants[index].body
                }
            };
        update.apply(operation);
        Ok(())
    }

    /// Converts an owned request into the HTTP request consumed by Probe's engine.
    ///
    /// Native GraphQL requests become GraphQL-over-HTTP: GET requests carry the selected
    /// operation in query parameters, other methods carry a JSON envelope body.
    pub fn into_http(mut self) -> Result<PreparedHttpRequest, RequestProtocolError> {
        let operation = match std::mem::take(&mut self.kind) {
            http @ RequestKind::Http { .. } => {
                self.kind = http;
                return Ok(PreparedHttpRequest(self));
            }
            RequestKind::WebSocket { .. } => {
                return Err(RequestProtocolError::UnsupportedExecution(
                    RequestProtocol::WebSocket,
                ));
            }
            RequestKind::Graphql { body: None } => GraphqlOperation::default(),
            RequestKind::Graphql {
                body: Some(GraphqlBody::Single(operation)),
            } => operation,
            RequestKind::Graphql {
                body: Some(GraphqlBody::Variants(mut variants)),
            } => {
                let index = Self::selected_graphql_variant_index(&variants)?;
                variants.swap_remove(index).body
            }
        };
        if self
            .method
            .as_deref()
            .is_some_and(|method| method.eq_ignore_ascii_case("GET"))
        {
            self.query_parameters
                .retain(|parameter| !is_graphql_http_parameter(&parameter.name));
            if let Some(query) = operation.query {
                self.query_parameters.push(QueryParameter {
                    name: "query".to_owned(),
                    value: query,
                    disabled: false,
                });
            }
            append_graphql_parameter(&mut self, "variables", operation.variables);
            if let Some(operation_name) = operation.operation_name {
                self.query_parameters.push(QueryParameter {
                    name: "operationName".to_owned(),
                    value: operation_name,
                    disabled: false,
                });
            }
            append_graphql_parameter(&mut self, "extensions", operation.extensions);
        } else {
            let envelope = operation.into_json_envelope();
            self.kind = RequestKind::Http {
                body: Some(RequestBody::Single(Body::Raw(RawBody {
                    kind: RawBodyKind::Json,
                    data: Value::Object(envelope).to_string(),
                }))),
            };
        }
        Ok(PreparedHttpRequest(self))
    }
}

/// A request ready for HTTP transport.
///
/// Only [`Request::into_http`] constructs this value, so its request is always
/// [`RequestKind::Http`].
#[derive(Clone, Debug, PartialEq)]
pub struct PreparedHttpRequest(Request);

impl PreparedHttpRequest {
    /// Returns the prepared request fields.
    #[must_use]
    pub const fn request(&self) -> &Request {
        &self.0
    }

    /// Returns the HTTP body to send.
    #[must_use]
    pub const fn body(&self) -> Option<&RequestBody> {
        self.0.http_body()
    }
}

impl GraphqlOperation {
    fn into_json_envelope(self) -> Map<String, Value> {
        let mut fields = Map::new();
        if let Some(query) = self.query {
            fields.insert("query".to_owned(), Value::String(query));
        }
        if let Some(variables) = self.variables {
            fields.insert("variables".to_owned(), Value::Object(variables));
        }
        if let Some(operation_name) = self.operation_name {
            fields.insert("operationName".to_owned(), Value::String(operation_name));
        }
        if let Some(extensions) = self.extensions {
            fields.insert("extensions".to_owned(), Value::Object(extensions));
        }
        fields
    }
}

fn append_graphql_parameter(request: &mut Request, name: &str, value: Option<Map<String, Value>>) {
    if let Some(value) = value {
        request.query_parameters.push(QueryParameter {
            name: name.to_owned(),
            value: Value::Object(value).to_string(),
            disabled: false,
        });
    }
}

fn is_graphql_http_parameter(name: &str) -> bool {
    matches!(name, "query" | "variables" | "operationName" | "extensions")
}

/// A raw request body.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawBody {
    /// Raw body content type.
    pub kind: RawBodyKind,
    /// Body data.
    pub data: String,
}

/// Raw body types defined by OpenCollection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RawBodyKind {
    /// JSON text.
    Json,
    /// Plain text.
    Text,
    /// XML text.
    Xml,
    /// SPARQL text.
    Sparql,
}

/// A URL-encoded form field.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FormField {
    /// Field name.
    pub name: String,
    /// Field value.
    pub value: String,
    /// Whether the field is disabled.
    pub disabled: bool,
}

/// A multipart form part.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MultipartPart {
    /// Part name.
    pub name: String,
    /// Part kind.
    pub kind: MultipartPartKind,
    /// Text value or file paths.
    pub value: MultipartValue,
    /// Optional content type.
    pub content_type: Option<String>,
    /// Whether the part is disabled.
    pub disabled: bool,
}

/// Multipart part types.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MultipartPartKind {
    /// A text part.
    Text,
    /// A file part.
    File,
}

/// A multipart part value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MultipartValue {
    /// A single text value or path.
    Single(String),
    /// Multiple file paths.
    Multiple(Vec<String>),
}

/// A file-body choice.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileReference {
    /// File path.
    pub file_path: String,
    /// File media type.
    pub content_type: String,
    /// Whether the file is selected.
    pub selected: bool,
}

/// Authentication configuration for a request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Authentication {
    /// Authentication scheme.
    pub kind: AuthenticationKind,
    /// Scheme-specific properties.
    pub properties: BTreeMap<String, AuthenticationValue>,
}

/// Authentication schemes defined by OpenCollection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AuthenticationKind {
    /// Inherit authentication.
    Inherit,
    /// AWS Signature Version 4.
    AwsV4,
    /// HTTP Basic authentication.
    Basic,
    /// WS-Security UsernameToken.
    Wsse,
    /// Bearer authentication.
    Bearer,
    /// HTTP Digest authentication.
    Digest,
    /// NTLM authentication.
    Ntlm,
    /// API-key authentication.
    ApiKey,
    /// OAuth 1.0.
    OAuth1,
    /// OAuth 2.0.
    OAuth2,
    /// A future or extension scheme.
    Other(String),
}

impl AuthenticationKind {
    /// Returns the OpenCollection scheme name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Inherit => "inherit",
            Self::AwsV4 => "awsv4",
            Self::Basic => "basic",
            Self::Wsse => "wsse",
            Self::Bearer => "bearer",
            Self::Digest => "digest",
            Self::Ntlm => "ntlm",
            Self::ApiKey => "apikey",
            Self::OAuth1 => "oauth1",
            Self::OAuth2 => "oauth2",
            Self::Other(kind) => kind,
        }
    }
}

/// A serialization-independent authentication property value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AuthenticationValue {
    /// A string value.
    String(String),
    /// A boolean value.
    Boolean(bool),
    /// A number retained as a string.
    Number(String),
    /// A null value.
    Null,
    /// A sequence of values.
    Sequence(Vec<AuthenticationValue>),
    /// A string-keyed object.
    Object(BTreeMap<String, AuthenticationValue>),
}
