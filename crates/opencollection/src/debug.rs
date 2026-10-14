//! Debug views preserve document metadata while withholding passphrase values.

use serde_yaml_ng::Value;
use std::fmt;

pub(crate) struct YamlDebug<'a>(pub(crate) &'a Value);

impl fmt::Debug for YamlDebug<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut value = self.0.clone();
        redact_passphrases(&mut value);
        value.fmt(f)
    }
}

pub(crate) struct SourceDebug<'a>(pub(crate) &'a [u8]);

impl fmt::Debug for SourceDebug<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match serde_yaml_ng::from_slice::<Value>(self.0) {
            Ok(value) => YamlDebug(&value).fmt(f),
            // Malformed source cannot be safely inspected field by field.
            Err(_) => f.write_str("[REDACTED: invalid YAML]"),
        }
    }
}

fn redact_passphrases(value: &mut Value) {
    match value {
        Value::Mapping(mapping) => {
            for (key, value) in mapping {
                if key.as_str() == Some("passphrase") {
                    *value = Value::String("[REDACTED]".into());
                } else {
                    redact_passphrases(value);
                }
            }
        }
        Value::Sequence(values) => {
            for value in values {
                redact_passphrases(value);
            }
        }
        Value::Tagged(tagged) => redact_passphrases(&mut tagged.value),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::SourceDebug;

    #[test]
    fn retained_yaml_debug_preserves_metadata_and_redacts_nested_and_tagged_passphrases() {
        for yaml in [
            "domain: api.example.com\npassphrase: SENSITIVE_VALUE\n",
            "config:\n  clientCertificates:\n    - domain: api.example.com\n      passphrase: SENSITIVE_VALUE\n",
            "config: !tagged\n  domain: api.example.com\n  passphrase: SENSITIVE_VALUE\n",
        ] {
            let debug = format!("{:?}", SourceDebug(yaml.as_bytes()));
            assert!(debug.contains("api.example.com"));
            assert!(debug.contains("[REDACTED]"));
            assert!(!debug.contains("SENSITIVE_VALUE"));
        }
    }

    #[test]
    fn invalid_yaml_debug_does_not_disclose_unparseable_source() {
        let debug = format!("{:?}", SourceDebug(b"passphrase: [SENSITIVE_VALUE"));
        assert!(debug.contains("REDACTED"));
        assert!(!debug.contains("SENSITIVE_VALUE"));
    }
}
