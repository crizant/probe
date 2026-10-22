//! Shared response assertion evaluation.
//!
//! See `docs/ASSERTIONS.md` for the deliberately restricted evaluation language.

use std::{borrow::Cow, cmp::Ordering, fmt, sync::Arc};

use probe_core::{Assertion, SecretProvider, resolve_environment_for_templates_with_provider};
use probe_http::HttpResponse;
use serde_json::{Number, Value};

use crate::RequestResolution;

/// One result per declaration, in input order. Actual values are redacted using
/// the complete execution disclosure before they leave the application boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssertionResult {
    /// Zero-based position in the supplied assertion list.
    pub index: usize,
    pub outcome: AssertionOutcome,
    /// Safe display text, absent for disabled declarations and evaluation errors.
    pub actual: Option<String>,
    /// Whether the redacted display value exceeded the display limit.
    pub actual_truncated: bool,
}

/// Maximum Unicode characters retained before the explicit truncation marker.
pub const MAX_ASSERTION_ACTUAL_CHARS: usize = 512;

// Never Debug/serialize raw observations. They stay inside the execution boundary
// until every assertion has disclosed its secrets (including later declarations).
pub(crate) struct PendingAssertionResult {
    index: usize,
    outcome: AssertionOutcome,
    actual: Option<ObservedActual>,
}

// Share the parsed original body for repeated observations instead of cloning a
// potentially large object for every declaration. No observation escapes unredacted.
enum ObservedActual {
    Status(u16),
    Body {
        root: Arc<Value>,
        expression: String,
    },
}

impl PendingAssertionResult {
    pub(crate) fn redact(self, redactor: &probe_core::SecretRedactor<'_>) -> AssertionResult {
        fn safe_value(value: &Value, redactor: &probe_core::SecretRedactor<'_>) -> String {
            match value {
                Value::String(text) => redactor.redact(text),
                Value::Array(_) | Value::Object(_) => {
                    let original = value.to_string();
                    if redactor.is_empty() {
                        return original;
                    }
                    // An entire structured value may itself contain a secret. Avoid
                    // reconstructing fragments of such a value after replacement.
                    if redactor.redact(&original) != original {
                        return "[REDACTED]".into();
                    }
                    fn decoded(value: &Value, redactor: &probe_core::SecretRedactor<'_>) -> Value {
                        match value {
                            Value::String(text) => Value::String(redactor.redact(text)),
                            Value::Array(values) => {
                                Value::Array(values.iter().map(|v| decoded(v, redactor)).collect())
                            }
                            Value::Object(values) => Value::Object(
                                values
                                    .iter()
                                    .map(|(k, v)| (redactor.redact(k), decoded(v, redactor)))
                                    .collect(),
                            ),
                            _ => {
                                let original = value.to_string();
                                let safe = redactor.redact(&original);
                                if safe == original {
                                    value.clone()
                                } else {
                                    Value::String(safe)
                                }
                            }
                        }
                    }
                    // Redact decoded strings/keys before JSON escaping, so quoted
                    // and multiline secrets cannot evade the unified matcher.
                    decoded(value, redactor).to_string()
                }
                _ => redactor.redact(&value.to_string()),
            }
        }
        let mut actual = self.actual.as_ref().map(|observed| match observed {
            ObservedActual::Status(status) => redactor.redact(&status.to_string()),
            ObservedActual::Body { root, expression } => {
                let path = parse_expression(expression)
                    .expect("observed expression was evaluated")
                    .expect("body expression");
                let value = select(root, &path).expect("observed property was evaluated");
                safe_value(&value, redactor)
            }
        });
        let mut actual_truncated = false;
        if let Some(text) = &mut actual
            && let Some((offset, _)) = text.char_indices().nth(MAX_ASSERTION_ACTUAL_CHARS)
        {
            text.truncate(offset);
            text.push_str("… [truncated]");
            actual_truncated = true;
        }
        AssertionResult {
            index: self.index,
            outcome: self.outcome,
            actual,
            actual_truncated,
        }
    }
}

/// Disabled declarations are retained for stable indexing but never evaluated.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AssertionOutcome {
    Passed,
    Failed,
    Disabled,
    Error(AssertionError),
}

/// Value-free diagnostics, safe to display and debug even with secret operands.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssertionError {
    UnsupportedExpression,
    UnsupportedOperator,
    MissingExpectedValue,
    VariableResolution,
    InvalidExpectedValue,
    BodyUnavailable,
    InvalidJsonBody,
    MissingProperty,
    IncompatibleOperands,
}

impl fmt::Display for AssertionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::UnsupportedExpression => "unsupported assertion expression",
            Self::UnsupportedOperator => "unsupported assertion operator",
            Self::MissingExpectedValue => "operator requires an expected value",
            Self::VariableResolution => {
                "assertion variable resolution failed or a value is unavailable"
            }
            Self::InvalidExpectedValue => "invalid or unsupported expected literal",
            Self::BodyUnavailable => {
                "The complete response body is unavailable for this assertion."
            }
            Self::InvalidJsonBody => "response body is not valid JSON",
            Self::MissingProperty => "response property or array index does not exist",
            Self::IncompatibleOperands => "operator does not support these operand types",
        })
    }
}

impl std::error::Error for AssertionError {}

/// Evaluates HTTP and GraphQL assertions against the same completed transport
/// response, without mutating the declarations, response, or collection.
///
/// Uses the existing environment resolver and provider, scoped to each active
/// binary assertion value that contains `{{` interpolation. Values without a
/// template are not resolved. One resolution error cannot abort later assertions.
/// Missing references always error, even in non-strict request mode: unresolved
/// templates cannot prove an assertion. Request-sending semantics are unchanged.
/// Unary operators ignore their optional value. Providers may block; interactive
/// callers must invoke this off their UI thread. No response spool files are read.
#[must_use]
pub fn evaluate_assertions(
    assertions: &[Assertion],
    response: &HttpResponse,
    resolution: &RequestResolution<'_>,
    secrets: &dyn SecretProvider,
) -> Vec<AssertionResult> {
    let secrets = crate::execution::CachedSecrets::new(secrets);
    let mut environments = Vec::new();
    let pending = evaluate_with_disclosure(assertions, response, resolution, &secrets, |env| {
        environments.push(env)
    });
    let fetched = secrets.redaction_values();
    let redactor = probe_core::SecretRedactor::from_environments(&environments)
        .with_values(&fetched)
        .with_json_literals();
    pending
        .into_iter()
        .map(|result| result.redact(&redactor))
        .collect()
}

/// A metadata-only storage preflight. Never consult the caller's secret provider.
/// A failed resolution is treated conservatively: no original bytes may be spooled.
/// Disabled declarations and unary values cannot resolve secrets during evaluation.
pub(crate) fn may_resolve_secrets(
    assertions: &[Assertion],
    resolution: &RequestResolution<'_>,
) -> bool {
    let templates: Vec<_> = assertions
        .iter()
        .filter(|assertion| {
            assertion.disabled != Some(true)
                && !matches!(assertion.operator.as_str(), "isNull" | "isNotNull")
        })
        .filter_map(|assertion| assertion.value.as_deref())
        .filter(|value| value.contains("{{"))
        .collect();
    if templates.is_empty() {
        return false;
    }
    match resolve_environment_for_templates_with_provider(
        templates.iter().copied(),
        resolution.environments,
        resolution.environment,
        resolution.overrides,
        &crate::NoSecrets,
        resolution.workspace_identity,
    ) {
        Ok(environment) => {
            environment.has_resolved_secrets()
                || templates
                    .iter()
                    .any(|template| environment.interpolate_strict(template).is_err())
        }
        Err(_) => true,
    }
}

pub(crate) fn evaluate_with_disclosure(
    assertions: &[Assertion],
    response: &HttpResponse,
    resolution: &RequestResolution<'_>,
    secrets: &crate::execution::CachedSecrets<'_>,
    mut disclose: impl FnMut(probe_core::ResolvedEnvironment),
) -> Vec<PendingAssertionResult> {
    // Parse only on demand, once, and never use an incomplete preview as JSON.
    let mut body = None;
    assertions
        .iter()
        .enumerate()
        .map(|(index, assertion)| {
            if assertion.disabled == Some(true) {
                PendingAssertionResult {
                    index,
                    outcome: AssertionOutcome::Disabled,
                    actual: None,
                }
            } else {
                match evaluate_one(
                    assertion,
                    response,
                    resolution,
                    secrets,
                    &mut body,
                    &mut disclose,
                ) {
                    Ok((passed, actual)) => PendingAssertionResult {
                        index,
                        outcome: if passed {
                            AssertionOutcome::Passed
                        } else {
                            AssertionOutcome::Failed
                        },
                        actual: Some(actual),
                    },
                    Err(error) => PendingAssertionResult {
                        index,
                        outcome: AssertionOutcome::Error(error),
                        actual: None,
                    },
                }
            }
        })
        .collect()
}

#[derive(Clone, Copy)]
enum Operator {
    Equals,
    NotEquals,
    Contains,
    NotContains,
    GreaterThan,
    LessThan,
    IsNull,
    IsNotNull,
}

impl Operator {
    fn parse(input: &str) -> Result<Self, AssertionError> {
        match input {
            "equals" => Ok(Self::Equals),
            "notEquals" => Ok(Self::NotEquals),
            "contains" => Ok(Self::Contains),
            "notContains" => Ok(Self::NotContains),
            "greaterThan" => Ok(Self::GreaterThan),
            "lessThan" => Ok(Self::LessThan),
            "isNull" => Ok(Self::IsNull),
            "isNotNull" => Ok(Self::IsNotNull),
            _ => Err(AssertionError::UnsupportedOperator),
        }
    }
}

fn evaluate_one(
    assertion: &Assertion,
    response: &HttpResponse,
    resolution: &RequestResolution<'_>,
    secrets: &crate::execution::CachedSecrets<'_>,
    body: &mut Option<Result<Arc<Value>, AssertionError>>,
    disclose: &mut impl FnMut(probe_core::ResolvedEnvironment),
) -> Result<(bool, ObservedActual), AssertionError> {
    let operator = Operator::parse(&assertion.operator)?;
    let path = parse_expression(&assertion.expression)?;
    let root = if path.is_some() {
        Some(Arc::clone(parsed_body(body, response)?))
    } else {
        None
    };
    let observation = || match &root {
        Some(root) => ObservedActual::Body {
            root: Arc::clone(root),
            expression: assertion.expression.clone(),
        },
        None => ObservedActual::Status(response.status),
    };
    let actual = match path.as_deref() {
        None => Cow::Owned(Value::from(response.status)),
        Some(path) => select(root.as_ref().expect("body was parsed"), path)?,
    };
    match operator {
        Operator::IsNull => return Ok((actual.is_null(), observation())),
        Operator::IsNotNull => return Ok((!actual.is_null(), observation())),
        _ => {}
    }
    let template = assertion
        .value
        .as_deref()
        .ok_or(AssertionError::MissingExpectedValue)?;
    let resolved = resolve_expected(template, resolution, secrets, disclose)?;
    if matches!(operator, Operator::Contains | Operator::NotContains) {
        let Value::String(text) = actual.as_ref() else {
            return Err(AssertionError::IncompatibleOperands);
        };
        let needle = substring_needle(resolved.as_ref())?;
        let found = text.contains(needle.as_ref());
        return Ok((
            found == matches!(operator, Operator::Contains),
            observation(),
        ));
    }
    let expected = parse_expected(resolved.as_ref())?;
    let passed = compare(actual.as_ref(), operator, &expected)?;
    Ok((passed, observation()))
}

fn parsed_body<'a>(
    body: &'a mut Option<Result<Arc<Value>, AssertionError>>,
    response: &HttpResponse,
) -> Result<&'a Arc<Value>, AssertionError> {
    if body.is_none() {
        *body = Some(if !response.body_complete {
            Err(AssertionError::BodyUnavailable)
        } else {
            serde_json::from_slice(&response.body)
                .map(Arc::new)
                .map_err(|_| AssertionError::InvalidJsonBody)
        });
    }
    match body.as_ref() {
        Some(Ok(value)) => Ok(value),
        Some(Err(error)) => Err(*error),
        None => unreachable!("response body was just stored"),
    }
}

/// Plain expected values do not consult environments or secret providers.
fn resolve_expected<'a>(
    template: &'a str,
    resolution: &RequestResolution<'_>,
    secrets: &crate::execution::CachedSecrets<'_>,
    disclose: &mut impl FnMut(probe_core::ResolvedEnvironment),
) -> Result<Cow<'a, str>, AssertionError> {
    if !template.contains("{{") {
        return Ok(Cow::Borrowed(template));
    }
    let environment = resolve_environment_for_templates_with_provider(
        [template],
        resolution.environments,
        resolution.environment,
        resolution.overrides,
        secrets,
        resolution.workspace_identity,
    )
    .map_err(|_| AssertionError::VariableResolution)?;
    let result: Result<Cow<'_, str>, AssertionError> = environment
        .interpolate_strict(template)
        .map(Cow::Owned)
        .map_err(|_| AssertionError::VariableResolution);
    if environment.has_resolved_secrets()
        && let Ok(value) = &result
    {
        secrets.record_assertion_disclosure(value.to_string());
    }
    disclose(environment);
    result
}

enum Segment<'a> {
    Property(&'a str),
    Index(usize),
}

// None means status; Some(empty) means the complete JSON body.
fn parse_expression(input: &str) -> Result<Option<Vec<Segment<'_>>>, AssertionError> {
    if input == "response.status" {
        return Ok(None);
    }
    let mut rest = input
        .strip_prefix("response.body")
        .ok_or(AssertionError::UnsupportedExpression)?;
    let mut path = Vec::new();
    while !rest.is_empty() {
        if let Some(property) = rest.strip_prefix('.') {
            let end = property
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '$'))
                .unwrap_or(property.len());
            let name = &property[..end];
            if name.is_empty() || name.as_bytes()[0].is_ascii_digit() {
                return Err(AssertionError::UnsupportedExpression);
            }
            path.push(Segment::Property(name));
            rest = &property[end..];
        } else if let Some(index) = rest.strip_prefix('[') {
            let end = index
                .find(']')
                .ok_or(AssertionError::UnsupportedExpression)?;
            let digits = &index[..end];
            if digits.is_empty()
                || !digits.bytes().all(|b| b.is_ascii_digit())
                || (digits.len() > 1 && digits.starts_with('0'))
            {
                return Err(AssertionError::UnsupportedExpression);
            }
            path.push(Segment::Index(
                digits
                    .parse()
                    .map_err(|_| AssertionError::UnsupportedExpression)?,
            ));
            rest = &index[end + 1..];
        } else {
            return Err(AssertionError::UnsupportedExpression);
        }
    }
    Ok(Some(path))
}

fn select<'a>(body: &'a Value, path: &[Segment<'_>]) -> Result<Cow<'a, Value>, AssertionError> {
    let mut value = body;
    for (position, segment) in path.iter().enumerate() {
        value = match (segment, value) {
            (Segment::Property(name), Value::Object(object)) => {
                object.get(*name).ok_or(AssertionError::MissingProperty)?
            }
            (Segment::Index(index), Value::Array(array)) => {
                array.get(*index).ok_or(AssertionError::MissingProperty)?
            }
            (Segment::Property("length"), Value::Array(array)) if position + 1 == path.len() => {
                return Ok(Cow::Owned(Value::from(array.len())));
            }
            (Segment::Property("length"), Value::String(string)) if position + 1 == path.len() => {
                return Ok(Cow::Owned(Value::from(string.chars().count())));
            }
            _ => return Err(AssertionError::IncompatibleOperands),
        };
    }
    Ok(Cow::Borrowed(value))
}

fn parse_expected(input: &str) -> Result<Value, AssertionError> {
    let literal = input.trim();
    if literal.starts_with('"') {
        return serde_json::from_str::<String>(literal)
            .map(Value::String)
            .map_err(|_| AssertionError::InvalidExpectedValue);
    }
    if matches!(literal, "true" | "false" | "null") {
        return serde_json::from_str(literal).map_err(|_| AssertionError::InvalidExpectedValue);
    }
    if literal.starts_with(|c: char| c.is_ascii_digit() || c == '-' || c == '+') {
        if let Ok(Value::Number(number)) = serde_json::from_str(literal) {
            return Ok(Value::Number(number));
        }
        // Grammar match with a failed parse means the number does not fit,
        // as with `1e400`. Syntax such as `2024-01-01` and `+200` stays text.
        if is_json_number(literal) || matches!(literal, "NaN" | "Infinity" | "-Infinity") {
            return Err(AssertionError::InvalidExpectedValue);
        }
    } else if literal.starts_with(['\'', '[', '{'])
        || matches!(literal, "NaN" | "Infinity" | "-Infinity")
    {
        return Err(AssertionError::InvalidExpectedValue);
    }
    Ok(Value::String(input.to_owned()))
}

/// RFC 8259 number grammar: optional `-`, integer without leading zeroes,
/// optional fraction, optional exponent. This does not apply serde_json's range.
fn is_json_number(literal: &str) -> bool {
    let after_sign = literal.strip_prefix('-').unwrap_or(literal);
    let Some(after_int) = take_json_int(after_sign) else {
        return false;
    };
    let Some(after_frac) = take_json_frac(after_int) else {
        return false;
    };
    let Some(after_exp) = take_json_exp(after_frac) else {
        return false;
    };
    after_exp.is_empty()
}

fn take_json_int(input: &str) -> Option<&str> {
    match input.as_bytes().first() {
        Some(b'0') => Some(&input[1..]),
        Some(b'1'..=b'9') => take_digits(input),
        _ => None,
    }
}

fn take_json_frac(input: &str) -> Option<&str> {
    let Some(after_dot) = input.strip_prefix('.') else {
        return Some(input);
    };
    take_digits(after_dot)
}

fn take_json_exp(input: &str) -> Option<&str> {
    let Some(after_mark) = input.strip_prefix(['e', 'E']) else {
        return Some(input);
    };
    let after_sign = after_mark.strip_prefix(['+', '-']).unwrap_or(after_mark);
    take_digits(after_sign)
}

fn take_digits(input: &str) -> Option<&str> {
    let len = input.bytes().take_while(u8::is_ascii_digit).count();
    if len == 0 { None } else { Some(&input[len..]) }
}

/// Needle for `contains` when the actual value is already a string.
///
/// Numbers, booleans, and null keep the literal's spelling (`1e3`, not `1000`).
/// Quoted JSON strings contribute their decoded text.
fn substring_needle(resolved: &str) -> Result<Cow<'_, str>, AssertionError> {
    match parse_expected(resolved)? {
        Value::String(text) => Ok(Cow::Owned(text)),
        Value::Number(_) | Value::Bool(_) | Value::Null => Ok(Cow::Borrowed(resolved.trim())),
        _ => Err(AssertionError::InvalidExpectedValue),
    }
}

fn compare(actual: &Value, operator: Operator, expected: &Value) -> Result<bool, AssertionError> {
    match operator {
        Operator::Equals | Operator::NotEquals => {
            let equal = match (actual, expected) {
                (Value::Number(a), Value::Number(b)) => number_cmp(a, b) == Ordering::Equal,
                _ => actual == expected,
            };
            Ok(equal == matches!(operator, Operator::Equals))
        }
        Operator::GreaterThan | Operator::LessThan => {
            let (Value::Number(actual), Value::Number(expected)) = (actual, expected) else {
                return Err(AssertionError::IncompatibleOperands);
            };
            let wanted = if matches!(operator, Operator::GreaterThan) {
                Ordering::Greater
            } else {
                Ordering::Less
            };
            Ok(number_cmp(actual, expected) == wanted)
        }
        Operator::Contains | Operator::NotContains | Operator::IsNull | Operator::IsNotNull => {
            unreachable!("handled before typed comparison")
        }
    }
}

// Compare decimal representations rather than casting u64/i64 to f64 (which
// would make distinct large integers compare equal). JSON floats retain
// serde_json's finite f64 precision; exponent spelling is normalized here.
fn number_cmp(a: &Number, b: &Number) -> Ordering {
    fn decimal(number: &Number) -> (bool, i32, Vec<u8>) {
        let text = number.to_string();
        let negative = text.starts_with('-');
        let text = text.trim_start_matches('-');
        let (mantissa, exponent) = text.split_once(['e', 'E']).map_or((text, 0), |(m, e)| {
            (m, e.parse::<i32>().expect("JSON number exponent"))
        });
        let fraction = mantissa.split_once('.').map_or(0, |(_, f)| f.len() as i32);
        let digits: Vec<_> = mantissa
            .bytes()
            .filter(|b| *b != b'.')
            .skip_while(|b| *b == b'0')
            .collect();
        if digits.is_empty() {
            return (false, 0, vec![b'0']);
        }
        (negative, digits.len() as i32 + exponent - fraction, digits)
    }
    let (an, ae, mut ad) = decimal(a);
    let (bn, be, mut bd) = decimal(b);
    if an != bn {
        return bn.cmp(&an);
    }
    // Zero is smaller than every positive nonzero decimal, regardless of scale.
    let magnitude = if ad.as_slice() == b"0" || bd.as_slice() == b"0" {
        (bd.as_slice() == b"0").cmp(&(ad.as_slice() == b"0"))
    } else {
        let width = ad.len().max(bd.len());
        ad.resize(width, b'0');
        bd.resize(width, b'0');
        ae.cmp(&be).then_with(|| ad.cmp(&bd))
    };
    if an { magnitude.reverse() } else { magnitude }
}

#[cfg(test)]
mod tests;
