//! Standalone response assertions; never called by request preparation or execution.
//!
//! See `docs/ASSERTIONS.md` for the deliberately restricted evaluation language.

use std::{cmp::Ordering, fmt};

use probe_core::{Assertion, SecretProvider, resolve_environment_for_templates_with_provider};
use probe_http::HttpResponse;
use serde_json::{Number, Value};

use crate::RequestResolution;

/// One result per declaration, in input order. No declaration or runtime values
/// are copied into results: even response properties may contain echoed secrets.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssertionResult {
    /// Zero-based position in the supplied assertion list.
    pub index: usize,
    pub outcome: AssertionOutcome,
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
            Self::BodyUnavailable => "complete in-memory response body is unavailable",
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
/// binary assertion's value. One resolution error cannot abort later assertions.
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
    // Parse only on demand, once, and never use an incomplete preview as JSON.
    let mut body = None;
    assertions
        .iter()
        .enumerate()
        .map(|(index, assertion)| AssertionResult {
            index,
            outcome: if assertion.disabled == Some(true) {
                AssertionOutcome::Disabled
            } else {
                match evaluate_one(assertion, response, resolution, secrets, &mut body) {
                    Ok(true) => AssertionOutcome::Passed,
                    Ok(false) => AssertionOutcome::Failed,
                    Err(error) => AssertionOutcome::Error(error),
                }
            },
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
    secrets: &dyn SecretProvider,
    body: &mut Option<Result<Value, AssertionError>>,
) -> Result<bool, AssertionError> {
    let operator = Operator::parse(&assertion.operator)?;
    let path = parse_expression(&assertion.expression)?;
    let actual = match path {
        None => Value::from(response.status),
        Some(path) => {
            let body = body.get_or_insert_with(|| {
                if !response.body_complete {
                    Err(AssertionError::BodyUnavailable)
                } else {
                    serde_json::from_slice(&response.body)
                        .map_err(|_| AssertionError::InvalidJsonBody)
                }
            });
            select(body.as_ref().map_err(|error| *error)?, &path)?
        }
    };
    match operator {
        Operator::IsNull => return Ok(actual.is_null()),
        Operator::IsNotNull => return Ok(!actual.is_null()),
        _ => {}
    }
    let template = assertion
        .value
        .as_deref()
        .ok_or(AssertionError::MissingExpectedValue)?;
    let environment = resolve_environment_for_templates_with_provider(
        [template],
        resolution.environments,
        resolution.environment,
        resolution.overrides,
        secrets,
        resolution.workspace_identity,
    )
    .map_err(|_| AssertionError::VariableResolution)?;
    let resolved = environment
        .interpolate_strict(template)
        .map_err(|_| AssertionError::VariableResolution)?;
    let expected = parse_expected(&resolved)?;
    compare(&actual, operator, &expected)
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

fn select(body: &Value, path: &[Segment<'_>]) -> Result<Value, AssertionError> {
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
                return Ok(Value::from(array.len()));
            }
            (Segment::Property("length"), Value::String(string)) if position + 1 == path.len() => {
                return Ok(Value::from(string.chars().count()));
            }
            _ => return Err(AssertionError::IncompatibleOperands),
        };
    }
    Ok(value.clone())
}

fn parse_expected(input: &str) -> Result<Value, AssertionError> {
    let literal = input.trim();
    if literal.starts_with('"') {
        return serde_json::from_str::<String>(literal)
            .map(Value::String)
            .map_err(|_| AssertionError::InvalidExpectedValue);
    }
    if matches!(literal, "true" | "false" | "null")
        || literal.starts_with(|c: char| c.is_ascii_digit() || c == '-' || c == '+')
    {
        let value: Value =
            serde_json::from_str(literal).map_err(|_| AssertionError::InvalidExpectedValue)?;
        return match value {
            Value::Number(_) | Value::Bool(_) | Value::Null => Ok(value),
            _ => Err(AssertionError::InvalidExpectedValue),
        };
    }
    if literal.starts_with(['\'', '[', '{']) || matches!(literal, "NaN" | "Infinity" | "-Infinity")
    {
        return Err(AssertionError::InvalidExpectedValue);
    }
    Ok(Value::String(input.to_owned()))
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
        Operator::Contains | Operator::NotContains => {
            let (Value::String(actual), Value::String(expected)) = (actual, expected) else {
                return Err(AssertionError::IncompatibleOperands);
            };
            Ok(actual.contains(expected) == matches!(operator, Operator::Contains))
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
        Operator::IsNull | Operator::IsNotNull => {
            unreachable!("unary operators handled before resolution")
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
