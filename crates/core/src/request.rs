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
}

impl RequestUpdate {
    /// Returns whether every field is unchanged.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.method.is_none()
            && self.url.is_none()
            && self.headers.is_none()
            && self.query_parameters.is_none()
            && self.path_parameters.is_none()
            && self.body.is_none()
            && self.authentication.is_none()
    }

    /// Applies the update to a domain request.
    pub fn apply(&self, request: &mut HttpRequest) {
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

/// A GraphQL-over-HTTP JSON request envelope.
///
/// OpenCollection 1.0.0 has no GraphQL body type. Probe therefore represents this protocol as a
/// canonical `type: json` body, retaining full compatibility with generic HTTP consumers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphqlRequest {
    /// GraphQL operation document.
    pub query: String,
    /// Optional GraphQL variables object.
    pub variables: Option<Map<String, Value>>,
    /// Optional operation name for documents with multiple operations.
    pub operation_name: Option<String>,
}

/// An invalid GraphQL JSON envelope or GraphQL variables value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GraphqlRequestError {
    /// The variables value is not a JSON object.
    VariablesMustBeObject,
    /// A field in an otherwise GraphQL-shaped envelope has an unsupported type.
    InvalidField {
        field: &'static str,
        expected: &'static str,
    },
}

impl std::fmt::Display for GraphqlRequestError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::VariablesMustBeObject => {
                formatter.write_str("GraphQL variables must be a JSON object")
            }
            Self::InvalidField { field, expected } => {
                write!(formatter, "GraphQL field '{field}' must be {expected}")
            }
        }
    }
}

impl std::error::Error for GraphqlRequestError {}

impl GraphqlRequest {
    /// Builds a GraphQL request from a query and optional JSON object source for variables.
    pub fn from_parts(
        query: String,
        variables_json: Option<&str>,
        operation_name: Option<String>,
    ) -> Result<Self, GraphqlRequestError> {
        let variables = variables_json
            .map(|source| {
                serde_json::from_str::<Value>(source)
                    .ok()
                    .and_then(|value| value.as_object().cloned())
                    .ok_or(GraphqlRequestError::VariablesMustBeObject)
            })
            .transpose()?;
        Ok(Self {
            query,
            variables,
            operation_name,
        })
    }

    /// Projects a JSON body into GraphQL fields when it is an exact conventional envelope.
    ///
    /// Envelopes with extension fields remain generic JSON so that a later typed update cannot
    /// discard data that OpenCollection does not model.
    pub fn from_json_envelope(source: &str) -> Result<Option<Self>, GraphqlRequestError> {
        let Ok(Value::Object(mut fields)) = serde_json::from_str(source) else {
            return Ok(None);
        };
        let Some(query) = fields.remove("query") else {
            return Ok(None);
        };
        if fields
            .keys()
            .any(|name| name != "variables" && name != "operationName")
        {
            return Ok(None);
        }
        let query = query
            .as_str()
            .map(str::to_owned)
            .ok_or(Self::invalid_field("query", "a string"))?;
        let variables = match fields.remove("variables") {
            None | Some(Value::Null) => None,
            Some(Value::Object(value)) => Some(value),
            Some(_) => return Err(GraphqlRequestError::VariablesMustBeObject),
        };
        let operation_name = match fields.remove("operationName") {
            None | Some(Value::Null) => None,
            Some(Value::String(value)) => Some(value),
            Some(_) => return Err(Self::invalid_field("operationName", "a string")),
        };
        Ok(Some(Self {
            query,
            variables,
            operation_name,
        }))
    }

    /// Returns this request as the canonical OpenCollection raw JSON body.
    #[must_use]
    pub fn as_raw_body(&self) -> RawBody {
        RawBody {
            kind: RawBodyKind::Json,
            data: self.to_json_envelope(),
        }
    }

    /// Serializes the GraphQL protocol envelope as JSON.
    #[must_use]
    pub fn to_json_envelope(&self) -> String {
        let mut fields = Map::new();
        fields.insert("query".to_owned(), Value::String(self.query.clone()));
        if let Some(variables) = &self.variables {
            fields.insert("variables".to_owned(), Value::Object(variables.clone()));
        }
        if let Some(operation_name) = &self.operation_name {
            fields.insert(
                "operationName".to_owned(),
                Value::String(operation_name.clone()),
            );
        }
        Value::Object(fields).to_string()
    }

    const fn invalid_field(field: &'static str, expected: &'static str) -> GraphqlRequestError {
        GraphqlRequestError::InvalidField { field, expected }
    }
}

impl HttpRequest {
    /// Returns a first-class GraphQL view when this request uses the compatible JSON envelope.
    ///
    /// Unsupported JSON bodies are intentionally left generic to preserve their source data.
    pub fn graphql(&self) -> Result<Option<GraphqlRequest>, GraphqlRequestError> {
        let Some(RequestBody::Single(Body::Raw(RawBody {
            kind: RawBodyKind::Json,
            data,
        }))) = &self.body
        else {
            return Ok(None);
        };
        GraphqlRequest::from_json_envelope(data)
    }
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
