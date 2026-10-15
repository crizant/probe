//! Transport identity loading. All diagnostics are fixed strings, never parser output.

use std::{
    fmt,
    path::{Path, PathBuf},
};

use pkcs8::{EncryptedPrivateKeyInfo, SecretDocument, der::pem::LineEnding};
use reqwest::{Identity, Url};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};
use zeroize::Zeroizing;

use crate::{ExecutionOptions, HttpError};

/// A selected transport identity. This has no collection or environment semantics.
#[derive(Clone, Eq, PartialEq)]
pub struct ClientIdentity {
    domain: String,
    material: IdentityMaterial,
    passphrase: Option<Zeroizing<String>>,
}

#[derive(Clone, Eq, PartialEq)]
enum IdentityMaterial {
    Pem { certificate: String, key: String },
    Pkcs12 { path: String },
}

/// Backwards-compatible name for the PEM constructor.
pub type PemClientIdentity = ClientIdentity;

impl fmt::Debug for ClientIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ClientIdentity")
            .finish_non_exhaustive()
    }
}

impl ClientIdentity {
    /// Retains the selected configuration; performs no I/O or decryption.
    #[must_use]
    pub fn new(
        domain: String,
        certificate_file_path: String,
        private_key_file_path: String,
        passphrase: Option<String>,
    ) -> Self {
        Self {
            domain,
            material: IdentityMaterial::Pem {
                certificate: certificate_file_path,
                key: private_key_file_path,
            },
            passphrase: passphrase.map(Zeroizing::new),
        }
    }

    /// Retains a PKCS#12 path and passphrase without reading or decrypting it.
    #[must_use]
    pub fn pkcs12(domain: String, path: String, passphrase: Option<String>) -> Self {
        Self {
            domain,
            material: IdentityMaterial::Pkcs12 { path },
            passphrase: passphrase.map(Zeroizing::new),
        }
    }

    pub(crate) fn for_redirects(&self) -> Self {
        Self::new(self.domain.clone(), String::new(), String::new(), None)
    }

    pub(crate) fn matches(&self, url: &Url) -> bool {
        url.scheme() == "https"
            && url.host_str().is_some_and(|host| {
                probe_core::client_certificate_domain_matches(
                    &self.domain,
                    host,
                    url.port_or_known_default().unwrap_or(443),
                )
            })
    }

    pub(crate) async fn load(&self, options: &ExecutionOptions) -> Result<Identity, HttpError> {
        let passphrase = self.passphrase.clone();
        let (cert_path, key_path) = match &self.material {
            IdentityMaterial::Pkcs12 { path } => {
                let path = resolve_path(path, options)?;
                let container = Zeroizing::new(
                    tokio::fs::read(path)
                        .await
                        .map_err(|_| HttpError::Pkcs12(crate::Pkcs12Error::Unreadable))?,
                );
                return tokio::task::spawn_blocking(move || {
                    crate::pkcs12::identity_from_pkcs12(
                        &container,
                        passphrase.as_deref().map(String::as_str),
                    )
                })
                .await
                .map_err(|_| configuration("client identity loading worker failed"))?;
            }
            IdentityMaterial::Pem { certificate, key } => (
                resolve_path(certificate, options)?,
                resolve_path(key, options)?,
            ),
        };
        let cert = Zeroizing::new(
            tokio::fs::read(cert_path)
                .await
                .map_err(|_| configuration("cannot read client certificate file"))?,
        );
        let key = Zeroizing::new(
            tokio::fs::read(key_path)
                .await
                .map_err(|_| configuration("cannot read client private key file"))?,
        );
        // Key derivation can be expensive. Keep it off async/GPUI executor threads.
        tokio::task::spawn_blocking(move || {
            identity_from_pem(&cert, &key, passphrase.as_deref().map(String::as_str))
        })
        .await
        .map_err(|_| configuration("client identity loading worker failed"))?
    }
}

fn configuration(message: &'static str) -> HttpError {
    HttpError::ClientConfiguration(message.into())
}

fn resolve_path(path: &str, options: &ExecutionOptions) -> Result<PathBuf, HttpError> {
    let path = Path::new(path);
    if path.is_absolute() {
        return Ok(path.to_owned());
    }
    let base = options
        .base_directory
        .as_ref()
        .filter(|base| !base.as_os_str().is_empty())
        .ok_or_else(|| {
            configuration("relative client certificate/key path requires a base directory")
        })?;
    Ok(base.join(path))
}

fn identity_from_pem(
    cert: &[u8],
    key: &[u8],
    passphrase: Option<&str>,
) -> Result<Identity, HttpError> {
    let certificates = CertificateDer::pem_slice_iter(cert)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| configuration("invalid client certificate PEM"))?;
    if certificates.is_empty() {
        return Err(configuration("invalid client certificate PEM"));
    }
    for certificate in &certificates {
        rustls::server::ParsedCertificate::try_from(certificate)
            .map_err(|_| configuration("invalid client certificate PEM"))?;
    }
    let key = unencrypted_key(key, passphrase)?;
    let private_key = PrivateKeyDer::from_pem_slice(key.as_bytes())
        .map_err(|_| configuration("invalid client private key PEM"))?;
    let provider = rustls::crypto::aws_lc_rs::default_provider();
    let signing_key = provider
        .key_provider
        .load_private_key(private_key)
        .map_err(|_| configuration("invalid client private key PEM"))?;
    let certified = rustls::sign::CertifiedKey::new(certificates, signing_key);
    certified.keys_match().map_err(|error| match error {
        rustls::Error::InconsistentKeys(_) => {
            configuration("client certificate and private key do not match")
        }
        _ => configuration("invalid client certificate PEM"),
    })?;
    let mut combined = Zeroizing::new(cert.to_vec());
    combined.push(b'\n');
    combined.extend_from_slice(key.as_bytes());
    Identity::from_pem(&combined).map_err(|_| configuration("invalid client PEM identity"))
}

fn unencrypted_key(key: &[u8], passphrase: Option<&str>) -> Result<Zeroizing<String>, HttpError> {
    let text =
        std::str::from_utf8(key).map_err(|_| configuration("invalid client private key PEM"))?;
    if text.contains("Proc-Type:") || text.contains("DEK-Info:") {
        return Err(configuration(
            "legacy OpenSSL encrypted PEM keys are unsupported; use encrypted PKCS#8",
        ));
    }
    if !text.contains("-----BEGIN ENCRYPTED PRIVATE KEY-----") {
        return Ok(Zeroizing::new(text.to_owned()));
    }
    let passphrase = passphrase
        .ok_or_else(|| configuration("encrypted PKCS#8 private key requires a passphrase"))?;
    let (label, document) = SecretDocument::from_pem(text)
        .map_err(|_| configuration("invalid encrypted PKCS#8 private key"))?;
    if label != "ENCRYPTED PRIVATE KEY" {
        return Err(configuration("invalid encrypted PKCS#8 private key"));
    }
    let encrypted = EncryptedPrivateKeyInfo::try_from(document.as_bytes())
        .map_err(|_| configuration("unsupported or invalid encrypted PKCS#8 private key"))?;
    let decrypted = encrypted.decrypt(passphrase.as_bytes()).map_err(|_| {
        configuration(
            "cannot decrypt PKCS#8 private key: incorrect passphrase or unsupported encryption",
        )
    })?;
    decrypted
        .to_pem("PRIVATE KEY", LineEnding::LF)
        .map_err(|_| configuration("invalid decrypted PKCS#8 private key"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/mtls")
            .join(name)
    }

    fn read(name: &str) -> Vec<u8> {
        std::fs::read(fixture(name)).unwrap()
    }

    #[test]
    fn rsa_ec_pkcs8_chains_and_encrypted_pkcs8_construct_identities() {
        for (cert, key, passphrase) in [
            ("client.pem", "client-key.pem", None),
            ("client.pem", "client-pkcs8-key.pem", None),
            ("ec-client.pem", "ec-client-key.pem", None),
            ("client-chain.pem", "client-key.pem", None),
            (
                "client.pem",
                "client-encrypted-key.pem",
                Some("probe-test-only-password"),
            ),
            (
                "client.pem",
                "client-key.pem",
                Some("unused-passphrase-marker"),
            ),
        ] {
            assert!(
                identity_from_pem(&read(cert), &read(key), passphrase).is_ok(),
                "fixture {cert}, {key}"
            );
        }
    }

    #[test]
    fn invalid_and_encrypted_keys_fail_without_sensitive_diagnostics() {
        let cert = read("client.pem");
        let encrypted = read("client-encrypted-key.pem");
        for (certificate, key, passphrase, diagnostic) in [
            (
                b"not a certificate".as_slice(),
                read("client-key.pem"),
                None,
                "invalid client certificate PEM",
            ),
            (
                cert.as_slice(),
                b"private-key-secret-marker".to_vec(),
                None,
                "invalid client private key PEM",
            ),
            (
                cert.as_slice(),
                encrypted.clone(),
                None,
                "encrypted PKCS#8 private key requires a passphrase",
            ),
            (
                cert.as_slice(),
                encrypted,
                Some("wrong-passphrase-marker"),
                "cannot decrypt PKCS#8 private key: incorrect passphrase or unsupported encryption",
            ),
            (
                cert.as_slice(),
                read("client-legacy-encrypted-key.pem"),
                Some("probe-test-only-password"),
                "legacy OpenSSL encrypted PEM keys are unsupported; use encrypted PKCS#8",
            ),
            (
                cert.as_slice(),
                read("ec-client-key.pem"),
                None,
                "client certificate and private key do not match",
            ),
        ] {
            let error = match identity_from_pem(certificate, &key, passphrase) {
                Ok(_) => panic!("invalid fixture unexpectedly accepted"),
                Err(error) => error,
            };
            assert_eq!(error, configuration(diagnostic));
            let formatted = format!("{error:?} {error}");
            for marker in [
                "private-key-secret-marker",
                "wrong-passphrase-marker",
                "probe-test-only-password",
                "BEGIN PRIVATE KEY",
            ] {
                assert!(!formatted.contains(marker));
            }
        }
        // Either legacy encryption header is sufficient to reject that format,
        // including a truncated or malformed legacy PEM envelope.
        for header in ["Proc-Type: 4,ENCRYPTED", "DEK-Info: AES-256-CBC,00"] {
            let malformed = format!("-----BEGIN RSA PRIVATE KEY-----\n{header}\n");
            assert_eq!(
                unencrypted_key(malformed.as_bytes(), None).err(),
                Some(configuration(
                    "legacy OpenSSL encrypted PEM keys are unsupported; use encrypted PKCS#8"
                ))
            );
        }
        let identity = PemClientIdentity::new(
            "localhost".into(),
            "client.pem".into(),
            "client-key.pem".into(),
            Some("passphrase-marker".into()),
        );
        assert_eq!(format!("{identity:?}"), "ClientIdentity { .. }");
    }

    #[tokio::test]
    async fn filesystem_paths_are_explicit_and_failures_are_safe() {
        let options = ExecutionOptions {
            base_directory: Some(fixture("")),
            ..ExecutionOptions::default()
        };
        let identity = PemClientIdentity::new(
            "localhost".into(),
            "client.pem".into(),
            "client-key.pem".into(),
            None,
        );
        assert!(identity.load(&options).await.is_ok());
        assert_eq!(
            identity.load(&ExecutionOptions::default()).await.err(),
            Some(configuration(
                "relative client certificate/key path requires a base directory"
            ))
        );
        let empty_base = ExecutionOptions {
            base_directory: Some(PathBuf::new()),
            ..ExecutionOptions::default()
        };
        assert_eq!(
            identity.load(&empty_base).await.err(),
            Some(configuration(
                "relative client certificate/key path requires a base directory"
            ))
        );
        let absolute = PemClientIdentity::new(
            "localhost".into(),
            fixture("client.pem").to_str().unwrap().into(),
            fixture("client-key.pem").to_str().unwrap().into(),
            None,
        );
        assert!(absolute.load(&ExecutionOptions::default()).await.is_ok());
        for (cert, key, diagnostic) in [
            (
                "absent-secret-path.pem",
                "client-key.pem",
                "cannot read client certificate file",
            ),
            (
                "client.pem",
                "absent-secret-path.pem",
                "cannot read client private key file",
            ),
            ("", "client-key.pem", "cannot read client certificate file"),
            ("client.pem", "", "cannot read client private key file"),
        ] {
            let identity = PemClientIdentity::new(
                "localhost".into(),
                cert.into(),
                key.into(),
                Some("passphrase-marker".into()),
            );
            assert_eq!(
                identity.load(&options).await.err(),
                Some(configuration(diagnostic))
            );
        }
    }
}
