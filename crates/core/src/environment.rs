use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
};

use crate::{
    Environment, EnvironmentVariable, SecretContext, SecretProvider, SecretValue, Variable,
    VariableValue, VariableValueSet,
};

/// Whether a `{{name}}` reference would be substituted in a resolved environment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VariableStatus {
    /// The name has a resolved value, including an explicit empty string.
    Resolved,
    /// The name is an enabled secret with no runtime value.
    SecretWithoutValue,
    /// The name is absent or disabled.
    Missing,
}

impl VariableStatus {
    /// Returns whether this name would be substituted.
    #[must_use]
    pub const fn is_resolved(self) -> bool {
        matches!(self, Self::Resolved)
    }
}

/// Classifies `name` the same way [`ResolvedEnvironment::variable_status`] does.
///
/// Secrets without runtime values are checked first, matching interpolation.
/// An explicit empty string is [`VariableStatus::Resolved`].
#[must_use]
pub fn variable_status(
    variables: &BTreeMap<String, String>,
    secrets_without_values: &BTreeSet<String>,
    name: &str,
) -> VariableStatus {
    if secrets_without_values.contains(name) {
        VariableStatus::SecretWithoutValue
    } else if variables.contains_key(name) {
        VariableStatus::Resolved
    } else {
        VariableStatus::Missing
    }
}

/// An environment selected and resolved entirely in memory.
/// Its debug view identifies the environment and secret names without secret values.
#[derive(Clone, Eq, PartialEq)]
pub struct ResolvedEnvironment {
    name: String,
    variables: BTreeMap<String, String>,
    secrets_without_values: BTreeSet<String>,
    secrets: BTreeMap<String, SecretValue>,
    provider_failures: BTreeSet<String>,
    deferred_errors: BTreeMap<String, EnvironmentResolutionError>,
}

impl fmt::Debug for ResolvedEnvironment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResolvedEnvironment")
            .field("name", &self.name)
            .field("variables", &self.variables)
            .field("secrets_without_values", &self.secrets_without_values)
            .field("secret_names", &self.secrets.keys().collect::<Vec<_>>())
            .field("provider_failure_names", &self.provider_failures)
            .field(
                "deferred_error_names",
                &self.deferred_errors.keys().collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl ResolvedEnvironment {
    /// Returns the selected environment name, or an empty string when only runtime
    /// overrides were resolved.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns all resolved, non-secret variables in deterministic name order.
    #[must_use]
    pub const fn variables(&self) -> &BTreeMap<String, String> {
        &self.variables
    }

    /// Returns secret variable names that have no runtime value.
    #[must_use]
    pub const fn secrets_without_values(&self) -> &BTreeSet<String> {
        &self.secrets_without_values
    }

    /// Reports whether `name` would be substituted from this environment.
    #[must_use]
    pub fn variable_status(&self, name: &str) -> VariableStatus {
        if self.secrets.contains_key(name) {
            VariableStatus::Resolved
        } else if self.deferred_errors.contains_key(name) {
            VariableStatus::SecretWithoutValue
        } else {
            variable_status(&self.variables, &self.secrets_without_values, name)
        }
    }

    /// Looks up a resolved variable.
    #[must_use]
    pub fn variable(&self, name: &str) -> Option<&str> {
        self.variables.get(name).map(String::as_str)
    }

    /// Interpolates `{{variable}}` references in a request value.
    ///
    /// References with no usable plain-variable value are preserved literally so
    /// callers can intentionally send template syntax. Unavailable secrets still
    /// produce an error rather than being sent in place of their value.
    pub fn interpolate(&self, input: &str) -> Result<String, EnvironmentResolutionError> {
        interpolate(input, |name| {
            if let Some(error) = self.interpolation_error(name) {
                Err(error)
            } else if self.secrets_without_values.contains(name) {
                Err(EnvironmentResolutionError::SecretVariableUnavailable(
                    name.to_owned(),
                ))
            } else {
                Ok(self
                    .secrets
                    .get(name)
                    .map(|value| value.expose_for_execution().to_owned())
                    .or_else(|| self.variables.get(name).cloned()))
            }
        })
    }

    /// Interpolates `{{variable}}` references and rejects any unavailable value.
    pub fn interpolate_strict(&self, input: &str) -> Result<String, EnvironmentResolutionError> {
        interpolate(input, |name| {
            if let Some(error) = self.interpolation_error(name) {
                Err(error)
            } else if self.secrets_without_values.contains(name) {
                Err(EnvironmentResolutionError::SecretVariableUnavailable(
                    name.to_owned(),
                ))
            } else {
                self.secrets
                    .get(name)
                    .map(|value| value.expose_for_execution().to_owned())
                    .or_else(|| self.variables.get(name).cloned())
                    .map(Some)
                    .ok_or_else(|| EnvironmentResolutionError::MissingVariable(name.to_owned()))
            }
        })
    }

    /// Interpolates plain values and retains secret references for presentation.
    pub fn interpolate_for_presentation(
        &self,
        input: &str,
        strict: bool,
    ) -> Result<String, EnvironmentResolutionError> {
        interpolate(input, |name| {
            if let Some(error) = self.interpolation_error(name) {
                Err(error)
            } else if self.secrets_without_values.contains(name) {
                Err(EnvironmentResolutionError::SecretVariableUnavailable(
                    name.to_owned(),
                ))
            } else if self.secrets.contains_key(name) {
                Ok(Some(format!("{{{{{name}}}}}")))
            } else if let Some(value) = self.variables.get(name) {
                Ok(Some(value.clone()))
            } else if strict {
                Err(EnvironmentResolutionError::MissingVariable(name.to_owned()))
            } else {
                Ok(None)
            }
        })
    }

    fn interpolation_error(&self, name: &str) -> Option<EnvironmentResolutionError> {
        if self.provider_failures.contains(name) {
            Some(EnvironmentResolutionError::SecretProviderFailure(
                name.to_owned(),
            ))
        } else {
            self.deferred_errors.get(name).cloned()
        }
    }

    /// Reports whether runtime secret material was resolved.
    #[must_use]
    pub fn has_resolved_secrets(&self) -> bool {
        !self.secrets.is_empty()
    }

    /// Redacts exact secret values without exposing them to callers.
    #[must_use]
    pub fn redact_secrets(&self, input: &str) -> String {
        let mut values = self.secrets.values().collect::<Vec<_>>();
        values.sort_by_key(|value| std::cmp::Reverse(value.expose_for_execution().len()));
        let mut redacted = input.to_owned();
        for secret in values {
            let value = secret.expose_for_execution();
            if !value.is_empty() {
                redacted = redacted.replace(value, "[REDACTED]");
            }
        }
        redacted
    }
}

/// A deterministic environment-selection or interpolation failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EnvironmentResolutionError {
    /// The selected environment does not exist.
    EnvironmentNotFound(String),
    /// Two environment documents use the same name.
    DuplicateEnvironment(String),
    /// An environment extends a parent that does not exist.
    ParentEnvironmentNotFound {
        /// Child environment name.
        environment: String,
        /// Missing parent environment name.
        parent: String,
    },
    /// Environment inheritance contains a cycle.
    EnvironmentInheritanceCycle(Vec<String>),
    /// A referenced variable is absent, disabled, or has no value.
    MissingVariable(String),
    /// A referenced secret has no runtime value provider.
    SecretVariableUnavailable(String),
    /// The configured provider failed without disclosing its diagnostic.
    SecretProviderFailure(String),
    /// A variable value has no selected variant.
    NoSelectedVariant {
        /// Environment containing the variable.
        environment: String,
        /// Variable name.
        variable: String,
    },
    /// A variable value has more than one selected variant.
    MultipleSelectedVariants {
        /// Environment containing the variable.
        environment: String,
        /// Variable name.
        variable: String,
    },
    /// Variable values refer to each other cyclically.
    VariableInterpolationCycle(Vec<String>),
    /// An interpolation starts with `{{` but has no valid closing expression.
    MalformedInterpolation,
    /// An environment name is empty or otherwise unusable for creation.
    InvalidEnvironmentName,
    /// A variable name is empty or otherwise unusable for set/unset.
    InvalidVariableName,
    /// The named environment has no entry for this variable.
    VariableNotFound {
        /// Environment that was asked to unset the variable.
        environment: String,
        /// Variable name.
        variable: String,
    },
    /// Two variables in the same environment share a name, including across plain and secret kinds.
    DuplicateVariable {
        /// Environment containing the colliding names.
        environment: String,
        /// Variable name that appears more than once.
        variable: String,
    },
    /// The environment is used as another environment's parent.
    EnvironmentInUse(String),
}

/// A plain environment variable as it appears after inheritance, together with its source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EffectiveEnvironmentVariable {
    /// Plain variable definition that currently wins for this name.
    pub variable: Variable,
    /// Environment that defines this effective value.
    pub defined_in: String,
    /// Index in the selected environment's variable list when this entry is local.
    pub direct_index: Option<usize>,
}

/// Validates environment identity and inheritance independently of variable use.
///
/// This is suitable for workspace validation because it does not require selecting an
/// environment or resolving variables that a request may never reference.
pub fn validate_environments(
    environments: &[Environment],
) -> Result<(), EnvironmentResolutionError> {
    EnvironmentIndex::new(environments).map(|_| ())
}

struct EnvironmentIndex<'a> {
    by_name: BTreeMap<&'a str, &'a Environment>,
}

impl<'a> EnvironmentIndex<'a> {
    fn new(environments: &'a [Environment]) -> Result<Self, EnvironmentResolutionError> {
        let mut by_name = BTreeMap::new();
        for environment in environments {
            if by_name
                .insert(environment.name.as_str(), environment)
                .is_some()
            {
                return Err(EnvironmentResolutionError::DuplicateEnvironment(
                    environment.name.clone(),
                ));
            }
        }

        let mut validated = BTreeSet::new();
        for environment in environments {
            validate_environment_inheritance(
                &environment.name,
                &by_name,
                &mut Vec::new(),
                &mut validated,
            )?;
        }
        Ok(Self { by_name })
    }

    fn get(&self, name: &str) -> Option<&'a Environment> {
        self.by_name.get(name).copied()
    }

    fn inheritance_chain(
        &self,
        selected: &str,
    ) -> Result<Vec<&'a Environment>, EnvironmentResolutionError> {
        let mut environment = self
            .get(selected)
            .ok_or_else(|| EnvironmentResolutionError::EnvironmentNotFound(selected.to_owned()))?;
        let mut chain = Vec::new();
        loop {
            chain.push(environment);
            let Some(parent) = environment.extends.as_deref() else {
                break;
            };
            environment = self
                .get(parent)
                .expect("validated parent environment must exist");
        }
        chain.reverse();
        Ok(chain)
    }
}

fn validate_environment_inheritance<'a>(
    name: &'a str,
    environments: &BTreeMap<&'a str, &'a Environment>,
    stack: &mut Vec<String>,
    validated: &mut BTreeSet<String>,
) -> Result<(), EnvironmentResolutionError> {
    if validated.contains(name) {
        return Ok(());
    }
    if let Some(position) = stack.iter().position(|item| item == name) {
        let mut cycle = stack[position..].to_vec();
        cycle.push(name.to_owned());
        return Err(EnvironmentResolutionError::EnvironmentInheritanceCycle(
            cycle,
        ));
    }

    let environment = environments
        .get(name)
        .expect("environment name must exist during validation");
    stack.push(name.to_owned());
    if let Some(parent) = environment.extends.as_deref() {
        if !environments.contains_key(parent) {
            return Err(EnvironmentResolutionError::ParentEnvironmentNotFound {
                environment: environment.name.clone(),
                parent: parent.to_owned(),
            });
        }
        validate_environment_inheritance(parent, environments, stack, validated)?;
    }
    stack.pop();
    validated.insert(name.to_owned());
    Ok(())
}

impl fmt::Display for EnvironmentResolutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EnvironmentNotFound(name) => write!(formatter, "environment not found: {name}"),
            Self::DuplicateEnvironment(name) => {
                write!(formatter, "environment name is duplicated: {name}")
            }
            Self::ParentEnvironmentNotFound {
                environment,
                parent,
            } => write!(
                formatter,
                "environment '{environment}' extends missing environment '{parent}'"
            ),
            Self::EnvironmentInheritanceCycle(names) => write!(
                formatter,
                "environment inheritance cycle: {}",
                names.join(" -> ")
            ),
            Self::MissingVariable(name) => write!(formatter, "variable not found: {name}"),
            Self::SecretVariableUnavailable(name) => {
                write!(formatter, "secret variable has no runtime value: {name}")
            }
            Self::SecretProviderFailure(name) => {
                write!(formatter, "secret provider failed for variable: {name}")
            }
            Self::NoSelectedVariant {
                environment,
                variable,
            } => write!(
                formatter,
                "variable '{variable}' in environment '{environment}' has no selected variant"
            ),
            Self::MultipleSelectedVariants {
                environment,
                variable,
            } => write!(
                formatter,
                "variable '{variable}' in environment '{environment}' has multiple selected variants"
            ),
            Self::VariableInterpolationCycle(names) => {
                write!(
                    formatter,
                    "variable interpolation cycle: {}",
                    names.join(" -> ")
                )
            }
            Self::MalformedInterpolation => write!(formatter, "malformed variable interpolation"),
            Self::InvalidEnvironmentName => {
                formatter.write_str("environment name must not be empty")
            }
            Self::InvalidVariableName => formatter.write_str("variable name must not be empty"),
            Self::VariableNotFound {
                environment,
                variable,
            } => write!(
                formatter,
                "variable '{variable}' is not defined on environment '{environment}'"
            ),
            Self::DuplicateVariable {
                environment,
                variable,
            } => write!(
                formatter,
                "environment '{environment}' has duplicate variable '{variable}'"
            ),
            Self::EnvironmentInUse(name) => write!(
                formatter,
                "environment '{name}' is extended by another environment"
            ),
        }
    }
}

impl Error for EnvironmentResolutionError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum RawVariable {
    Value(String),
    Unavailable,
    Secret,
}

#[derive(Clone, Copy)]
pub(crate) enum EffectiveVariableDeclaration {
    Plain,
    Secret,
    Disabled,
}

pub(crate) fn effective_variable_declarations(
    environments: &[Environment],
    selected: Option<&str>,
) -> Result<BTreeMap<String, EffectiveVariableDeclaration>, EnvironmentResolutionError> {
    let Some(selected) = selected else {
        return Ok(BTreeMap::new());
    };
    let index = EnvironmentIndex::new(environments)?;
    let mut declarations = BTreeMap::new();
    for environment in index.inheritance_chain(selected)? {
        for variable in &environment.variables {
            let (name, declaration) = match variable {
                EnvironmentVariable::Plain(variable) => (
                    variable.name.as_ref(),
                    if variable.disabled {
                        EffectiveVariableDeclaration::Disabled
                    } else {
                        EffectiveVariableDeclaration::Plain
                    },
                ),
                EnvironmentVariable::Secret(variable) => (
                    variable.name.as_ref(),
                    if variable.disabled {
                        EffectiveVariableDeclaration::Disabled
                    } else {
                        EffectiveVariableDeclaration::Secret
                    },
                ),
            };
            if let Some(name) = name {
                declarations.insert(name.clone(), declaration);
            }
        }
    }
    Ok(declarations)
}

/// Selects an environment, applies its inheritance chain, and resolves variable values.
pub fn resolve_environment(
    environments: &[Environment],
    selected: &str,
) -> Result<ResolvedEnvironment, EnvironmentResolutionError> {
    resolve_environment_with_overrides(environments, Some(selected), &[])
}

/// Resolves an optional selected environment after applying invocation-only overrides.
///
/// Overrides are applied after inheritance and before variable interpolation. When a name
/// occurs more than once, the last value wins. Passing `None` for `selected` resolves only
/// the supplied runtime variables without selecting or modifying a persisted environment.
pub fn resolve_environment_with_overrides(
    environments: &[Environment],
    selected: Option<&str>,
    overrides: &[(String, String)],
) -> Result<ResolvedEnvironment, EnvironmentResolutionError> {
    resolve_environment_internal(environments, selected, overrides, None, None)
}

/// Resolves effective secrets through a runtime provider; overrides to declared secrets
/// are kept as secret material. The provider is never asked for plain declarations.
pub fn resolve_environment_with_provider(
    environments: &[Environment],
    selected: Option<&str>,
    overrides: &[(String, String)],
    provider: &dyn SecretProvider,
    workspace_identity: Option<&str>,
) -> Result<ResolvedEnvironment, EnvironmentResolutionError> {
    resolve_environment_internal(
        environments,
        selected,
        overrides,
        Some(provider),
        workspace_identity,
    )
}

fn resolve_environment_internal(
    environments: &[Environment],
    selected: Option<&str>,
    overrides: &[(String, String)],
    provider: Option<&dyn SecretProvider>,
    workspace_identity: Option<&str>,
) -> Result<ResolvedEnvironment, EnvironmentResolutionError> {
    let mut raw = if let Some(selected) = selected {
        raw_variables(environments, selected)?
    } else {
        EnvironmentIndex::new(environments)?;
        BTreeMap::new()
    };
    let mut secrets = BTreeMap::new();
    let mut provider_failures = BTreeSet::new();
    let mut overridden_secrets = BTreeSet::new();
    for (name, value) in overrides {
        if name.is_empty() {
            return Err(EnvironmentResolutionError::InvalidVariableName);
        }
        if matches!(raw.get(name), Some(RawVariable::Secret)) {
            secrets.insert(name.clone(), SecretValue::new(value.clone()));
            overridden_secrets.insert(name.clone());
        } else {
            raw.insert(name.clone(), RawVariable::Value(value.clone()));
        }
    }

    if let Some(provider) = provider {
        for (name, declaration) in &raw {
            if !matches!(declaration, RawVariable::Secret) || overridden_secrets.contains(name) {
                continue;
            }
            let context = SecretContext {
                variable_name: name,
                environment_name: selected,
                workspace_identity,
            };
            match provider.resolve_secret(&context) {
                Ok(Some(value)) => {
                    secrets.insert(name.clone(), value);
                }
                Ok(None) => {}
                Err(_) => {
                    provider_failures.insert(name.clone());
                }
            }
        }
    }

    // Resolve plain values against opaque secret material, then classify values
    // derived from secrets before publishing the public variable map.
    let mut variables = BTreeMap::new();
    let mut deferred_errors = BTreeMap::new();
    for name in raw.keys() {
        if matches!(raw.get(name), Some(RawVariable::Value(_))) {
            let result = resolve_variable(
                name,
                &raw,
                &secrets,
                &provider_failures,
                &mut variables,
                &mut Vec::new(),
            );
            match result {
                Ok(_) => {}
                Err(
                    error @ (EnvironmentResolutionError::SecretProviderFailure(_)
                    | EnvironmentResolutionError::SecretVariableUnavailable(_)),
                ) => {
                    deferred_errors.insert(name.clone(), error);
                }
                Err(error) => return Err(error),
            }
        }
    }
    // A plain value interpolating a secret is secret-derived and cannot enter the
    // public variable map. Each changing pass taints at least one raw variable,
    // so the number of raw variables bounds propagation. Reuse resolved strings,
    // then move them to the secret map.
    let mut tainted = secrets.keys().cloned().collect::<BTreeSet<_>>();
    for _ in 0..raw.len() {
        let mut changed = false;
        for (name, declaration) in &raw {
            if tainted.contains(name) {
                continue;
            }
            if let RawVariable::Value(value) = declaration {
                let mut depends = false;
                interpolate(value, |reference| {
                    depends |= tainted.contains(reference);
                    Ok(None)
                })?;
                if depends {
                    tainted.insert(name.clone());
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    for name in tainted {
        if let Some(value) = variables.remove(&name) {
            secrets.insert(name, SecretValue::new(value));
        }
    }

    let secrets_without_values = raw
        .iter()
        .filter_map(|(name, value)| {
            (matches!(value, RawVariable::Secret) && !secrets.contains_key(name))
                .then_some(name.clone())
        })
        .collect();
    Ok(ResolvedEnvironment {
        name: selected.unwrap_or_default().to_owned(),
        variables,
        secrets_without_values,
        secrets,
        provider_failures,
        deferred_errors,
    })
}

pub(crate) fn raw_variables(
    environments: &[Environment],
    selected: &str,
) -> Result<BTreeMap<String, RawVariable>, EnvironmentResolutionError> {
    let index = EnvironmentIndex::new(environments)?;
    let mut raw = BTreeMap::new();
    for environment in index.inheritance_chain(selected)? {
        for variable in &environment.variables {
            match variable {
                EnvironmentVariable::Plain(variable) => {
                    let Some(name) = variable.name.as_ref() else {
                        continue;
                    };
                    let value = match (&variable.value, variable.disabled) {
                        (_, true) | (None, false) => RawVariable::Unavailable,
                        (Some(value), false) => {
                            RawVariable::Value(select_value(value, &environment.name, name)?)
                        }
                    };
                    raw.insert(name.clone(), value);
                }
                EnvironmentVariable::Secret(variable) => {
                    let Some(name) = variable.name.as_ref() else {
                        continue;
                    };
                    raw.insert(
                        name.clone(),
                        if variable.disabled {
                            RawVariable::Unavailable
                        } else {
                            RawVariable::Secret
                        },
                    );
                }
            }
        }
    }
    Ok(raw)
}

fn select_value(
    value: &VariableValueSet,
    environment: &str,
    variable: &str,
) -> Result<String, EnvironmentResolutionError> {
    let value = match value {
        VariableValueSet::Single(value) => value,
        VariableValueSet::Variants(variants) => {
            let mut selected = variants.iter().filter(|variant| variant.selected);
            let value =
                selected
                    .next()
                    .ok_or_else(|| EnvironmentResolutionError::NoSelectedVariant {
                        environment: environment.to_owned(),
                        variable: variable.to_owned(),
                    })?;
            if selected.next().is_some() {
                return Err(EnvironmentResolutionError::MultipleSelectedVariants {
                    environment: environment.to_owned(),
                    variable: variable.to_owned(),
                });
            }
            &value.value
        }
    };
    Ok(match value {
        VariableValue::String(value) | VariableValue::Typed { data: value, .. } => value.clone(),
    })
}

fn resolve_variable(
    name: &str,
    raw: &BTreeMap<String, RawVariable>,
    secrets: &BTreeMap<String, SecretValue>,
    provider_failures: &BTreeSet<String>,
    resolved: &mut BTreeMap<String, String>,
    stack: &mut Vec<String>,
) -> Result<String, EnvironmentResolutionError> {
    if let Some(value) = resolved.get(name) {
        return Ok(value.clone());
    }
    if let Some(position) = stack.iter().position(|item| item == name) {
        let mut cycle = stack[position..].to_vec();
        cycle.push(name.to_owned());
        return Err(EnvironmentResolutionError::VariableInterpolationCycle(
            cycle,
        ));
    }

    let value = match raw.get(name) {
        Some(RawVariable::Value(value)) => value,
        Some(RawVariable::Secret) => {
            if provider_failures.contains(name) {
                return Err(EnvironmentResolutionError::SecretProviderFailure(
                    name.to_owned(),
                ));
            }
            return secrets
                .get(name)
                .map(|value| value.expose_for_execution().to_owned())
                .ok_or_else(|| {
                    EnvironmentResolutionError::SecretVariableUnavailable(name.to_owned())
                });
        }
        Some(RawVariable::Unavailable) | None => {
            return Err(EnvironmentResolutionError::MissingVariable(name.to_owned()));
        }
    };
    stack.push(name.to_owned());
    let value = interpolate(value, |reference| {
        resolve_variable(reference, raw, secrets, provider_failures, resolved, stack).map(Some)
    })?;
    stack.pop();
    resolved.insert(name.to_owned(), value.clone());
    Ok(value)
}

pub(crate) fn interpolate<F>(
    input: &str,
    mut lookup: F,
) -> Result<String, EnvironmentResolutionError>
where
    F: FnMut(&str) -> Result<Option<String>, EnvironmentResolutionError>,
{
    let mut output = String::with_capacity(input.len());
    let mut remaining = input;
    while let Some(start) = remaining.find("{{") {
        output.push_str(&remaining[..start]);
        let expression = &remaining[start + 2..];
        let Some(end) = expression.find("}}") else {
            return Err(EnvironmentResolutionError::MalformedInterpolation);
        };
        let name = expression[..end].trim();
        if name.is_empty() || name.contains("{{") {
            return Err(EnvironmentResolutionError::MalformedInterpolation);
        }
        match lookup(name)? {
            Some(value) => output.push_str(&value),
            None => output.push_str(&remaining[start..start + 2 + end + 2]),
        }
        remaining = &expression[end + 2..];
    }
    output.push_str(remaining);
    Ok(output)
}
