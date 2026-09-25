use std::time::Duration;

use probe_core::{
    Author, CollectionMetadata, Environment, EnvironmentVariable, FileReference, FormField, Header,
    ItemMetadata, MultipartPart, MultipartPartKind, MultipartValue, QueryParameter, RawBodyKind,
    RequestSettings, SecretVariable, Variable, VariableValue, VariableValueSet, VariableValueType,
    VariableValueVariant,
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
        }
    }
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct ItemDocument {
    #[serde(default)]
    pub(crate) info: ItemInfoDocument,
    #[serde(default)]
    pub(crate) items: Vec<Value>,
    pub(crate) http: Option<HttpDetailsDocument>,
    pub(crate) graphql: Option<GraphqlDetailsDocument>,
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
}

impl RequestSettingsDocument {
    pub(crate) fn into_domain(self) -> Result<RequestSettings, serde_yaml_ng::Error> {
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
    pub(crate) extends: Option<String>,
    pub(crate) dot_env_file_path: Option<String>,
    #[serde(default)]
    pub(crate) variables: Vec<EnvironmentVariableDocument>,
}

impl EnvironmentDocument {
    pub(crate) fn into_domain(self) -> Environment {
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
