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

/// A non-interactive partial update to an HTTP request.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RequestUpdate {
    /// Replacement request name.
    pub name: Option<String>,
    /// Replacement HTTP method.
    pub method: Option<String>,
    /// Replacement URL.
    pub url: Option<String>,
    /// Replacement headers.
    pub headers: Option<Vec<Header>>,
    /// Replacement query parameters.
    pub query_parameters: Option<Vec<QueryParameter>>,
    /// Replacement path parameters.
    pub path_parameters: Option<Vec<QueryParameter>>,
    /// Replacement body; inner `None` removes it.
    pub body: Option<Option<RequestBody>>,
    /// Replacement authentication; inner `None` removes it.
    pub authentication: Option<Option<Authentication>>,
    /// Partial native GraphQL body update.
    pub graphql: Option<GraphqlUpdate>,
}

impl RequestUpdate {
    /// Returns whether every field is unchanged.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.method.is_none()
            && self.url.is_none()
            && self.headers.is_none()
            && self.query_parameters.is_none()
            && self.path_parameters.is_none()
            && self.body.is_none()
            && self.authentication.is_none()
            && self.graphql.as_ref().is_none_or(GraphqlUpdate::is_empty)
    }

    /// Applies the update to a domain request, including native GraphQL fields.
    pub fn apply(&self, request: &mut HttpRequest) -> Result<(), GraphqlRequestError> {
        if let Some(name) = &self.name {
            request.metadata.name = Some(name.clone());
        }
        if let Some(method) = &self.method {
            request.method = Some(method.clone());
        }
        if let Some(url) = &self.url {
            request.url = Some(url.clone());
        }
        if let Some(headers) = &self.headers {
            request.headers.clone_from(headers);
        }
        if let Some(parameters) = &self.query_parameters {
            request.query_parameters.clone_from(parameters);
        }
        if let Some(parameters) = &self.path_parameters {
            request.path_parameters.clone_from(parameters);
        }
        if let Some(body) = &self.body {
            request.body.clone_from(body);
        }
        if let Some(authentication) = &self.authentication {
            request.authentication.clone_from(authentication);
        }
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
    pub query: Option<String>,
    /// Replacement variables; inner `None` clears them.
    pub variables: Option<Option<Map<String, Value>>>,
    /// Replacement operation name; inner `None` clears it.
    pub operation_name: Option<Option<String>>,
    /// Replacement extensions; inner `None` clears them.
    pub extensions: Option<Option<Map<String, Value>>>,
}

impl GraphqlUpdate {
    /// Returns whether every GraphQL field is unchanged.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.query.is_none()
            && self.variables.is_none()
            && self.operation_name.is_none()
            && self.extensions.is_none()
    }

    /// Applies this update to one operation.
    pub fn apply(&self, operation: &mut GraphqlOperation) {
        if let Some(query) = &self.query {
            operation.query = Some(query.clone());
        }
        if let Some(variables) = &self.variables {
            operation.variables.clone_from(variables);
        }
        if let Some(operation_name) = &self.operation_name {
            operation.operation_name.clone_from(operation_name);
        }
        if let Some(extensions) = &self.extensions {
            operation.extensions.clone_from(extensions);
        }
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

    /// Returns the selected native GraphQL operation.
    pub fn selected_graphql(&self) -> Result<Option<&GraphqlOperation>, GraphqlRequestError> {
        match self.graphql() {
            None => Ok(None),
            Some(GraphqlBody::Single(operation)) => Ok(Some(operation)),
            Some(GraphqlBody::Variants(variants)) => {
                let mut selected = variants.iter().filter(|variant| variant.selected);
                let operation = selected.next().ok_or_else(|| {
                    GraphqlRequestError::InvalidBodySelection(
                        "GraphQL body variants have no selected value".to_owned(),
                    )
                })?;
                if selected.next().is_some() {
                    return Err(GraphqlRequestError::InvalidBodySelection(
                        "GraphQL body variants have multiple selected values".to_owned(),
                    ));
                }
                Ok(Some(&operation.body))
            }
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
        if body.is_none() {
            *body = Some(GraphqlBody::Single(GraphqlOperation::default()));
        }
        let operation = match body.as_mut().expect("GraphQL body was initialized") {
            GraphqlBody::Single(operation) => operation,
            GraphqlBody::Variants(variants) => {
                let mut selected = variants.iter_mut().filter(|variant| variant.selected);
                let operation = selected.next().ok_or_else(|| {
                    GraphqlRequestError::InvalidBodySelection(
                        "GraphQL body variants have no selected value".to_owned(),
                    )
                })?;
                if selected.next().is_some() {
                    return Err(GraphqlRequestError::InvalidBodySelection(
                        "GraphQL body variants have multiple selected values".to_owned(),
                    ));
                }
                &mut operation.body
            }
        };
        update.apply(operation);
        Ok(())
    }

    /// Builds the GraphQL-over-HTTP request consumed by Probe's HTTP engine.
    pub fn prepare_http(&self) -> Result<Self, GraphqlRequestError> {
        if matches!(self.protocol, RequestProtocol::Http) {
            return Ok(self.clone());
        }
        let mut request = self.clone();
        request.protocol = RequestProtocol::Http;
        let operation = self.selected_graphql()?.cloned().unwrap_or_default();
        if self
            .method
            .as_deref()
            .is_some_and(|method| method.eq_ignore_ascii_case("GET"))
        {
            request
                .query_parameters
                .retain(|parameter| !is_graphql_http_parameter(&parameter.name));
            if let Some(query) = operation.query {
                request.query_parameters.push(QueryParameter {
                    name: "query".to_owned(),
                    value: query,
                    disabled: false,
                });
            }
            append_graphql_parameter(&mut request, "variables", operation.variables);
            if let Some(operation_name) = operation.operation_name {
                request.query_parameters.push(QueryParameter {
                    name: "operationName".to_owned(),
                    value: operation_name,
                    disabled: false,
                });
            }
            append_graphql_parameter(&mut request, "extensions", operation.extensions);
        } else {
            let envelope = operation.into_json_envelope();
            request.body = Some(RequestBody::Single(Body::Raw(RawBody {
                kind: RawBodyKind::Json,
                data: Value::Object(envelope).to_string(),
            })));
        }
        Ok(request)
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
