//! Native OpenCollection client-certificate configuration. No execution or selection rules.

use std::fmt;

/// A client certificate declared in one collection or environment scope.
///
/// Paths and passphrases are retained verbatim as configuration strings. Lists
/// remain in declaration order; no matching, inheritance, or precedence is applied.
#[derive(Clone, Eq, PartialEq)]
pub enum ClientCertificate {
    /// A PEM certificate and private key.
    Pem {
        /// Domain string from the declaration.
        domain: String,
        /// Certificate path, without filesystem resolution.
        certificate_file_path: String,
        /// Private-key path, without filesystem resolution.
        private_key_file_path: String,
        /// Optional literal passphrase or variable reference.
        passphrase: Option<String>,
        /// Whether the declaration is disabled.
        disabled: bool,
    },
    /// A PKCS#12 archive.
    Pkcs12 {
        /// Domain string from the declaration.
        domain: String,
        /// Archive path, without filesystem resolution.
        pkcs12_file_path: String,
        /// Optional literal passphrase or variable reference.
        passphrase: Option<String>,
        /// Whether the declaration is disabled.
        disabled: bool,
    },
}

impl fmt::Debug for ClientCertificate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pem {
                domain,
                certificate_file_path,
                private_key_file_path,
                disabled,
                ..
            } => f
                .debug_struct("Pem")
                .field("domain", domain)
                .field("certificate_file_path", certificate_file_path)
                .field("private_key_file_path", private_key_file_path)
                .field("passphrase", &"[REDACTED]")
                .field("disabled", disabled)
                .finish(),
            Self::Pkcs12 {
                domain,
                pkcs12_file_path,
                disabled,
                ..
            } => f
                .debug_struct("Pkcs12")
                .field("domain", domain)
                .field("pkcs12_file_path", pkcs12_file_path)
                .field("passphrase", &"[REDACTED]")
                .field("disabled", disabled)
                .finish(),
        }
    }
}
