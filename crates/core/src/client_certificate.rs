//! Native OpenCollection client-certificate configuration and pure domain matching.

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

/// Matches a literal hostname/authority glob against an HTTPS URL's parsed host.
/// `*` matches zero or more hostname characters; other characters are literal.
/// A portless pattern permits any port. An explicit port matches the effective
/// HTTPS port (including 443). Paths, userinfo, and regex syntax are rejected.
#[must_use]
pub fn client_certificate_domain_matches(pattern: &str, host: &str, port: u16) -> bool {
    if pattern.is_empty()
        || pattern
            .bytes()
            .any(|byte| !byte.is_ascii_alphanumeric() && !b".-*:[]".contains(&byte))
    {
        return false;
    }
    // An unbracketed IPv6 literal is a whole host, never a host:port pair.
    if let Ok(address) = pattern.parse::<std::net::Ipv6Addr>() {
        return host.strip_prefix('[').and_then(|host| host.strip_suffix(']'))
            .and_then(|host| host.parse::<std::net::Ipv6Addr>().ok()) == Some(address);
    }
    let (pattern, configured_port) = match pattern.rsplit_once(':') {
        Some((hostname, value)) if !value.contains(']') => {
            let Ok(port) = value.parse::<u16>() else {
                return false;
            };
            (hostname, Some(port))
        }
        _ => (pattern, None),
    };
    if configured_port.is_some_and(|value| value != port) {
        return false;
    }
    let pattern = pattern.to_ascii_lowercase();
    let host = host.to_ascii_lowercase();
    let mut parts = pattern.split('*');
    let prefix = parts.next().unwrap_or_default();
    let Some(mut remaining) = host.strip_prefix(prefix) else {
        return false;
    };
    let Some(suffix) = parts.next_back() else {
        return remaining.is_empty();
    };
    let Some(middle) = remaining.strip_suffix(suffix) else {
        return false;
    };
    remaining = middle;
    for part in parts {
        let Some(index) = remaining.find(part) else {
            return false;
        };
        remaining = &remaining[index + part.len()..];
    }
    true
}
