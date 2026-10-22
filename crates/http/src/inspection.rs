//! Local inspection only. Call on a blocking worker; never performs network I/O.
use crate::{ClientIdentity, ExecutionOptions, HttpError};
use der::Decode;
use rustls::pki_types::{CertificateDer, pem::PemObject};
use sha2::{Digest, Sha256};
use std::time::{Duration, SystemTime};
use zeroize::Zeroizing;

/// Public certificate data; contains no identity material or resolved paths.
#[derive(Clone, Debug)]
pub struct CertificateMetadata {
    pub subject: String,
    pub issuer: String,
    pub valid_from: String,
    pub expires: String,
    pub serial: String,
    pub fingerprint: String,
    pub not_before: SystemTime,
    pub not_after: SystemTime,
}

impl CertificateMetadata {
    pub fn validity_warning(&self, now: SystemTime) -> Option<&'static str> {
        if now < self.not_before {
            Some("Certificate is not yet valid.")
        } else if now > self.not_after {
            Some("Certificate has expired.")
        } else if self.not_after.duration_since(now).unwrap_or_default()
            <= Duration::from_secs(30 * 86400)
        {
            Some("Certificate expires within 30 days.")
        } else {
            None
        }
    }

    fn parse(bytes: &[u8]) -> Result<Self, HttpError> {
        let cert = x509_cert::Certificate::from_der(bytes).map_err(|_| invalid())?;
        let tbs = cert.tbs_certificate();
        Ok(Self {
            subject: tbs
                .subject()
                .common_name()
                .ok()
                .flatten()
                .map(|name| name.value().into_owned())
                .unwrap_or_else(|| tbs.subject().to_string()),
            issuer: tbs.issuer().to_string(),
            valid_from: tbs.validity().not_before.to_string(),
            expires: tbs.validity().not_after.to_string(),
            serial: hex(tbs.serial_number().as_bytes()),
            fingerprint: hex(&Sha256::digest(bytes)),
            not_before: SystemTime::UNIX_EPOCH + tbs.validity().not_before.to_unix_duration(),
            not_after: SystemTime::UNIX_EPOCH + tbs.validity().not_after.to_unix_duration(),
        })
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}
fn invalid() -> HttpError {
    HttpError::ClientConfiguration("invalid client certificate PEM".into())
}

#[derive(Clone, Debug)]
pub struct CertificateInspection {
    pub metadata: Option<CertificateMetadata>,
    /// Identity validity is independent of certificate dates and remote trust.
    pub validation: Result<(), HttpError>,
}

impl ClientIdentity {
    /// Reads public PEM or cleartext PKCS#12 certificate bags without decryption.
    pub fn public_metadata(
        &self,
        options: &ExecutionOptions,
    ) -> Result<CertificateMetadata, HttpError> {
        let path = match &self.material {
            super::identity::IdentityMaterial::Pem { certificate, .. } => certificate,
            super::identity::IdentityMaterial::Pkcs12 { path } => path,
        };
        let bytes = Zeroizing::new(
            std::fs::read(super::identity::resolve_path(path, options)?).map_err(|_| {
                HttpError::ClientConfiguration("cannot read client certificate file".into())
            })?,
        );
        let cert = match &self.material {
            super::identity::IdentityMaterial::Pem { .. } => {
                CertificateDer::from_pem_slice(&bytes).map_err(|_| invalid())?
            }
            super::identity::IdentityMaterial::Pkcs12 { .. } => {
                crate::pkcs12::public_certificate_from_pkcs12(&bytes)?
            }
        };
        CertificateMetadata::parse(cert.as_ref())
    }

    pub fn inspect(&self, options: &ExecutionOptions) -> CertificateInspection {
        let mut metadata = None;
        let validation = (|| {
            let password = self.passphrase.as_deref().map(String::as_str);
            match &self.material {
                super::identity::IdentityMaterial::Pem { certificate, key } => {
                    let cert = Zeroizing::new(
                        std::fs::read(super::identity::resolve_path(certificate, options)?)
                            .map_err(|_| {
                                HttpError::ClientConfiguration(
                                    "cannot read client certificate file".into(),
                                )
                            })?,
                    );
                    let leaf = CertificateDer::from_pem_slice(&cert).map_err(|_| invalid())?;
                    metadata = Some(CertificateMetadata::parse(leaf.as_ref())?);
                    let key = Zeroizing::new(
                        std::fs::read(super::identity::resolve_path(key, options)?).map_err(
                            |_| {
                                HttpError::ClientConfiguration(
                                    "cannot read client private key file".into(),
                                )
                            },
                        )?,
                    );
                    super::identity::identity_from_pem(&cert, &key, password)?;
                }
                super::identity::IdentityMaterial::Pkcs12 { path } => {
                    let data = Zeroizing::new(
                        std::fs::read(super::identity::resolve_path(path, options)?)
                            .map_err(|_| HttpError::Pkcs12(crate::Pkcs12Error::Unreadable))?,
                    );
                    let (_, cert) = crate::pkcs12::inspect_pkcs12(&data, password)?;
                    metadata = Some(CertificateMetadata::parse(cert.as_ref())?);
                }
            }
            Ok(())
        })();
        CertificateInspection {
            metadata,
            validation,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn options() -> ExecutionOptions {
        ExecutionOptions {
            base_directory: Some(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/mtls"),
            ),
            ..Default::default()
        }
    }
    fn pem(key: &str, password: Option<&str>) -> ClientIdentity {
        ClientIdentity::new(
            String::new(),
            "client.pem".into(),
            key.into(),
            password.map(str::to_owned),
        )
    }

    #[test]
    fn public_metadata_does_not_read_keys_or_decrypt_and_matches_inspected_leaf() {
        let options = options();
        let public = pem("absent-private-key", Some("wrong-password"))
            .public_metadata(&options)
            .unwrap();
        assert!(public.subject.contains("client"));
        assert!(!public.issuer.is_empty());
        assert!(!public.serial.is_empty());
        assert_eq!(public.fingerprint.split(':').count(), 32);
        for file in [
            "client-unencrypted.p12",
            "client-protected.p12",
            "client-empty-password.pfx",
        ] {
            let identity = ClientIdentity::pkcs12(
                String::new(),
                file.into(),
                Some(
                    if file == "client-empty-password.pfx" {
                        ""
                    } else {
                        "probe-test-only-password"
                    }
                    .into(),
                ),
            );
            let inspected = identity.inspect(&options);
            assert!(
                inspected.validation.is_ok(),
                "{file}: {:?}",
                inspected.validation
            );
            assert_eq!(inspected.metadata.unwrap().fingerprint, public.fingerprint);
            if file == "client-unencrypted.p12" {
                assert_eq!(
                    identity.public_metadata(&options).unwrap().fingerprint,
                    public.fingerprint
                );
            } else {
                assert!(identity.public_metadata(&options).is_err());
            }
        }
        let preview = |file: &str| {
            ClientIdentity::pkcs12(String::new(), file.into(), None)
                .public_metadata(&options)
                .unwrap_err()
        };
        assert_eq!(
            preview("client-key-only.p12"),
            HttpError::Pkcs12(crate::Pkcs12Error::NoIdentity)
        );
        assert_eq!(
            preview("client-cert-only.p12"),
            HttpError::Pkcs12(crate::Pkcs12Error::MultipleIdentities)
        );
    }

    #[test]
    fn dates_are_independent_of_key_validation_and_have_expiry_boundaries() {
        let inspected = pem("client-key.pem", None).inspect(&options());
        assert!(inspected.validation.is_ok());
        let metadata = inspected.metadata.unwrap();
        // The timestamps used by warnings must agree with the dates shown in the UI.
        assert_eq!(
            der::DateTime::from_unix_duration(
                metadata
                    .not_before
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap()
            )
            .unwrap()
            .to_string(),
            metadata.valid_from
        );
        assert_eq!(
            der::DateTime::from_unix_duration(
                metadata
                    .not_after
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap()
            )
            .unwrap()
            .to_string(),
            metadata.expires
        );
        assert_eq!(metadata.validity_warning(metadata.not_before), None);
        assert_eq!(
            metadata.validity_warning(metadata.not_after),
            Some("Certificate expires within 30 days.")
        );
        assert_eq!(
            metadata.validity_warning(metadata.not_after - Duration::from_secs(30 * 86400)),
            Some("Certificate expires within 30 days.")
        );
        assert_eq!(
            metadata.validity_warning(metadata.not_after - Duration::from_secs(30 * 86400 + 1)),
            None
        );

        assert_eq!(
            metadata.validity_warning(metadata.not_before - Duration::from_secs(1)),
            Some("Certificate is not yet valid.")
        );
        assert_eq!(
            metadata.validity_warning(metadata.not_after + Duration::from_secs(1)),
            Some("Certificate has expired.")
        );
        assert_eq!(
            metadata.validity_warning(metadata.not_after - Duration::from_secs(86400)),
            Some("Certificate expires within 30 days.")
        );
        assert_eq!(
            metadata.validity_warning(metadata.not_before + Duration::from_secs(1)),
            None
        );
    }

    #[test]
    fn encrypted_keys_mismatch_and_failures_keep_safe_public_metadata() {
        for (key, password, success, message) in [
            (
                "client-encrypted-key.pem",
                Some("probe-test-only-password"),
                true,
                "",
            ),
            (
                "client-encrypted-key.pem",
                Some("wrong-password-marker"),
                false,
                "cannot decrypt",
            ),
            ("wrong-client-key.pem", None, false, "do not match"),
            (
                "client-legacy-encrypted-key.pem",
                Some("probe-test-only-password"),
                false,
                "unsupported",
            ),
            ("absent-secret-key-path", None, false, "cannot read"),
        ] {
            let result = pem(key, password).inspect(&options());
            assert!(result.metadata.is_some());
            assert_eq!(result.validation.is_ok(), success);
            if !success {
                assert!(result.validation.unwrap_err().to_string().contains(message));
            }
            assert!(!format!("{:?}", pem(key, password)).contains("password"));
        }
        for path in ["absent-secret-path", "client-key.pem"] {
            let result =
                ClientIdentity::new(String::new(), path.into(), "client-key.pem".into(), None)
                    .inspect(&options());
            assert!(result.metadata.is_none());
            assert!(result.validation.is_err());
            assert!(!format!("{result:?}").contains(path));
        }
        for (file, expected) in [
            ("client-mismatch.p12", crate::Pkcs12Error::Mismatch),
            (
                "client-multiple.p12",
                crate::Pkcs12Error::MultipleIdentities,
            ),
            ("client-legacy.p12", crate::Pkcs12Error::Unsupported),
        ] {
            let result = ClientIdentity::pkcs12(
                String::new(),
                file.into(),
                Some("probe-test-only-password".into()),
            )
            .inspect(&options());
            assert_eq!(result.validation, Err(HttpError::Pkcs12(expected)));
        }
        let result = ClientIdentity::pkcs12(
            String::new(),
            "client-protected.p12".into(),
            Some("wrong-password-marker".into()),
        )
        .inspect(&options());
        assert_eq!(
            result.validation,
            Err(HttpError::Pkcs12(crate::Pkcs12Error::Integrity))
        );
        assert!(!format!("{result:?}").contains("wrong-password-marker"));
    }
}
