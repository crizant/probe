use std::time::Duration;

use probe_core::{
    Author, CollectionMetadata, Documentation, Environment, EnvironmentVariable, FileReference,
    FormField, Header, ItemMetadata, MultipartPart, MultipartPartKind, MultipartValue,
    QueryParameter, RawBodyKind, RequestSettings, SecretVariable, Variable, VariableValue,
    VariableValueSet, VariableValueType, VariableValueVariant,
};
use serde::Deserialize;
use serde_yaml_ng::Value;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CollectionDocument {
    pub(crate) opencollection: String,
    pub(crate) info: CollectionInfoDocument,
    pub(crate) bundled: bool,
    #[serde(default)]
    pub(crate) items: Vec<Value>,
    #[serde(default)]
    pub(crate) config: CollectionConfigDocument,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct CollectionConfigDocument {
    #[serde(default)]
    pub(crate) environments: Vec<EnvironmentDocument>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct CollectionInfoDocument {
    pub(crate) name: Option<String>,
    pub(crate) summary: Option<String>,
    pub(crate) version: Option<String>,
    #[serde(default)]
    pub(crate) authors: Vec<AuthorDocument>,
}

impl CollectionInfoDocument {
    pub(crate) fn into_domain(self) -> CollectionMetadata {
        CollectionMetadata {
            name: self.name,
            summary: self.summary,
            version: self.version,
            authors: self
                .authors
                .into_iter()
                .map(AuthorDocument::into_domain)
                .collect(),
            docs: None,
        }
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct AuthorDocument {
    pub(crate) name: Option<String>,
    pub(crate) email: Option<String>,
    pub(crate) url: Option<String>,
}

impl AuthorDocument {
    pub(crate) fn into_domain(self) -> Author {
        Author {
            name: self.name,
            email: self.email,
            url: self.url,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct ItemInfoDocument {
    pub(crate) name: Option<String>,
    pub(crate) seq: Option<f64>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct ItemKindDocument {
    #[serde(default)]
    pub(crate) info: ItemKindInfoDocument,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct ItemKindInfoDocument {
    #[serde(rename = "type")]
    pub(crate) item_type: Option<String>,
}

impl ItemInfoDocument {
    pub(crate) fn into_domain(self) -> ItemMetadata {
        ItemMetadata {
            name: self.name,
            sequence: self.seq,
            description: None,
        }
    }
}

pub(crate) fn optional_documentation(
    value: Option<&Value>,
) -> Result<Option<Documentation>, serde_yaml_ng::Error> {
    value.map(documentation_from_yaml).transpose()
}

pub(crate) fn documentation_from_yaml(
    value: &Value,
) -> Result<Documentation, serde_yaml_ng::Error> {
    match value {
        Value::Null => Ok(Documentation::Null),
        Value::String(text) => Ok(Documentation::Text(text.clone())),
        Value::Mapping(mapping) => documentation_object(mapping),
        _ => Err(yaml_error(
            "documentation must be a string, an object with content and type, or null",
        )),
    }
}

fn documentation_object(
    mapping: &serde_yaml_ng::Mapping,
) -> Result<Documentation, serde_yaml_ng::Error> {
    if mapping.len() != 2 {
        return Err(yaml_error(
            "documentation object must contain only content and type",
        ));
    }
    let content = mapping_string(mapping, "content")
        .ok_or_else(|| yaml_error("documentation content must be a string"))?;
    let media_type = mapping_string(mapping, "type")
        .ok_or_else(|| yaml_error("documentation type must be a string"))?;
    Ok(Documentation::Content {
        content,
        media_type,
    })
}

fn mapping_string(mapping: &serde_yaml_ng::Mapping, key: &str) -> Option<String> {
    mapping
        .get(Value::String(key.to_owned()))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

pub(crate) fn request_docs_from_yaml(
    value: Option<&Value>,
) -> Result<Option<String>, serde_yaml_ng::Error> {
    match value {
        None => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.clone())),
        Some(_) => Err(yaml_error(
            "request docs must be a string; an object or null is invalid",
        )),
    }
}

fn yaml_error(message: &str) -> serde_yaml_ng::Error {
    <serde_yaml_ng::Error as serde::de::Error>::custom(message)
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct ItemDocument {
    #[serde(default)]
    pub(crate) info: ItemInfoDocument,
    #[serde(default)]
    pub(crate) items: Vec<Value>,
    pub(crate) http: Option<HttpDetailsDocument>,
    pub(crate) graphql: Option<GraphqlDetailsDocument>,
    pub(crate) websocket: Option<WebSocketDetailsDocument>,
    #[serde(default)]
    pub(crate) settings: RequestSettingsDocument,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct RequestSettingsDocument {
    pub(crate) timeout: Option<Value>,
    #[serde(rename = "followRedirects")]
    pub(crate) follow_redirects: Option<bool>,
    #[serde(rename = "maxRedirects")]
    pub(crate) max_redirects: Option<usize>,
    #[serde(rename = "keepAliveInterval")]
    pub(crate) keep_alive_interval: Option<Value>,
}

impl RequestSettingsDocument {
    /// Projects HTTP and GraphQL settings, which do not define a keep-alive interval.
    pub(crate) fn into_domain(self) -> Result<RequestSettings, serde_yaml_ng::Error> {
        Ok(RequestSettings {
            timeout: milliseconds_setting(self.timeout, "request timeout")?,
            follow_redirects: self.follow_redirects,
            max_redirects: self.max_redirects,
            keep_alive_interval: None,
        })
    }

    pub(crate) fn into_websocket_domain(mut self) -> Result<RequestSettings, serde_yaml_ng::Error> {
        let keep_alive_interval =
            milliseconds_setting(self.keep_alive_interval.take(), "keep-alive interval")?;
        Ok(RequestSettings {
            keep_alive_interval,
            ..self.into_domain()?
        })
    }
}

fn milliseconds_setting(
    value: Option<Value>,
    name: &str,
) -> Result<Option<Duration>, serde_yaml_ng::Error> {
    match value {
        None => Ok(None),
        Some(Value::String(value)) if value == "inherit" => Ok(None),
        Some(Value::Number(value)) => {
            let milliseconds = value
                .as_f64()
                .filter(|milliseconds| !milliseconds.is_sign_negative() && milliseconds.is_finite())
                .ok_or_else(|| {
                    yaml_error(&format!("{name} must be a finite non-negative number"))
                })?;
            Duration::try_from_secs_f64(milliseconds / 1000.0)
                .map(Some)
                .map_err(|_| yaml_error(&format!("{name} is too large")))
        }
        Some(_) => Err(yaml_error(&format!(
            "{name} must be milliseconds or 'inherit'"
        ))),
    }
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct HttpDetailsDocument {
    pub(crate) method: Option<String>,
    pub(crate) url: Option<String>,
    #[serde(default)]
    pub(crate) headers: Vec<HeaderDocument>,
    #[serde(default)]
    pub(crate) params: Vec<Value>,
    pub(crate) body: Option<Value>,
    pub(crate) auth: Option<Value>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct GraphqlDetailsDocument {
    pub(crate) method: Option<String>,
    pub(crate) url: Option<String>,
    #[serde(default)]
    pub(crate) headers: Vec<HeaderDocument>,
    #[serde(default)]
    pub(crate) params: Vec<Value>,
    pub(crate) body: Option<Value>,
    pub(crate) auth: Option<Value>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct WebSocketDetailsDocument {
    pub(crate) url: Option<String>,
    #[serde(default)]
    pub(crate) headers: Vec<HeaderDocument>,
    pub(crate) message: Option<Value>,
    pub(crate) auth: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WebSocketMessageDocument {
    #[serde(rename = "type")]
    pub(crate) message_type: String,
    pub(crate) data: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WebSocketMessageVariantDocument {
    pub(crate) title: String,
    #[serde(default)]
    pub(crate) selected: bool,
    pub(crate) message: WebSocketMessageDocument,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GraphqlBodyDocument {
    pub(crate) query: Option<String>,
    pub(crate) variables: Option<Value>,
    pub(crate) operation_name: Option<String>,
    pub(crate) extensions: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct GraphqlBodyVariantDocument {
    pub(crate) title: String,
    #[serde(default)]
    pub(crate) selected: bool,
    pub(crate) body: GraphqlBodyDocument,
}

#[derive(Debug, Deserialize)]
pub(crate) struct HeaderDocument {
    pub(crate) name: String,
    pub(crate) value: String,
    #[serde(default)]
    pub(crate) disabled: bool,
}

impl HeaderDocument {
    pub(crate) fn into_domain(self) -> Header {
        Header {
            name: self.name,
            value: self.value,
            disabled: self.disabled,
        }
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct ParameterDocument {
    pub(crate) name: String,
    pub(crate) value: String,
    #[serde(default)]
    pub(crate) disabled: bool,
}

impl ParameterDocument {
    pub(crate) fn into_domain(self) -> QueryParameter {
        QueryParameter {
            name: self.name,
            value: self.value,
            disabled: self.disabled,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EnvironmentDocument {
    pub(crate) name: String,
    pub(crate) color: Option<String>,
    #[serde(default, deserialize_with = "deserialize_present_value")]
    pub(crate) description: Option<Value>,
    pub(crate) extends: Option<String>,
    pub(crate) dot_env_file_path: Option<String>,
    #[serde(default)]
    pub(crate) variables: Vec<EnvironmentVariableDocument>,
}

fn deserialize_present_value<'de, D>(deserializer: D) -> Result<Option<Value>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Value::deserialize(deserializer).map(Some)
}

impl EnvironmentDocument {
    pub(crate) fn into_domain(self) -> Result<Environment, serde_yaml_ng::Error> {
        Ok(Environment {
            name: self.name,
            color: self.color,
            description: optional_documentation(self.description.as_ref())?,
            extends: self.extends,
            dot_env_file_path: self.dot_env_file_path,
            variables: self
                .variables
                .into_iter()
                .map(EnvironmentVariableDocument::into_domain)
                .collect(),
        })
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct EnvironmentVariableDocument {
    pub(crate) name: Option<String>,
    pub(crate) value: Option<VariableValueSetDocument>,
    #[serde(default)]
    pub(crate) disabled: bool,
    #[serde(default)]
    pub(crate) secret: bool,
    #[serde(rename = "type")]
    pub(crate) value_type: Option<VariableValueTypeDocument>,
}

impl EnvironmentVariableDocument {
    pub(crate) fn into_domain(self) -> EnvironmentVariable {
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
pub(crate) enum VariableValueSetDocument {
    String(String),
    Typed(TypedVariableValueDocument),
    Variants(Vec<VariableValueVariantDocument>),
}

impl VariableValueSetDocument {
    pub(crate) fn into_domain(self) -> VariableValueSet {
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
pub(crate) struct TypedVariableValueDocument {
    #[serde(rename = "type")]
    pub(crate) value_type: VariableValueTypeDocument,
    pub(crate) data: String,
}

impl TypedVariableValueDocument {
    pub(crate) fn into_domain(self) -> VariableValue {
        VariableValue::Typed {
            kind: self.value_type.into_domain(),
            data: self.data,
        }
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct VariableValueVariantDocument {
    pub(crate) title: String,
    #[serde(default)]
    pub(crate) selected: bool,
    pub(crate) value: VariableValueDocument,
}

impl VariableValueVariantDocument {
    pub(crate) fn into_domain(self) -> VariableValueVariant {
        VariableValueVariant {
            title: self.title,
            selected: self.selected,
            value: self.value.into_domain(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum VariableValueDocument {
    String(String),
    Typed(TypedVariableValueDocument),
}

impl VariableValueDocument {
    pub(crate) fn into_domain(self) -> VariableValue {
        match self {
            Self::String(value) => VariableValue::String(value),
            Self::Typed(value) => value.into_domain(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum VariableValueTypeDocument {
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
pub(crate) struct BodyKindDocument {
    #[serde(rename = "type")]
    pub(crate) body_type: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawBodyDocument {
    #[serde(rename = "type")]
    pub(crate) body_type: RawBodyKindDocument,
    pub(crate) data: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum RawBodyKindDocument {
    Json,
    Text,
    Xml,
    Sparql,
}

impl RawBodyKindDocument {
    pub(crate) const fn into_domain(self) -> RawBodyKind {
        match self {
            Self::Json => RawBodyKind::Json,
            Self::Text => RawBodyKind::Text,
            Self::Xml => RawBodyKind::Xml,
            Self::Sparql => RawBodyKind::Sparql,
        }
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct FormBodyDocument {
    pub(crate) data: Vec<FormFieldDocument>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct FormFieldDocument {
    pub(crate) name: String,
    pub(crate) value: String,
    #[serde(default)]
    pub(crate) disabled: bool,
}

impl FormFieldDocument {
    pub(crate) fn into_domain(self) -> FormField {
        FormField {
            name: self.name,
            value: self.value,
            disabled: self.disabled,
        }
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct MultipartBodyDocument {
    pub(crate) data: Vec<MultipartPartDocument>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MultipartPartDocument {
    pub(crate) name: String,
    #[serde(rename = "type")]
    pub(crate) part_type: MultipartPartKindDocument,
    pub(crate) value: MultipartValueDocument,
    pub(crate) content_type: Option<String>,
    #[serde(default)]
    pub(crate) disabled: bool,
}

impl MultipartPartDocument {
    pub(crate) fn into_domain(self) -> MultipartPart {
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
pub(crate) enum MultipartPartKindDocument {
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
pub(crate) enum MultipartValueDocument {
    Single(String),
    Multiple(Vec<String>),
}

impl MultipartValueDocument {
    pub(crate) fn into_domain(self) -> MultipartValue {
        match self {
            Self::Single(value) => MultipartValue::Single(value),
            Self::Multiple(values) => MultipartValue::Multiple(values),
        }
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct FileBodyDocument {
    pub(crate) data: Vec<FileReferenceDocument>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FileReferenceDocument {
    pub(crate) file_path: String,
    pub(crate) content_type: String,
    pub(crate) selected: bool,
}

impl FileReferenceDocument {
    pub(crate) fn into_domain(self) -> FileReference {
        FileReference {
            file_path: self.file_path,
            content_type: self.content_type,
            selected: self.selected,
        }
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct BodyVariantDocument {
    pub(crate) title: String,
    #[serde(default)]
    pub(crate) selected: bool,
    pub(crate) body: Value,
}
