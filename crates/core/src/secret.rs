use std::fmt;

/// Runtime secret material. Formatting uses a redaction marker rather than its contents.
#[derive(Clone, Eq, PartialEq)]
pub struct SecretValue(String);

impl SecretValue {
    /// Wraps a value obtained from a trusted runtime source.
    #[must_use]
    pub fn new(value: String) -> Self {
        Self(value)
    }

    /// Exposes the value only within core request resolution.
    #[must_use]
    pub(crate) fn expose_for_execution(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretValue([REDACTED])")
    }
}

impl fmt::Display for SecretValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}

/// Effective declaration and workspace context for a provider lookup.
#[derive(Clone, Copy, Debug)]
pub struct SecretContext<'a> {
    pub variable_name: &'a str,
    pub environment_name: Option<&'a str>,
    pub workspace_identity: Option<&'a str>,
}

/// A backend failure carries no secret material or backend diagnostic.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SecretError;

/// Supplies values for enabled effective secret declarations.
pub trait SecretProvider {
    fn resolve_secret(
        &self,
        context: &SecretContext<'_>,
    ) -> Result<Option<SecretValue>, SecretError>;
}
