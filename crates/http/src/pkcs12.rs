//! PKCS#12 decoding with RustCrypto. Never expose parser errors or decoded bags.

use der::{
    AnyRef, Decode, Encode, EncodePem,
    asn1::{ObjectIdentifier, OctetStringRef},
};
use hmac::{KeyInit, Mac};
use pkcs12::cms::{content_info::ContentInfo, encrypted_data::EncryptedData};
use pkcs12::{AuthenticatedSafe, CertBag, MacData, Pfx, kdf};
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
use zeroize::Zeroizing;

use crate::{HttpError, Pkcs12Error};

const DATA: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.7.1");
const ENCRYPTED_DATA: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.7.6");
const MAX_ITERATIONS: u32 = 1_000_000;
const INVALID: Pkcs12Error = Pkcs12Error::Invalid;
const UNSUPPORTED: Pkcs12Error = Pkcs12Error::Unsupported;
const PASSWORD: Pkcs12Error = Pkcs12Error::Decryption;
const INTEGRITY: Pkcs12Error = Pkcs12Error::Integrity;
const MISMATCH: Pkcs12Error = Pkcs12Error::Mismatch;
const AMBIGUOUS: Pkcs12Error = Pkcs12Error::MultipleIdentities;

fn error(kind: Pkcs12Error) -> HttpError {
    HttpError::Pkcs12(kind)
}

// RFC 7292 SafeBag, borrowed through RustCrypto DER so plaintext key bags
// reference the zeroizing safe buffer instead of making non-zeroizing copies.
#[derive(der::Sequence)]
struct SafeBagRef<'a> {
    bag_id: ObjectIdentifier,
    #[asn1(context_specific = "0", tag_mode = "EXPLICIT")]
    bag_value: AnyRef<'a>,
    bag_attributes: Option<x509_cert::attr::Attributes>,
}

pub(crate) fn identity_from_pkcs12(
    data: &[u8],
    password: Option<&str>,
) -> Result<reqwest::Identity, HttpError> {
    let pfx = Pfx::from_der(data).map_err(|_| error(INVALID))?;
    if pfx.auth_safe.content_type != DATA {
        return Err(error(INVALID));
    }
    // The outer content may itself contain unencrypted private keys.
    let safe = Zeroizing::new(
        pfx.auth_safe
            .content
            .decode_as::<&OctetStringRef>()
            .map_err(|_| error(INVALID))?
            .as_bytes()
            .to_vec(),
    );
    if let Some(mac) = &pfx.mac_data {
        verify_mac(mac, &safe, password.unwrap_or(""))?;
    }
    let safes = AuthenticatedSafe::from_der(&safe).map_err(|_| error(INVALID))?;
    let mut keys: Vec<Zeroizing<Vec<u8>>> = Vec::new();
    let mut certs: Vec<CertificateDer<'static>> = Vec::new();
    for safe in safes {
        let contents = decode_safe(safe, password)?;
        let bags = Vec::<SafeBagRef<'_>>::from_der(&contents).map_err(|_| error(INVALID))?;
        for bag in &bags {
            match bag.bag_id {
                pkcs12::PKCS_12_KEY_BAG_OID => {
                    keys.push(Zeroizing::new(
                        bag.bag_value.to_der().map_err(|_| error(INVALID))?,
                    ));
                }
                pkcs12::PKCS_12_PKCS8_KEY_BAG_OID => {
                    let encrypted = bag
                        .bag_value
                        .decode_as::<pkcs12::pbe_params::EncryptedPrivateKeyInfo>()
                        .map_err(|_| error(INVALID))?;
                    keys.push(decrypt(
                        &encrypted
                            .encryption_algorithm
                            .to_der()
                            .map_err(|_| error(INVALID))?,
                        encrypted.encrypted_data.as_bytes(),
                        password,
                    )?);
                }
                pkcs12::PKCS_12_CERT_BAG_OID => {
                    let cert = bag
                        .bag_value
                        .decode_as::<CertBag>()
                        .map_err(|_| error(INVALID))?;
                    if cert.cert_id != pkcs12::PKCS_12_X509_CERT_OID {
                        return Err(error(UNSUPPORTED));
                    }
                    let cert = CertificateDer::from(cert.cert_value.as_bytes().to_vec());
                    rustls::server::ParsedCertificate::try_from(&cert)
                        .map_err(|_| error(INVALID))?;
                    certs.push(cert);
                }
                // Reject rather than ignore bags that might conceal identities (e.g.
                // nested safeContents). This PR deliberately has no selection policy.
                _ => return Err(error(INVALID)),
            }
        }
    }
    certs.sort_unstable_by(|a, b| a.as_ref().cmp(b.as_ref()));
    certs.dedup();
    select_identity(&keys, certs)
}

fn decode_safe(safe: ContentInfo, password: Option<&str>) -> Result<Zeroizing<Vec<u8>>, HttpError> {
    match safe.content_type {
        DATA => Ok(Zeroizing::new(
            safe.content
                .decode_as::<&OctetStringRef>()
                .map_err(|_| error(INVALID))?
                .as_bytes()
                .to_vec(),
        )),
        ENCRYPTED_DATA => {
            let encoded = safe.content.to_der().map_err(|_| error(INVALID))?;
            let encrypted = EncryptedData::from_der(&encoded).map_err(|_| error(INVALID))?;
            let info = encrypted.enc_content_info;
            if info.content_type != DATA {
                return Err(error(INVALID));
            }
            let data = info.encrypted_content.ok_or_else(|| error(INVALID))?;
            decrypt(
                &info.content_enc_alg.to_der().map_err(|_| error(INVALID))?,
                data.as_bytes(),
                password,
            )
        }
        _ => Err(error(INVALID)),
    }
}

fn decrypt(
    algorithm: &[u8],
    data: &[u8],
    password: Option<&str>,
) -> Result<Zeroizing<Vec<u8>>, HttpError> {
    let scheme = pkcs5::EncryptionScheme::from_der(algorithm).map_err(|_| error(UNSUPPORTED))?;
    let pkcs5::EncryptionScheme::Pbes2(params) = scheme else {
        return Err(error(UNSUPPORTED));
    };
    if !matches!(
        params.encryption,
        pkcs5::pbes2::EncryptionScheme::Aes128Cbc { .. }
            | pkcs5::pbes2::EncryptionScheme::Aes192Cbc { .. }
            | pkcs5::pbes2::EncryptionScheme::Aes256Cbc { .. }
    ) {
        return Err(error(UNSUPPORTED));
    }
    // Keep resource consumption bounded and the compatibility promise explicit.
    let pkcs5::pbes2::Kdf::Pbkdf2(kdf) = &params.kdf else {
        return Err(error(UNSUPPORTED));
    };
    if !(1..=MAX_ITERATIONS).contains(&kdf.iteration_count) {
        return Err(error(INVALID));
    }
    // None means the conventional empty password, supporting empty-password exports.
    let mut decrypted = Zeroizing::new(data.to_vec());
    let length = params
        .decrypt_in_place(password.unwrap_or("").as_bytes(), &mut decrypted)
        .map_err(|failure| match failure {
            pkcs5::Error::DecryptFailed => error(PASSWORD),
            _ => error(INVALID),
        })?
        .len();
    decrypted.truncate(length);
    Ok(decrypted)
}

fn verify_mac(mac: &MacData, data: &[u8], password: &str) -> Result<(), HttpError> {
    if !(1..=MAX_ITERATIONS as i32).contains(&mac.iterations) {
        return Err(error(INVALID));
    }
    // PKCS#12 passwords use big-endian UTF-16 with a terminating NUL.
    // Use RustCrypto's raw KDF entry point rather than its BMPString wrapper,
    // which rejects valid supplementary-plane passwords exported by OpenSSL.
    let password = Zeroizing::new(
        password
            .encode_utf16()
            .flat_map(u16::to_be_bytes)
            .chain([0, 0])
            .collect::<Vec<_>>(),
    );
    macro_rules! verify {
        ($digest:ty) => {{
            let key = Zeroizing::new(kdf::derive_key::<$digest>(
                &password,
                mac.mac_salt.as_bytes(),
                kdf::Pkcs12KeyType::Mac,
                mac.iterations,
                <$digest as hmac::digest::OutputSizeUser>::output_size(),
            ));
            let mut verifier =
                hmac::Hmac::<$digest>::new_from_slice(&key).map_err(|_| error(INVALID))?;
            verifier.update(data);
            verifier
                .verify_slice(mac.mac.digest.as_bytes())
                .map_err(|_| error(INTEGRITY))
        }};
    }
    match mac.mac.algorithm.oid.to_string().as_str() {
        "1.3.14.3.2.26" => verify!(sha1::Sha1),
        "2.16.840.1.101.3.4.2.4" => verify!(sha2::Sha224),
        "2.16.840.1.101.3.4.2.1" => verify!(sha2::Sha256),
        "2.16.840.1.101.3.4.2.2" => verify!(sha2::Sha384),
        "2.16.840.1.101.3.4.2.3" => verify!(sha2::Sha512),
        "2.16.840.1.101.3.4.2.5" => verify!(sha2::Sha512_224),
        "2.16.840.1.101.3.4.2.6" => verify!(sha2::Sha512_256),
        _ => Err(error(UNSUPPORTED)),
    }
}

fn select_identity(
    keys: &[Zeroizing<Vec<u8>>],
    certs: Vec<CertificateDer<'static>>,
) -> Result<reqwest::Identity, HttpError> {
    let mut selected: Option<(usize, usize)> = None;
    let mut public_key = None;
    for (key_index, key) in keys.iter().enumerate() {
        // Borrow the zeroizing input: KeyProvider::load_private_key requires an
        // owned 'static copy, while this same provider's parser accepts a borrow.
        let private_key = PrivatePkcs8KeyDer::from(key.as_slice()).into();
        let signing = rustls::crypto::aws_lc_rs::sign::any_supported_type(&private_key)
            .map_err(|_| error(UNSUPPORTED))?;
        let public = signing.public_key().ok_or_else(|| error(UNSUPPORTED))?;
        if let Some(previous) = &public_key {
            if previous != public.as_ref() {
                return Err(error(AMBIGUOUS));
            }
        } else {
            public_key = Some(public.as_ref().to_vec());
        }
        for (cert_index, cert) in certs.iter().enumerate() {
            let certified = rustls::sign::CertifiedKey::new(vec![cert.clone()], signing.clone());
            if certified.keys_match().is_ok() {
                if selected.is_some_and(|(_, leaf)| leaf != cert_index) {
                    return Err(error(AMBIGUOUS));
                }
                selected = Some((key_index, cert_index));
            }
        }
    }
    let Some((key, leaf)) = selected else {
        return Err(error(if keys.is_empty() || certs.is_empty() {
            Pkcs12Error::NoIdentity
        } else {
            MISMATCH
        }));
    };
    let chain = certificate_chain(leaf, &certs)?;
    // reqwest stays on rustls; its PEM identity API accepts a leaf-first chain.
    let mut pem = Zeroizing::new(String::new());
    for cert in chain {
        let parsed = x509_cert::Certificate::from_der(cert.as_ref()).map_err(|_| error(INVALID))?;
        pem.push_str(
            &parsed
                .to_pem(der::pem::LineEnding::LF)
                .map_err(|_| error(INVALID))?,
        );
    }
    let key = pkcs8::SecretDocument::try_from(keys[key].as_slice()).map_err(|_| error(INVALID))?;
    pem.push_str(
        &key.to_pem("PRIVATE KEY", pkcs8::der::pem::LineEnding::LF)
            .map_err(|_| error(INVALID))?,
    );
    reqwest::Identity::from_pem(pem.as_bytes()).map_err(|_| error(INVALID))
}

fn certificate_chain<'a>(
    leaf: usize,
    certificates: &'a [CertificateDer<'static>],
) -> Result<Vec<&'a CertificateDer<'static>>, HttpError> {
    let parsed = certificates
        .iter()
        .map(|cert| x509_cert::Certificate::from_der(cert.as_ref()).map_err(|_| error(INVALID)))
        .collect::<Result<Vec<_>, _>>()?;
    let mut chain = vec![leaf];
    loop {
        let current = &parsed[*chain.last().expect("chain starts with leaf")];
        let mut issuer = None;
        for (index, candidate) in parsed.iter().enumerate() {
            if chain.contains(&index) {
                continue;
            }
            if candidate.tbs_certificate().subject() != current.tbs_certificate().issuer() {
                continue;
            }
            // Equal names do not imply equal keys, nor that a self-issued
            // intermediate is self-signed. Select its actual signer.
            if certificate_signed_by(current, candidate)? {
                issuer = Some(index);
                break;
            }
        }
        let Some(issuer) = issuer else { break };
        chain.push(issuer);
    }
    Ok(chain
        .into_iter()
        .map(|index| &certificates[index])
        .collect())
}

fn certificate_signed_by(
    certificate: &x509_cert::Certificate,
    issuer: &x509_cert::Certificate,
) -> Result<bool, HttpError> {
    let spki = issuer.tbs_certificate().subject_public_key_info();
    let public_algorithm = spki.algorithm.to_der().map_err(|_| error(INVALID))?;
    let signature_algorithm = certificate
        .signature_algorithm()
        .to_der()
        .map_err(|_| error(INVALID))?;
    let public_algorithm = AnyRef::from_der(&public_algorithm).map_err(|_| error(INVALID))?;
    let signature_algorithm = AnyRef::from_der(&signature_algorithm).map_err(|_| error(INVALID))?;
    let key = spki
        .subject_public_key
        .as_bytes()
        .ok_or_else(|| error(INVALID))?;
    let signature = certificate
        .signature()
        .as_bytes()
        .ok_or_else(|| error(INVALID))?;
    let message = certificate
        .tbs_certificate()
        .to_der()
        .map_err(|_| error(INVALID))?;
    Ok(rustls::crypto::aws_lc_rs::default_provider()
        .signature_verification_algorithms
        .all
        .iter()
        .any(|algorithm| {
            algorithm.public_key_alg_id().as_ref() == public_algorithm.value()
                && algorithm.signature_alg_id().as_ref() == signature_algorithm.value()
                && algorithm.verify_signature(key, &message, signature).is_ok()
        }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    const PASS: &str = "probe-test-only-password";
    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/mtls")
            .join(name)
    }
    fn read(name: &str) -> Vec<u8> {
        std::fs::read(fixture(name)).unwrap()
    }

    #[test]
    fn single_identity_archives_accept_chains_empty_passwords_and_duplicate_bags() {
        for (name, password) in [
            ("client-sha224.p12", Some(PASS)),
            ("client-sha384.p12", Some(PASS)),
            ("client-sha512.p12", Some(PASS)),
            ("client-sha512-224.p12", Some(PASS)),
            ("client-sha512-256.p12", Some(PASS)),
            ("client-unencrypted.p12", None),
            ("client-chain.p12", None),
            ("client-intermediates.p12", None),
            ("client-shared-subject.p12", None),
            ("client-duplicate-key.p12", None),
            ("client-protected.p12", Some(PASS)),
            ("client-unicode-password.p12", Some("probe-test-🔑")),
            ("client-aes128.p12", Some(PASS)),
            ("client-empty-password.pfx", None),
            ("client-empty-password.pfx", Some("")),
            ("client-no-mac.p12", Some(PASS)),
        ] {
            assert!(
                identity_from_pkcs12(&read(name), password).is_ok(),
                "{name}"
            );
        }
    }

    #[test]
    fn errors_are_deterministic_and_do_not_disclose_decoded_material() {
        for (name, password, expected) in [
            ("client-protected.p12", None, INTEGRITY),
            ("client-unicode-password.p12", Some("wrong-🔑"), INTEGRITY),
            (
                "client-protected.p12",
                Some("wrong-passphrase-marker"),
                INTEGRITY,
            ),
            (
                "client-no-mac.p12",
                Some("wrong-passphrase-marker"),
                PASSWORD,
            ),
            ("client-no-mac.p12", None, PASSWORD),
            ("client-mismatch.p12", None, MISMATCH),
            ("client-multiple.p12", None, AMBIGUOUS),
            ("client-multiple-leaves.p12", None, AMBIGUOUS),
            ("client-cert-only.p12", None, Pkcs12Error::NoIdentity),
            ("client-key-only.p12", None, Pkcs12Error::NoIdentity),
            ("client-legacy.p12", Some(PASS), UNSUPPORTED),
            ("client-unsupported-bag.p12", None, INVALID),
        ] {
            let failure = identity_from_pkcs12(&read(name), password).expect_err(name);
            assert_eq!(failure, error(expected), "{name}");
            assert!(failure.is_configuration());
            assert!(failure.to_string().contains("PKCS#12"));
            let diagnostic = format!("{failure} {failure:?}");
            for secret in [PASS, "wrong-passphrase-marker", "BEGIN PRIVATE KEY", name] {
                assert!(!diagnostic.contains(secret));
            }
        }
        assert!(!HttpError::Timeout.is_configuration());
        assert!(!HttpError::Transport("handshake failed".into()).is_configuration());
        for malformed in [
            b"private-key-secret-marker".as_slice(),
            b"",
            &[0x30, 0x80, 0, 0],
        ] {
            assert_eq!(
                identity_from_pkcs12(malformed, Some(PASS)).err(),
                Some(error(INVALID))
            );
        }
        let mut damaged = read("client-protected.p12");
        // OpenSSL ends MacData with salt followed by a four-byte iteration INTEGER.
        let index = damaged.len() - 5;
        damaged[index] ^= 1;
        assert_eq!(
            identity_from_pkcs12(&damaged, Some(PASS)).err(),
            Some(error(INTEGRITY))
        );
    }

    #[test]
    fn rejects_wrong_content_tags_and_unadvertised_encryption() {
        use der::asn1::Any;
        let mut pfx = Pfx::from_der(&read("client-unencrypted.p12")).unwrap();
        let safe = pfx.auth_safe.content.value().to_vec();
        pfx.auth_safe.content = Any::new(der::Tag::Sequence, safe).unwrap();
        assert_eq!(
            identity_from_pkcs12(&pfx.to_der().unwrap(), None).err(),
            Some(error(INVALID))
        );
        let wrong_tag = ContentInfo {
            content_type: DATA,
            content: Any::new(der::Tag::Sequence, vec![]).unwrap(),
        };
        assert_eq!(decode_safe(wrong_tag, None).err(), Some(error(INVALID)));
        // Encrypted data with unsupported algorithm parameters fails before KDF work.
        let mut params =
            pkcs5::pbes2::Parameters::generate_pbkdf2_sha256_aes128cbc(1, b"test-salt", [0; 16])
                .unwrap();
        params.encryption = pkcs5::pbes2::EncryptionScheme::Aes128Gcm { nonce: [0; 12] };
        let algorithm = pkcs5::EncryptionScheme::Pbes2(params).to_der().unwrap();
        assert_eq!(
            decrypt(&algorithm, &[0; 16], Some(PASS)).err(),
            Some(error(UNSUPPORTED))
        );
    }

    #[test]
    fn chain_is_leaf_first_and_deduplicated_from_unordered_archive() {
        use rustls::pki_types::pem::PemObject;
        let leaf = CertificateDer::from_pem_slice(&read("client.pem")).unwrap();
        let root = CertificateDer::from_pem_slice(&read("ca.pem")).unwrap();
        let unrelated = CertificateDer::from_pem_slice(&read("wrong-client.pem")).unwrap();
        let certificates = vec![unrelated, root.clone(), leaf.clone()];
        assert_eq!(
            certificate_chain(2, &certificates).unwrap(),
            vec![&leaf, &root]
        );
        assert_eq!(certificate_chain(1, &certificates).unwrap(), vec![&root]);
        // A missing issuer is valid: the remote verifier may already have it.
        assert_eq!(
            certificate_chain(0, std::slice::from_ref(&leaf)).unwrap(),
            vec![&leaf]
        );
    }

    #[test]
    fn chain_uses_signatures_when_root_intermediate_and_decoy_share_a_subject() {
        use rustls::pki_types::pem::PemObject;
        let leaf = CertificateDer::from_pem_slice(&read("pkcs12-shared-leaf.pem")).unwrap();
        let intermediate =
            CertificateDer::from_pem_slice(&read("pkcs12-shared-intermediate.pem")).unwrap();
        let root = CertificateDer::from_pem_slice(&read("pkcs12-shared-root.pem")).unwrap();
        let decoy = CertificateDer::from_pem_slice(&read("pkcs12-shared-decoy.pem")).unwrap();
        let issuers = [&root, &intermediate, &decoy];
        for order in [
            [0, 1, 2],
            [0, 2, 1],
            [1, 0, 2],
            [1, 2, 0],
            [2, 0, 1],
            [2, 1, 0],
        ] {
            let mut certificates = order.map(|index| issuers[index].clone()).to_vec();
            certificates.push(leaf.clone());
            assert_eq!(
                certificate_chain(3, &certificates).unwrap(),
                vec![&leaf, &intermediate, &root]
            );
        }
    }

    #[test]
    fn issuer_verification_honors_the_declared_signature_algorithm() {
        use der::asn1::Any;
        use rustls::pki_types::pem::PemObject;
        let leaf = CertificateDer::from_pem_slice(&read("pkcs12-shared-leaf.pem")).unwrap();
        let issuer =
            CertificateDer::from_pem_slice(&read("pkcs12-shared-intermediate.pem")).unwrap();
        let issuer = x509_cert::Certificate::from_der(&issuer).unwrap();
        let original = x509_cert::Certificate::from_der(&leaf).unwrap();
        assert!(certificate_signed_by(&original, &issuer).unwrap());
        // Relabel the outer algorithm as SHA-384/RSA while retaining the actual
        // SHA-256 signature and signed TBS bytes. A verifier must not guess a
        // different algorithm just because it can verify the bytes.
        let mut certificate = Vec::<Any>::from_der(&leaf).unwrap();
        let mut algorithm = Vec::<Any>::from_der(&certificate[1].to_der().unwrap()).unwrap();
        algorithm[0] = Any::from_der(
            &ObjectIdentifier::new_unwrap("1.2.840.113549.1.1.12")
                .to_der()
                .unwrap(),
        )
        .unwrap();
        certificate[1] = Any::from_der(&algorithm.to_der().unwrap()).unwrap();
        let relabeled = x509_cert::Certificate::from_der(&certificate.to_der().unwrap()).unwrap();
        assert!(!certificate_signed_by(&relabeled, &issuer).unwrap());
    }

    #[test]
    fn pbkdf2_iterations_above_the_limit_are_rejected_before_decryption() {
        for iterations in [1_000_001, 2_000_000] {
            let parameters = pkcs5::pbes2::Parameters::generate_pbkdf2_sha256_aes128cbc(
                iterations,
                b"test-salt",
                [0; 16],
            )
            .unwrap();
            let algorithm = pkcs5::EncryptionScheme::Pbes2(parameters).to_der().unwrap();
            assert_eq!(
                decrypt(&algorithm, &[0; 16], Some(PASS)).err(),
                Some(error(INVALID))
            );
        }
    }

    #[tokio::test]
    async fn paths_and_debug_are_safe_and_loading_is_deferred() {
        use crate::{ClientIdentity, ExecutionOptions};
        let relative = ClientIdentity::pkcs12(
            "localhost".into(),
            "client-protected.p12".into(),
            Some(PASS.into()),
        );
        assert_eq!(format!("{relative:?}"), "ClientIdentity { .. }");
        let options = ExecutionOptions {
            base_directory: Some(fixture("")),
            ..ExecutionOptions::default()
        };
        assert!(relative.load(&options).await.is_ok());
        assert_eq!(
            relative.load(&ExecutionOptions::default()).await.err(),
            Some(HttpError::ClientConfiguration(
                "relative client certificate/key path requires a base directory".into()
            ))
        );
        let absolute = ClientIdentity::pkcs12(
            "localhost".into(),
            fixture("client-protected.p12").to_str().unwrap().into(),
            Some(PASS.into()),
        );
        assert!(absolute.load(&ExecutionOptions::default()).await.is_ok());
        let missing = ClientIdentity::pkcs12(
            "localhost".into(),
            fixture("secret-path-marker.pfx").to_str().unwrap().into(),
            Some(PASS.into()),
        );
        assert_eq!(
            missing.load(&options).await.err(),
            Some(error(Pkcs12Error::Unreadable))
        );
    }
}
