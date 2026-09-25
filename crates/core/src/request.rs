//! HTTP request, body, authentication, and edit models.

use std::{collections::BTreeMap, time::Duration};

use serde_json::{Map, Value};

use crate::ItemMetadata;

/// An HTTP request definition.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HttpRequest {
    /// Request metadata.
    pub metadata: ItemMetadata,
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
    /// Request body definition.
    pub body: Option<RequestBody>,
    /// Request authentication configuration.
    pub authentication: Option<Authentication>,
    /// Execution settings.
    pub settings: RequestSettings,
    /// Canonical protocol identity for this request.
    pub protocol: RequestProtocol,
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

    fn apply_to(&self, target: &mut Option<T>)
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

/// A non-interactive partial update to an HTTP request.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RequestUpdate {
    /// Replacement request name. Removing a name is unsupported and rejected by `between`.
    pub name: Option<String>,
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
    /// Request body change.
    pub body: FieldPatch<RequestBody>,
    /// Authentication change.
    pub authentication: FieldPatch<Authentication>,
    /// Partial native GraphQL body update.
    pub graphql: Option<GraphqlUpdate>,
}

impl RequestUpdate {
    /// Builds the supported field changes from a saved request to its current draft.
    /// A missing `base` treats the request as new.
    pub fn between(
        base: Option<&HttpRequest>,
        current: &HttpRequest,
    ) -> Result<Self, RequestDiffError> {
        if base.is_some_and(|saved| saved.protocol.as_str() != current.protocol.as_str()) {
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
        let base_operation = base
            .map(HttpRequest::selected_graphql)
            .transpose()?
            .flatten();
        let current_operation = current.selected_graphql()?;
        if base.is_none()
            && matches!(
                current.protocol,
                RequestProtocol::Graphql(Some(GraphqlBody::Variants(_)))
            )
        {
            return Err(RequestDiffError::UnsupportedChange("GraphQL body variants"));
        }
        let update = Self {
            name: (base.and_then(|request| request.metadata.name.as_ref())
                != current.metadata.name.as_ref())
            .then(|| current.metadata.name.clone())
            .flatten(),
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
            body: if base.and_then(|request| request.body.as_ref()) != current.body.as_ref() {
                FieldPatch::from_optional(current.body.clone())
            } else {
                FieldPatch::Unchanged
            },
            authentication: if base.and_then(|request| request.authentication.as_ref())
                != current.authentication.as_ref()
            {
                FieldPatch::from_optional(current.authentication.clone())
            } else {
                FieldPatch::Unchanged
            },
            graphql: match (base.map(|request| &request.protocol), &current.protocol) {
                (None | Some(RequestProtocol::Graphql(_)), RequestProtocol::Graphql(_))
                    if base_operation != current_operation =>
                {
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
        };
        if let Some(base) = base {
            let mut reconstructed = base.clone();
            update.apply(&mut reconstructed)?;
            if reconstructed != *current {
                return Err(RequestDiffError::UnsupportedChange("GraphQL body variants"));
            }
        }
        Ok(update)
    }

    /// Returns whether every field is unchanged.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.method.is_unchanged()
            && self.url.is_unchanged()
            && self.headers.is_none()
            && self.query_parameters.is_none()
            && self.path_parameters.is_none()
            && self.body.is_unchanged()
            && self.authentication.is_unchanged()
            && self.graphql.as_ref().is_none_or(GraphqlUpdate::is_empty)
    }

    /// Applies the update to a domain request, including native GraphQL fields.
    pub fn apply(&self, request: &mut HttpRequest) -> Result<(), GraphqlRequestError> {
        if let Some(name) = &self.name {
            request.metadata.name = Some(name.clone());
        }
        self.method.apply_to(&mut request.method);
        self.url.apply_to(&mut request.url);
        if let Some(headers) = &self.headers {
            request.headers.clone_from(headers);
        }
        if let Some(parameters) = &self.query_parameters {
            request.query_parameters.clone_from(parameters);
        }
        if let Some(parameters) = &self.path_parameters {
            request.path_parameters.clone_from(parameters);
        }
        self.body.apply_to(&mut request.body);
        self.authentication.apply_to(&mut request.authentication);
        if let Some(graphql) = self.graphql.as_ref().filter(|update| !update.is_empty()) {
            request.apply_graphql_update(graphql)?;
        }
        Ok(())
    }
}

/// The canonical protocol represented by an in-memory request.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum RequestProtocol {
    /// A native OpenCollection HTTP request.
    #[default]
    Http,
    /// A native OpenCollection GraphQL request and its protocol body.
    Graphql(Option<GraphqlBody>),
}

impl RequestProtocol {
    /// Returns the stable lowercase protocol name.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Graphql(_) => "graphql",
        }
    }
}

/// HTTP execution settings.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RequestSettings {
    /// Total timeout; zero means no timeout.
    pub timeout: Option<Duration>,
    /// Whether redirects are followed.
    pub follow_redirects: Option<bool>,
    /// Maximum redirect hops.
    pub max_redirects: Option<usize>,
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

/// A native OpenCollection GraphQL request definition.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GraphqlRequest {
    /// Request metadata.
    pub metadata: ItemMetadata,
    /// GraphQL-over-HTTP method as written in the collection.
    pub method: Option<String>,
    /// GraphQL endpoint URL, which may contain variables.
    pub url: Option<String>,
    /// Request headers.
    pub headers: Vec<Header>,
    /// Query parameters.
    pub query_parameters: Vec<QueryParameter>,
    /// Path parameters.
    pub path_parameters: Vec<QueryParameter>,
    /// Native GraphQL body definition.
    pub body: Option<GraphqlBody>,
    /// Request authentication configuration.
    pub authentication: Option<Authentication>,
    /// Execution settings.
    pub settings: RequestSettings,
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
        self.query.apply_to(&mut operation.query);
        self.variables.apply_to(&mut operation.variables);
        self.operation_name.apply_to(&mut operation.operation_name);
        self.extensions.apply_to(&mut operation.extensions);
    }
}

/// An invalid native GraphQL request or update.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GraphqlRequestError {
    /// A body variant selection is missing or ambiguous.
    InvalidBodySelection(String),
    /// GraphQL-only fields were requested for an HTTP request.
    NotGraphql,
}

impl std::fmt::Display for GraphqlRequestError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidBodySelection(message) => formatter.write_str(message),
            Self::NotGraphql => formatter.write_str("request is not a native GraphQL request"),
        }
    }
}

impl std::error::Error for GraphqlRequestError {}

/// A request edit that cannot be represented by a persistence update.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RequestDiffError {
    /// The GraphQL body or selection is invalid.
    Graphql(GraphqlRequestError),
    /// A changed field has no supported persistence operation.
    UnsupportedChange(&'static str),
}

impl From<GraphqlRequestError> for RequestDiffError {
    fn from(error: GraphqlRequestError) -> Self {
        Self::Graphql(error)
    }
}

impl std::fmt::Display for RequestDiffError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Graphql(error) => error.fmt(formatter),
            Self::UnsupportedChange(field) => write!(formatter, "cannot save changed {field}"),
        }
    }
}

impl std::error::Error for RequestDiffError {}

impl GraphqlRequest {
    /// Converts this native request into Probe's common in-memory request representation.
    #[must_use]
    pub fn into_request(self) -> HttpRequest {
        HttpRequest {
            metadata: self.metadata,
            method: self.method,
            url: self.url,
            headers: self.headers,
            query_parameters: self.query_parameters,
            path_parameters: self.path_parameters,
            body: None,
            authentication: self.authentication,
            settings: self.settings,
            protocol: RequestProtocol::Graphql(self.body),
        }
    }
}

impl HttpRequest {
    /// Returns the native GraphQL body when this is a GraphQL request.
    #[must_use]
    pub const fn graphql(&self) -> Option<&GraphqlBody> {
        match &self.protocol {
            RequestProtocol::Http | RequestProtocol::Graphql(None) => None,
            RequestProtocol::Graphql(Some(body)) => Some(body),
        }
    }

    /// Finds the single selected operation in a GraphQL variant list.
    fn selected_graphql_variant_index(
        variants: &[GraphqlBodyVariant],
    ) -> Result<usize, GraphqlRequestError> {
        let mut selected = variants
            .iter()
            .enumerate()
            .filter(|(_, variant)| variant.selected);
        let (index, _) = selected.next().ok_or_else(|| {
            GraphqlRequestError::InvalidBodySelection(
                "GraphQL body variants have no selected value".to_owned(),
            )
        })?;
        if selected.next().is_some() {
            return Err(GraphqlRequestError::InvalidBodySelection(
                "GraphQL body variants have multiple selected values".to_owned(),
            ));
        }
        Ok(index)
    }

    /// Returns the selected native GraphQL operation.
    pub fn selected_graphql(&self) -> Result<Option<&GraphqlOperation>, GraphqlRequestError> {
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
    ) -> Result<(), GraphqlRequestError> {
        let RequestProtocol::Graphql(body) = &mut self.protocol else {
            return Err(GraphqlRequestError::NotGraphql);
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
    pub fn into_http(mut self) -> Result<Self, GraphqlRequestError> {
        let operation = match std::mem::replace(&mut self.protocol, RequestProtocol::Http) {
            RequestProtocol::Http => return Ok(self),
            RequestProtocol::Graphql(None) => GraphqlOperation::default(),
            RequestProtocol::Graphql(Some(GraphqlBody::Single(operation))) => operation,
            RequestProtocol::Graphql(Some(GraphqlBody::Variants(mut variants))) => {
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
            self.body = Some(RequestBody::Single(Body::Raw(RawBody {
                kind: RawBodyKind::Json,
                data: Value::Object(envelope).to_string(),
            })));
        }
        Ok(self)
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

fn append_graphql_parameter(
    request: &mut HttpRequest,
    name: &str,
    value: Option<Map<String, Value>>,
) {
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
