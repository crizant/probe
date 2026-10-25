//! Retained source Debug exposes only metadata, never source contents.

use std::{fmt, ops::Deref, sync::Arc};

pub(crate) struct SourceDebug<'a>(pub(crate) &'a [u8]);

impl fmt::Debug for SourceDebug<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Source(len={})", self.0.len())
    }
}

/// Shared retained YAML bytes with the same metadata-only Debug as borrowed source.
#[derive(Clone, PartialEq)]
pub(crate) struct SourceBytes(Arc<[u8]>);

impl fmt::Debug for SourceBytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        SourceDebug(self).fmt(f)
    }
}

impl Deref for SourceBytes {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<Vec<u8>> for SourceBytes {
    fn from(source: Vec<u8>) -> Self {
        Self(source.into())
    }
}

impl From<&[u8]> for SourceBytes {
    fn from(source: &[u8]) -> Self {
        Self(source.into())
    }
}

#[cfg(test)]
mod tests {
    use super::{SourceBytes, SourceDebug};
    use std::sync::Arc;

    #[test]
    fn shared_source_debug_preserves_metadata_and_clones_share_bytes() {
        let source = SourceBytes::from(b"passphrase: PRIVATE_SOURCE".as_slice());
        let clone = source.clone();
        assert!(Arc::ptr_eq(&source.0, &clone.0));
        assert_eq!(&*clone, b"passphrase: PRIVATE_SOURCE");
        for debug in [format!("{source:?}"), format!("{source:#?}")] {
            assert_eq!(debug, "Source(len=26)");
        }
        assert_eq!(format!("{:?}", Some(source)), "Some(Source(len=26))");
        assert_eq!(format!("{:?}", None::<SourceBytes>), "None");
        assert_eq!(
            format!("{:?}", SourceBytes::from(Vec::new())),
            "Source(len=0)"
        );
    }

    #[test]
    fn retained_source_debug_reports_length_without_disclosing_plain_or_aliased_passphrases() {
        for yaml in [
            "domain: api.example.com\npassphrase: SENSITIVE_VALUE\n",
            "config:\n  clientCertificates:\n    - domain: api.example.com\n      passphrase: SENSITIVE_VALUE\n",
            "config: !tagged\n  domain: api.example.com\n  passphrase: SENSITIVE_VALUE\n",
            "passphrase: &p SENSITIVE_VALUE\nb: *p\n",
            "passphrase: [SENSITIVE_VALUE",
        ] {
            let source = SourceBytes::from(yaml.as_bytes());
            let debug = format!("{source:?}");
            assert_eq!(debug, format!("{:?}", SourceDebug(yaml.as_bytes())));
            assert_eq!(debug, format!("{source:#?}"));
            assert!(debug.contains(&format!("len={}", yaml.len())));
            assert!(!debug.contains("api.example.com"));
            assert!(!debug.contains("SENSITIVE_VALUE"));
        }
    }
}
