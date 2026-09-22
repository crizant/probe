//! JWT and Unix timestamp classification.

use chrono::{DateTime, Local, TimeDelta, TimeZone, Utc};

use super::{JwtClaim, JwtFinding, TimestampDisplay, TimestampFinding};

const JWT_STANDARD_CLAIMS: &[&str] = &["exp", "iat", "nbf", "iss", "sub"];

pub(super) fn inspect_jwt(value: &serde_json::Value, path: &str) -> Option<JwtFinding> {
    inspect_jwt_text(value.as_str()?, path)
}

pub(super) fn inspect_jwt_text(token: &str, path: &str) -> Option<JwtFinding> {
    if token.matches('.').count() != 2 {
        return None;
    }
    let mut parts = token.split('.');
    let header = decode_base64url_json_object(parts.next()?)?;
    let payload = decode_base64url_json_object(parts.next()?)?;
    let signature = parts.next()?;
    if !base64url_candidate(signature) {
        return None;
    }

    let mut confidence = 0;
    if header.get("alg").is_some() {
        confidence += 2;
    }
    if header.get("typ").and_then(serde_json::Value::as_str) == Some("JWT") {
        confidence += 1;
    }
    confidence += JWT_STANDARD_CLAIMS
        .iter()
        .filter(|claim| payload.get(**claim).is_some())
        .count()
        .min(3);
    if confidence < 2 {
        return None;
    }

    let claims = JWT_STANDARD_CLAIMS
        .iter()
        .filter_map(|claim| jwt_claim(&payload, claim))
        .collect();
    Some(JwtFinding {
        path: path.to_owned(),
        search: token.to_owned(),
        source_range: None,
        header_json: serde_json::to_string_pretty(&header).ok()?,
        payload_json: serde_json::to_string_pretty(&payload).ok()?,
        claims,
    })
}

fn jwt_claim(payload: &serde_json::Map<String, serde_json::Value>, name: &str) -> Option<JwtClaim> {
    let value = payload.get(name)?;
    let timestamp = matches!(name, "exp" | "iat" | "nbf")
        .then(|| inspect_timestamp(value, name, Some(name), true))
        .flatten()
        .map(|finding| finding.timestamp);
    let relative = if name == "exp" {
        timestamp
            .as_ref()
            .map(|timestamp| expiration_relative(timestamp.epoch_millis))
    } else {
        None
    };
    Some(JwtClaim {
        name: name.to_owned(),
        value: value_to_compact_string(value),
        timestamp,
        relative,
    })
}

fn decode_base64url_json_object(
    segment: &str,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    if !base64url_candidate(segment) {
        return None;
    }
    let bytes = decode_base64url(segment)?;
    match serde_json::from_slice::<serde_json::Value>(&bytes).ok()? {
        serde_json::Value::Object(object) => Some(object),
        _ => None,
    }
}

fn base64url_candidate(segment: &str) -> bool {
    !segment.is_empty()
        && segment
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn decode_base64url(segment: &str) -> Option<Vec<u8>> {
    let mut bits = 0u32;
    let mut bit_count = 0u8;
    let mut output = Vec::with_capacity(segment.len() * 3 / 4);
    for byte in segment.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => return None,
        } as u32;
        bits = (bits << 6) | value;
        bit_count += 6;
        while bit_count >= 8 {
            bit_count -= 8;
            output.push((bits >> bit_count) as u8);
            bits &= (1 << bit_count) - 1;
        }
    }
    Some(output)
}

pub(super) fn inspect_timestamp(
    value: &serde_json::Value,
    path: &str,
    key: Option<&str>,
    explicit: bool,
) -> Option<TimestampFinding> {
    let (raw, number) = timestamp_number(value)?;
    inspect_timestamp_number(raw, number, path, key, explicit)
}

pub(super) fn inspect_timestamp_text(
    raw: &str,
    path: &str,
    key: Option<&str>,
    explicit: bool,
) -> Option<TimestampFinding> {
    if raw.is_empty() || !raw.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let number = raw.parse::<i64>().ok()?;
    inspect_timestamp_number(raw.to_owned(), number, path, key, explicit)
}

fn inspect_timestamp_number(
    raw: String,
    number: i64,
    path: &str,
    key: Option<&str>,
    explicit: bool,
) -> Option<TimestampFinding> {
    let candidate = classify_unix_timestamp(number)?;
    let mut confidence = if explicit {
        8
    } else {
        candidate.base_confidence
    };
    if let Some(key) = key {
        confidence += timestamp_key_score(key);
    }
    if confidence < 5 {
        return None;
    }
    Some(TimestampFinding {
        path: path.to_owned(),
        search: raw.clone(),
        source_range: None,
        raw,
        timestamp: TimestampDisplay {
            epoch_millis: candidate.epoch_millis,
            millisecond_precision: candidate.millisecond_precision,
        },
        confidence,
    })
}

struct TimestampCandidate {
    epoch_millis: i64,
    millisecond_precision: bool,
    base_confidence: u8,
}

fn timestamp_number(value: &serde_json::Value) -> Option<(String, i64)> {
    match value {
        serde_json::Value::Number(number) => {
            number.as_i64().map(|value| (value.to_string(), value))
        }
        serde_json::Value::String(text) => {
            let trimmed = text.trim();
            if trimmed.len() == text.len()
                && !trimmed.is_empty()
                && trimmed.bytes().all(|byte| byte.is_ascii_digit())
            {
                trimmed
                    .parse::<i64>()
                    .ok()
                    .map(|value| (text.clone(), value))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn classify_unix_timestamp(number: i64) -> Option<TimestampCandidate> {
    let digits = number.unsigned_abs().to_string().len();
    let (epoch_millis, millisecond_precision, base_confidence) = match digits {
        10 => (number.checked_mul(1000)?, false, 3),
        13 => (number, true, 3),
        _ => return None,
    };
    if !plausible_epoch_millis(epoch_millis) {
        return None;
    }
    Some(TimestampCandidate {
        epoch_millis,
        millisecond_precision,
        base_confidence,
    })
}

fn plausible_epoch_millis(epoch_millis: i64) -> bool {
    let start = Utc
        .with_ymd_and_hms(2000, 1, 1, 0, 0, 0)
        .unwrap()
        .timestamp_millis();
    let end = Utc
        .with_ymd_and_hms(2100, 1, 1, 0, 0, 0)
        .unwrap()
        .timestamp_millis();
    (start..end).contains(&epoch_millis)
}

fn timestamp_key_score(key: &str) -> u8 {
    let normalized = key
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    if matches!(
        normalized.as_str(),
        "id" | "userid" | "orderid" | "count" | "code" | "statuscode" | "zip" | "postalcode"
    ) || normalized.ends_with("id")
    {
        return 0;
    }
    if matches!(
        normalized.as_str(),
        "timestamp"
            | "createdat"
            | "updatedat"
            | "expiresat"
            | "issuedat"
            | "created"
            | "updated"
            | "expires"
            | "lastlogin"
            | "date"
            | "time"
    ) || normalized.ends_with("date")
        || normalized.ends_with("time")
        || normalized.contains("timestamp")
    {
        return 3;
    }
    1
}

fn value_to_compact_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        _ => value.to_string(),
    }
}

pub(super) fn format_millis_local(epoch_millis: i64, precise: bool) -> String {
    let Some(datetime) = DateTime::from_timestamp_millis(epoch_millis) else {
        return "Invalid timestamp".to_owned();
    };
    format_datetime(datetime.with_timezone(&Local), precise)
}

pub(super) fn format_millis_utc(epoch_millis: i64, precise: bool) -> String {
    let Some(datetime) = DateTime::from_timestamp_millis(epoch_millis) else {
        return "Invalid timestamp".to_owned();
    };
    format_datetime(datetime, precise)
}

fn format_datetime<Tz: TimeZone>(datetime: DateTime<Tz>, precise: bool) -> String
where
    Tz::Offset: std::fmt::Display,
{
    if precise {
        datetime.format("%Y-%m-%d %H:%M:%S%.3f %:z").to_string()
    } else {
        datetime.format("%Y-%m-%d %H:%M:%S %:z").to_string()
    }
}

fn expiration_relative(epoch_millis: i64) -> String {
    let now = Utc::now().timestamp_millis();
    let delta = epoch_millis - now;
    let duration = TimeDelta::try_milliseconds(delta.abs()).unwrap_or(TimeDelta::MAX);
    let text = if duration.num_days() > 0 {
        format!("{}d", duration.num_days())
    } else if duration.num_hours() > 0 {
        format!("{}h", duration.num_hours())
    } else if duration.num_minutes() > 0 {
        format!("{}m", duration.num_minutes())
    } else {
        format!("{}s", duration.num_seconds())
    };
    if delta >= 0 {
        format!("Expires in {text}")
    } else {
        format!("Expired {text} ago")
    }
}
