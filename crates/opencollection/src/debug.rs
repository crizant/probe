//! Retained source Debug exposes only metadata, never source contents.

use std::fmt;

pub(crate) struct SourceDebug<'a>(pub(crate) &'a [u8]);

impl fmt::Debug for SourceDebug<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Source(len={})", self.0.len())
    }
}

#[cfg(test)]
mod tests {
    use super::SourceDebug;

    #[test]
    fn retained_source_debug_reports_length_without_disclosing_plain_or_aliased_passphrases() {
        for yaml in [
            "domain: api.example.com\npassphrase: SENSITIVE_VALUE\n",
            "config:\n  clientCertificates:\n    - domain: api.example.com\n      passphrase: SENSITIVE_VALUE\n",
            "config: !tagged\n  domain: api.example.com\n  passphrase: SENSITIVE_VALUE\n",
            "passphrase: &p SENSITIVE_VALUE\nb: *p\n",
            "passphrase: [SENSITIVE_VALUE",
        ] {
            let debug = format!("{:?}", SourceDebug(yaml.as_bytes()));
            assert!(debug.contains(&format!("len={}", yaml.len())));
            assert!(!debug.contains("api.example.com"));
            assert!(!debug.contains("SENSITIVE_VALUE"));
        }
    }
}
