//! OpenCollection YAML adapter for Probe.
//!
//! The adapter retains the source document for loss-preserving serialization and
//! projects the supported subset into serialization-independent domain models.

#![forbid(unsafe_code)]

use std::{collections::BTreeMap, error::Error as StdError, fmt, time::Duration};

use probe_core::{
    Authentication, AuthenticationKind, AuthenticationValue, Author, Body, BodyVariant, Collection,
    CollectionItem, CollectionMetadata, Environment, EnvironmentVariable, FileReference, Folder,
    FormField, GraphqlBody, GraphqlBodyVariant, GraphqlOperation, GraphqlRequest, Header,
    HttpRequest, ItemMetadata, MultipartPart, MultipartPartKind, MultipartValue, QueryParameter,
    RawBody, RawBodyKind, RequestBody, RequestSettings, SecretVariable, Variable, VariableValue,
    VariableValueSet, VariableValueType, VariableValueVariant, validate_environments,
};
use serde::Deserialize;
use serde_yaml_ng::Value;

mod repository;
mod structure;

pub use repository::{
    CompletedEnvironmentCreate, CompletedEnvironmentDelete, CompletedEnvironmentReplace,
    CompletedEnvironmentSave, CompletedRequestSave, CreateError, LoadError, LoadedWorkspace,
    LocatedFolder, LocatedRequest, PreparedEnvironmentCreate, PreparedEnvironmentDelete,
    PreparedEnvironmentReplace, PreparedEnvironmentSave, PreparedRequestSave, SaveError,
    create_bundled_workspace, create_bundled_workspace_from_collection, load_workspace,
    load_workspace_from_str,
};
pub use structure::{
    CreatedRequestProtocol, ItemKind, StructureError, StructureOperation, StructureResult,
};

/// An OpenCollection document together with its supported domain projection.
#[derive(Clone, Debug)]
pub struct ParsedCollection {
    collection: Collection,
    document: Value,
    bundled: bool,
    diagnostics: Vec<ProjectionDiagnostic>,
}

impl ParsedCollection {
    pub(crate) const fn document(&self) -> &Value {
        &self.document
    }

    pub(crate) const fn is_bundled(&self) -> bool {
        self.bundled
    }

    /// Returns the serialization-independent collection model.
    #[must_use]
    pub const fn collection(&self) -> &Collection {
        &self.collection
    }

    /// Source values Probe cannot project or execute, while retaining their YAML.
    #[must_use]
    pub fn diagnostics(&self) -> &[ProjectionDiagnostic] {
        &self.diagnostics
    }

    /// Consumes the parsed document and returns its domain model.
    #[must_use]
    pub fn into_collection(self) -> Collection {
        self.collection
    }

    /// Serializes the retained OpenCollection document back to YAML.
    ///
    /// Unsupported fields are emitted from the retained document rather than rebuilt
    /// from the supported domain projection.
    pub fn to_yaml(&self) -> Result<String, ParseError> {
        serde_yaml_ng::to_string(&self.document).map_err(ParseError::new)
    }
}

/// A retained source value that Probe cannot project, or projects but cannot execute.
/// Authentication kinds and properties may remain in the domain while the HTTP
/// engine ignores them. The public diagnostic codes cover both limitations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectionDiagnostic {
    /// Structural path within a bundled document, or a workspace-relative file path.
    pub path: String,
    /// Stable category of unsupported value.
    pub kind: ProjectionDiagnosticKind,
    /// The unsupported type or property name.
    pub value: String,
}

/// Categories of unsupported OpenCollection projection or execution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProjectionDiagnosticKind {
    ItemType,
    BodyType,
    ParameterType,
    AuthenticationKind,
    AuthenticationProperty,
}

impl ProjectionDiagnosticKind {
    /// Stable machine-readable diagnostic code.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ItemType => "unsupported_item_type",
            Self::BodyType => "unsupported_body_type",
            Self::ParameterType => "unsupported_parameter_type",
            Self::AuthenticationKind => "unsupported_authentication_kind",
            Self::AuthenticationProperty => "unsupported_authentication_property",
        }
    }
}

/// An error raised while parsing or serializing OpenCollection YAML.
#[derive(Debug)]
pub struct ParseError {
    source: serde_yaml_ng::Error,
}

impl ParseError {
    fn new(source: serde_yaml_ng::Error) -> Self {
        Self { source }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid OpenCollection YAML: {}", self.source)
    }
}

impl StdError for ParseError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        Some(&self.source)
    }
}

/// Parses a bundled OpenCollection YAML document.
///
/// Unsupported items and fields remain in the retained YAML document. Projection
/// diagnostics identify values that cannot be represented or executed by Probe.
pub fn parse(source: &str) -> Result<ParsedCollection, ParseError> {
    let document: Value = serde_yaml_ng::from_str(source).map_err(ParseError::new)?;
    let wire: CollectionDocument =
        serde_yaml_ng::from_value(document.clone()).map_err(ParseError::new)?;
    if wire.opencollection != "1.0.0" {
        return Err(ParseError::new(
            <serde_yaml_ng::Error as serde::de::Error>::custom(format!(
                "unsupported OpenCollection version '{}'; supported version is 1.0.0",
                wire.opencollection
            )),
        ));
    }
    let bundled = wire.bundled;
    let mut diagnostics = Vec::new();
    let collection = wire
        .into_domain(&mut diagnostics)
        .map_err(ParseError::new)?;
    sort_diagnostics(&mut diagnostics);
    validate_environments(&collection.environments).map_err(|error| {
        ParseError::new(<serde_yaml_ng::Error as serde::de::Error>::custom(
            error.to_string(),
        ))
    })?;

    Ok(ParsedCollection {
        collection,
        document,
        bundled,
        diagnostics,
    })
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CollectionDocument {
    opencollection: String,
    info: CollectionInfoDocument,
    bundled: bool,
    #[serde(default)]
    items: Vec<Value>,
    #[serde(default)]
    config: CollectionConfigDocument,
}

impl CollectionDocument {
    fn into_domain(
        self,
        diagnostics: &mut Vec<ProjectionDiagnostic>,
    ) -> Result<Collection, serde_yaml_ng::Error> {
        Ok(Collection {
            metadata: self.info.into_domain(),
            items: project_items(self.items, "items", diagnostics)?,
            environments: self
                .config
                .environments
                .into_iter()
                .map(EnvironmentDocument::into_domain)
                .collect(),
        })
    }
}

#[derive(Debug, Default, Deserialize)]
struct CollectionConfigDocument {
    #[serde(default)]
    environments: Vec<EnvironmentDocument>,
}

#[derive(Debug, Default, Deserialize)]
struct CollectionInfoDocument {
    name: Option<String>,
    summary: Option<String>,
    version: Option<String>,
    #[serde(default)]
    authors: Vec<AuthorDocument>,
}

impl CollectionInfoDocument {
    fn into_domain(self) -> CollectionMetadata {
        CollectionMetadata {
            name: self.name,
            summary: self.summary,
            version: self.version,
            authors: self
                .authors
                .into_iter()
                .map(AuthorDocument::into_domain)
                .collect(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct AuthorDocument {
    name: Option<String>,
    email: Option<String>,
    url: Option<String>,
}

impl AuthorDocument {
    fn into_domain(self) -> Author {
        Author {
            name: self.name,
            email: self.email,
            url: self.url,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct ItemInfoDocument {
    name: Option<String>,
    seq: Option<f64>,
}

#[derive(Debug, Default, Deserialize)]
struct ItemKindDocument {
    #[serde(default)]
    info: ItemKindInfoDocument,
}

#[derive(Debug, Default, Deserialize)]
struct ItemKindInfoDocument {
    #[serde(rename = "type")]
    item_type: Option<String>,
}

impl ItemInfoDocument {
    fn into_domain(self) -> ItemMetadata {
        ItemMetadata {
            name: self.name,
            sequence: self.seq,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct ItemDocument {
    #[serde(default)]
    info: ItemInfoDocument,
    #[serde(default)]
    items: Vec<Value>,
    http: Option<HttpDetailsDocument>,
    graphql: Option<GraphqlDetailsDocument>,
    #[serde(default)]
    settings: RequestSettingsDocument,
}

#[derive(Debug, Default, Deserialize)]
struct RequestSettingsDocument {
    timeout: Option<Value>,
    #[serde(rename = "followRedirects")]
    follow_redirects: Option<bool>,
    #[serde(rename = "maxRedirects")]
    max_redirects: Option<usize>,
}

impl RequestSettingsDocument {
    fn into_domain(self) -> Result<RequestSettings, serde_yaml_ng::Error> {
        let timeout = match self.timeout {
            None => None,
            Some(Value::String(value)) if value == "inherit" => None,
            Some(Value::Number(value)) => {
                let milliseconds = value.as_f64().ok_or_else(|| {
                    <serde_yaml_ng::Error as serde::de::Error>::custom(
                        "request timeout must be a finite non-negative number",
                    )
                })?;
                if milliseconds.is_sign_negative() || !milliseconds.is_finite() {
                    return Err(<serde_yaml_ng::Error as serde::de::Error>::custom(
                        "request timeout must be a finite non-negative number",
                    ));
                }
                Some(
                    Duration::try_from_secs_f64(milliseconds / 1000.0).map_err(|_| {
                        <serde_yaml_ng::Error as serde::de::Error>::custom(
                            "request timeout is too large",
                        )
                    })?,
                )
            }
            Some(_) => {
                return Err(<serde_yaml_ng::Error as serde::de::Error>::custom(
                    "request timeout must be milliseconds or 'inherit'",
                ));
            }
        };
        Ok(RequestSettings {
            timeout,
            follow_redirects: self.follow_redirects,
            max_redirects: self.max_redirects,
        })
    }
}

#[derive(Debug, Default, Deserialize)]
struct HttpDetailsDocument {
    method: Option<String>,
    url: Option<String>,
    #[serde(default)]
    headers: Vec<HeaderDocument>,
    #[serde(default)]
    params: Vec<Value>,
    body: Option<Value>,
    auth: Option<Value>,
}

#[derive(Debug, Default, Deserialize)]
struct GraphqlDetailsDocument {
    method: Option<String>,
    url: Option<String>,
    #[serde(default)]
    headers: Vec<HeaderDocument>,
    #[serde(default)]
    params: Vec<Value>,
    body: Option<Value>,
    auth: Option<Value>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphqlBodyDocument {
    query: Option<String>,
    variables: Option<Value>,
    operation_name: Option<String>,
    extensions: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct GraphqlBodyVariantDocument {
    title: String,
    #[serde(default)]
    selected: bool,
    body: GraphqlBodyDocument,
}

#[derive(Debug, Deserialize)]
struct HeaderDocument {
    name: String,
    value: String,
    #[serde(default)]
    disabled: bool,
}

impl HeaderDocument {
    fn into_domain(self) -> Header {
        Header {
            name: self.name,
            value: self.value,
            disabled: self.disabled,
        }
    }
}

#[derive(Debug, Deserialize)]
struct ParameterDocument {
    name: String,
    value: String,
    #[serde(default)]
    disabled: bool,
}

impl ParameterDocument {
    fn into_domain(self) -> QueryParameter {
        QueryParameter {
            name: self.name,
            value: self.value,
            disabled: self.disabled,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EnvironmentDocument {
    name: String,
    color: Option<String>,
    extends: Option<String>,
    dot_env_file_path: Option<String>,
    #[serde(default)]
    variables: Vec<EnvironmentVariableDocument>,
}

impl EnvironmentDocument {
    fn into_domain(self) -> Environment {
        Environment {
            name: self.name,
            color: self.color,
            extends: self.extends,
            dot_env_file_path: self.dot_env_file_path,
            variables: self
                .variables
                .into_iter()
                .map(EnvironmentVariableDocument::into_domain)
                .collect(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct EnvironmentVariableDocument {
    name: Option<String>,
    value: Option<VariableValueSetDocument>,
    #[serde(default)]
    disabled: bool,
    #[serde(default)]
    secret: bool,
    #[serde(rename = "type")]
    value_type: Option<VariableValueTypeDocument>,
}

impl EnvironmentVariableDocument {
    fn into_domain(self) -> EnvironmentVariable {
        if self.secret {
            EnvironmentVariable::Secret(SecretVariable {
                name: self.name,
                value_type: self.value_type.map(VariableValueTypeDocument::into_domain),
                disabled: self.disabled,
            })
        } else {
            EnvironmentVariable::Plain(Variable {
                name: self.name,
                value: self.value.map(VariableValueSetDocument::into_domain),
                disabled: self.disabled,
            })
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum VariableValueSetDocument {
    String(String),
    Typed(TypedVariableValueDocument),
    Variants(Vec<VariableValueVariantDocument>),
}

impl VariableValueSetDocument {
    fn into_domain(self) -> VariableValueSet {
        match self {
            Self::String(value) => VariableValueSet::Single(VariableValue::String(value)),
            Self::Typed(value) => VariableValueSet::Single(value.into_domain()),
            Self::Variants(variants) => VariableValueSet::Variants(
                variants
                    .into_iter()
                    .map(VariableValueVariantDocument::into_domain)
                    .collect(),
            ),
        }
    }
}

#[derive(Debug, Deserialize)]
struct TypedVariableValueDocument {
    #[serde(rename = "type")]
    value_type: VariableValueTypeDocument,
    data: String,
}

impl TypedVariableValueDocument {
    fn into_domain(self) -> VariableValue {
        VariableValue::Typed {
            kind: self.value_type.into_domain(),
            data: self.data,
        }
    }
}

#[derive(Debug, Deserialize)]
struct VariableValueVariantDocument {
    title: String,
    #[serde(default)]
    selected: bool,
    value: VariableValueDocument,
}

impl VariableValueVariantDocument {
    fn into_domain(self) -> VariableValueVariant {
        VariableValueVariant {
            title: self.title,
            selected: self.selected,
            value: self.value.into_domain(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum VariableValueDocument {
    String(String),
    Typed(TypedVariableValueDocument),
}

impl VariableValueDocument {
    fn into_domain(self) -> VariableValue {
        match self {
            Self::String(value) => VariableValue::String(value),
            Self::Typed(value) => value.into_domain(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum VariableValueTypeDocument {
    String,
    Number,
    Boolean,
    Null,
    Object,
}

impl VariableValueTypeDocument {
    const fn into_domain(self) -> VariableValueType {
        match self {
            Self::String => VariableValueType::String,
            Self::Number => VariableValueType::Number,
            Self::Boolean => VariableValueType::Boolean,
            Self::Null => VariableValueType::Null,
            Self::Object => VariableValueType::Object,
        }
    }
}

#[derive(Debug, Deserialize)]
struct BodyKindDocument {
    #[serde(rename = "type")]
    body_type: String,
}

#[derive(Debug, Deserialize)]
struct RawBodyDocument {
    #[serde(rename = "type")]
    body_type: RawBodyKindDocument,
    data: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum RawBodyKindDocument {
    Json,
    Text,
    Xml,
    Sparql,
}

impl RawBodyKindDocument {
    const fn into_domain(self) -> RawBodyKind {
        match self {
            Self::Json => RawBodyKind::Json,
            Self::Text => RawBodyKind::Text,
            Self::Xml => RawBodyKind::Xml,
            Self::Sparql => RawBodyKind::Sparql,
        }
    }
}

#[derive(Debug, Deserialize)]
struct FormBodyDocument {
    data: Vec<FormFieldDocument>,
}

#[derive(Debug, Deserialize)]
struct FormFieldDocument {
    name: String,
    value: String,
    #[serde(default)]
    disabled: bool,
}

impl FormFieldDocument {
    fn into_domain(self) -> FormField {
        FormField {
            name: self.name,
            value: self.value,
            disabled: self.disabled,
        }
    }
}

#[derive(Debug, Deserialize)]
struct MultipartBodyDocument {
    data: Vec<MultipartPartDocument>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MultipartPartDocument {
    name: String,
    #[serde(rename = "type")]
    part_type: MultipartPartKindDocument,
    value: MultipartValueDocument,
    content_type: Option<String>,
    #[serde(default)]
    disabled: bool,
}

impl MultipartPartDocument {
    fn into_domain(self) -> MultipartPart {
        MultipartPart {
            name: self.name,
            kind: self.part_type.into_domain(),
            value: self.value.into_domain(),
            content_type: self.content_type,
            disabled: self.disabled,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum MultipartPartKindDocument {
    Text,
    File,
}

impl MultipartPartKindDocument {
    const fn into_domain(self) -> MultipartPartKind {
        match self {
            Self::Text => MultipartPartKind::Text,
            Self::File => MultipartPartKind::File,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum MultipartValueDocument {
    Single(String),
    Multiple(Vec<String>),
}

impl MultipartValueDocument {
    fn into_domain(self) -> MultipartValue {
        match self {
            Self::Single(value) => MultipartValue::Single(value),
            Self::Multiple(values) => MultipartValue::Multiple(values),
        }
    }
}

#[derive(Debug, Deserialize)]
struct FileBodyDocument {
    data: Vec<FileReferenceDocument>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileReferenceDocument {
    file_path: String,
    content_type: String,
    selected: bool,
}

impl FileReferenceDocument {
    fn into_domain(self) -> FileReference {
        FileReference {
            file_path: self.file_path,
            content_type: self.content_type,
            selected: self.selected,
        }
    }
}

#[derive(Debug, Deserialize)]
struct BodyVariantDocument {
    title: String,
    #[serde(default)]
    selected: bool,
    body: Value,
}

fn diagnostic(
    diagnostics: &mut Vec<ProjectionDiagnostic>,
    path: String,
    kind: ProjectionDiagnosticKind,
    value: impl Into<String>,
) {
    diagnostics.push(ProjectionDiagnostic {
        path,
        kind,
        value: value.into(),
    });
}

fn sort_diagnostics(diagnostics: &mut [ProjectionDiagnostic]) {
    diagnostics.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.kind.as_str().cmp(right.kind.as_str()))
            .then_with(|| left.value.cmp(&right.value))
    });
}

fn project_request_body(
    value: Value,
    path: &str,
    diagnostics: &mut Vec<ProjectionDiagnostic>,
) -> Result<Option<RequestBody>, serde_yaml_ng::Error> {
    if value.is_sequence() {
        let variants: Vec<BodyVariantDocument> = serde_yaml_ng::from_value(value)?;
        let mut projected = Vec::with_capacity(variants.len());

        for (index, variant) in variants.into_iter().enumerate() {
            if let Some(body) =
                project_body(variant.body, &format!("{path}/{index}/body"), diagnostics)?
            {
                projected.push(BodyVariant {
                    title: variant.title,
                    selected: variant.selected,
                    body,
                });
            }
        }

        Ok(Some(RequestBody::Variants(projected)))
    } else {
        Ok(project_body(value, path, diagnostics)?.map(RequestBody::Single))
    }
}

fn project_graphql_body(value: Value) -> Result<GraphqlBody, serde_yaml_ng::Error> {
    if value.is_sequence() {
        let variants: Vec<GraphqlBodyVariantDocument> = serde_yaml_ng::from_value(value)?;
        Ok(GraphqlBody::Variants(
            variants
                .into_iter()
                .map(|variant| {
                    project_graphql_operation(variant.body).map(|body| GraphqlBodyVariant {
                        title: variant.title,
                        selected: variant.selected,
                        body,
                    })
                })
                .collect::<Result<_, _>>()?,
        ))
    } else {
        let body: GraphqlBodyDocument = serde_yaml_ng::from_value(value)?;
        Ok(GraphqlBody::Single(project_graphql_operation(body)?))
    }
}

fn project_graphql_operation(
    body: GraphqlBodyDocument,
) -> Result<GraphqlOperation, serde_yaml_ng::Error> {
    Ok(GraphqlOperation {
        query: body.query,
        variables: body
            .variables
            .map(|value| project_graphql_object(value, "variables"))
            .transpose()?,
        operation_name: body.operation_name,
        extensions: body
            .extensions
            .map(|value| project_graphql_object(value, "extensions"))
            .transpose()?,
    })
}

fn project_graphql_object(
    value: Value,
    field: &str,
) -> Result<serde_json::Map<String, serde_json::Value>, serde_yaml_ng::Error> {
    let value = match value {
        Value::String(source) => serde_json::from_str(&source).map_err(|error| {
            <serde_yaml_ng::Error as serde::de::Error>::custom(format!(
                "GraphQL {field} must contain a JSON object: {error}"
            ))
        })?,
        value => serde_json::to_value(value).map_err(|error| {
            <serde_yaml_ng::Error as serde::de::Error>::custom(format!(
                "GraphQL {field} must be a JSON object: {error}"
            ))
        })?,
    };
    value.as_object().cloned().ok_or_else(|| {
        <serde_yaml_ng::Error as serde::de::Error>::custom(format!(
            "GraphQL {field} must be a JSON object"
        ))
    })
}

fn project_body(
    value: Value,
    path: &str,
    diagnostics: &mut Vec<ProjectionDiagnostic>,
) -> Result<Option<Body>, serde_yaml_ng::Error> {
    let kind: BodyKindDocument = serde_yaml_ng::from_value(value.clone())?;

    match kind.body_type.as_str() {
        "json" | "text" | "xml" | "sparql" => {
            let body: RawBodyDocument = serde_yaml_ng::from_value(value)?;
            Ok(Some(Body::Raw(RawBody {
                kind: body.body_type.into_domain(),
                data: body.data,
            })))
        }
        "form-urlencoded" => {
            let body: FormBodyDocument = serde_yaml_ng::from_value(value)?;
            Ok(Some(Body::FormUrlEncoded(
                body.data
                    .into_iter()
                    .map(FormFieldDocument::into_domain)
                    .collect(),
            )))
        }
        "multipart-form" => {
            let body: MultipartBodyDocument = serde_yaml_ng::from_value(value)?;
            Ok(Some(Body::Multipart(
                body.data
                    .into_iter()
                    .map(MultipartPartDocument::into_domain)
                    .collect(),
            )))
        }
        "file" => {
            let body: FileBodyDocument = serde_yaml_ng::from_value(value)?;
            Ok(Some(Body::File(
                body.data
                    .into_iter()
                    .map(FileReferenceDocument::into_domain)
                    .collect(),
            )))
        }
        other => {
            diagnostic(
                diagnostics,
                format!("{path}/type"),
                ProjectionDiagnosticKind::BodyType,
                other,
            );
            Ok(None)
        }
    }
}

fn project_parameters(
    parameters: Vec<Value>,
    location: &str,
    diagnostics: &mut Vec<ProjectionDiagnostic>,
) -> Result<(Vec<QueryParameter>, Vec<QueryParameter>), serde_yaml_ng::Error> {
    let mut query = Vec::new();
    let mut path_parameters = Vec::new();
    for (index, value) in parameters.into_iter().enumerate() {
        let kind = value.get("type").and_then(Value::as_str);
        match kind {
            Some("query") => {
                let parameter: ParameterDocument = serde_yaml_ng::from_value(value)?;
                query.push(parameter.into_domain());
            }
            Some("path") => {
                let parameter: ParameterDocument = serde_yaml_ng::from_value(value)?;
                path_parameters.push(parameter.into_domain());
            }
            _ => diagnostic(
                diagnostics,
                format!("{location}/{index}/type"),
                ProjectionDiagnosticKind::ParameterType,
                kind.unwrap_or("<missing>"),
            ),
        }
    }
    Ok((query, path_parameters))
}

fn project_authentication(
    value: Value,
    path: &str,
    diagnostics: &mut Vec<ProjectionDiagnostic>,
) -> Result<Authentication, serde_yaml_ng::Error> {
    Ok(match value {
        Value::String(value) => {
            if value != "inherit" {
                diagnostic(
                    diagnostics,
                    path.to_owned(),
                    ProjectionDiagnosticKind::AuthenticationKind,
                    &value,
                );
            }
            Authentication {
                kind: if value == "inherit" {
                    AuthenticationKind::Inherit
                } else {
                    AuthenticationKind::Other(value)
                },
                properties: BTreeMap::new(),
            }
        }
        Value::Mapping(properties) => {
            let kind = properties
                .get(Value::String("type".to_owned()))
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    <serde_yaml_ng::Error as serde::de::Error>::custom(
                        "authentication type must be a string",
                    )
                })?
                .to_owned();
            if !matches!(kind.as_str(), "basic" | "bearer") {
                diagnostic(
                    diagnostics,
                    format!("{path}/type"),
                    ProjectionDiagnosticKind::AuthenticationKind,
                    &kind,
                );
            }
            let mut projected_properties = BTreeMap::new();
            for (name, value) in properties {
                let Some(name) = name.as_str() else {
                    diagnostic(
                        diagnostics,
                        path.to_owned(),
                        ProjectionDiagnosticKind::AuthenticationProperty,
                        format!("{name:?}"),
                    );
                    continue;
                };
                if name == "type" {
                    continue;
                }
                let supported = match kind.as_str() {
                    "basic" => matches!(name, "username" | "password"),
                    "bearer" => name == "token",
                    _ => true,
                };
                if !supported || (matches!(kind.as_str(), "basic" | "bearer") && !value.is_string())
                {
                    diagnostic(
                        diagnostics,
                        format!("{path}/{name}"),
                        ProjectionDiagnosticKind::AuthenticationProperty,
                        name,
                    );
                }
                projected_properties.insert(
                    name.to_owned(),
                    authentication_value(value, &format!("{path}/{name}"), diagnostics),
                );
            }
            Authentication {
                kind: match kind.as_str() {
                    "awsv4" => AuthenticationKind::AwsV4,
                    "basic" => AuthenticationKind::Basic,
                    "wsse" => AuthenticationKind::Wsse,
                    "bearer" => AuthenticationKind::Bearer,
                    "digest" => AuthenticationKind::Digest,
                    "ntlm" => AuthenticationKind::Ntlm,
                    "apikey" => AuthenticationKind::ApiKey,
                    "oauth1" => AuthenticationKind::OAuth1,
                    "oauth2" => AuthenticationKind::OAuth2,
                    other => AuthenticationKind::Other(other.to_owned()),
                },
                properties: projected_properties,
            }
        }
        _ => {
            return Err(<serde_yaml_ng::Error as serde::de::Error>::custom(
                "authentication must be a string or mapping",
            ));
        }
    })
}

fn authentication_value(
    value: Value,
    path: &str,
    diagnostics: &mut Vec<ProjectionDiagnostic>,
) -> AuthenticationValue {
    match value {
        Value::Null => AuthenticationValue::Null,
        Value::Bool(value) => AuthenticationValue::Boolean(value),
        Value::Number(value) => AuthenticationValue::Number(value.to_string()),
        Value::String(value) => AuthenticationValue::String(value),
        Value::Sequence(values) => AuthenticationValue::Sequence(
            values
                .into_iter()
                .enumerate()
                .map(|(index, value)| {
                    authentication_value(value, &format!("{path}/{index}"), diagnostics)
                })
                .collect(),
        ),
        Value::Mapping(values) => {
            let mut projected = BTreeMap::new();
            for (name, value) in values {
                if let Some(name) = name.as_str() {
                    projected.insert(
                        name.to_owned(),
                        authentication_value(value, &format!("{path}/{name}"), diagnostics),
                    );
                } else {
                    diagnostic(
                        diagnostics,
                        path.to_owned(),
                        ProjectionDiagnosticKind::AuthenticationProperty,
                        format!("{name:?}"),
                    );
                }
            }
            AuthenticationValue::Object(projected)
        }
        Value::Tagged(value) => authentication_value(value.value, path, diagnostics),
    }
}

fn project_items(
    items: Vec<Value>,
    path: &str,
    diagnostics: &mut Vec<ProjectionDiagnostic>,
) -> Result<Vec<CollectionItem>, serde_yaml_ng::Error> {
    items
        .into_iter()
        .enumerate()
        .map(|(index, value)| project_item(value, &format!("{path}/{index}"), diagnostics))
        .filter_map(Result::transpose)
        .collect()
}

fn project_item(
    value: Value,
    path: &str,
    diagnostics: &mut Vec<ProjectionDiagnostic>,
) -> Result<Option<CollectionItem>, serde_yaml_ng::Error> {
    let kind: ItemKindDocument = serde_yaml_ng::from_value(value.clone())?;

    match kind.info.item_type.as_deref() {
        Some("folder") => {
            let item: ItemDocument = serde_yaml_ng::from_value(value)?;
            Ok(Some(CollectionItem::Folder(Folder {
                metadata: item.info.into_domain(),
                items: project_items(item.items, &format!("{path}/items"), diagnostics)?,
            })))
        }
        Some("http") => {
            let item: ItemDocument = serde_yaml_ng::from_value(value)?;
            let settings = item.settings.into_domain()?;
            let http = item.http.unwrap_or_default();
            let body = http
                .body
                .map(|value| project_request_body(value, &format!("{path}/http/body"), diagnostics))
                .transpose()?
                .flatten();
            let authentication = http
                .auth
                .map(|value| {
                    project_authentication(value, &format!("{path}/http/auth"), diagnostics)
                })
                .transpose()?;
            let (query_parameters, path_parameters) =
                project_parameters(http.params, &format!("{path}/http/params"), diagnostics)?;
            Ok(Some(CollectionItem::HttpRequest(HttpRequest {
                metadata: item.info.into_domain(),
                method: http.method,
                url: http.url,
                headers: http
                    .headers
                    .into_iter()
                    .map(HeaderDocument::into_domain)
                    .collect(),
                query_parameters,
                path_parameters,
                body,
                authentication,
                settings,
                protocol: probe_core::RequestProtocol::Http,
            })))
        }
        Some("graphql") => {
            let item: ItemDocument = serde_yaml_ng::from_value(value)?;
            let settings = item.settings.into_domain()?;
            let graphql = item.graphql.unwrap_or_default();
            let body = graphql.body.map(project_graphql_body).transpose()?;
            let authentication = graphql
                .auth
                .map(|value| {
                    project_authentication(value, &format!("{path}/graphql/auth"), diagnostics)
                })
                .transpose()?;
            let (query_parameters, path_parameters) = project_parameters(
                graphql.params,
                &format!("{path}/graphql/params"),
                diagnostics,
            )?;
            Ok(Some(CollectionItem::GraphqlRequest(GraphqlRequest {
                metadata: item.info.into_domain(),
                method: graphql.method,
                url: graphql.url,
                headers: graphql
                    .headers
                    .into_iter()
                    .map(HeaderDocument::into_domain)
                    .collect(),
                query_parameters,
                path_parameters,
                body,
                authentication,
                settings,
            })))
        }
        other => {
            diagnostic(
                diagnostics,
                format!("{path}/info/type"),
                ProjectionDiagnosticKind::ItemType,
                other.unwrap_or("<missing>"),
            );
            Ok(None)
        }
    }
}
