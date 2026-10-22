use crate::RequestResolution;
use probe_core::PreparedHttpRequest;
use probe_http::HttpError;

/// Only the selected environment's direct declarations are retained. This never
/// traverses `extends` or reads identity files.
pub(crate) struct CertificateScopes<'a> {
    environment: &'a [probe_core::ClientCertificate],
    collection: &'a [probe_core::ClientCertificate],
}

impl<'a> CertificateScopes<'a> {
    pub(crate) fn new(resolution: &RequestResolution<'a>) -> Self {
        Self {
            environment: resolution
                .environments
                .iter()
                .find(|environment| Some(environment.name.as_str()) == resolution.environment)
                .map_or(&[][..], |environment| {
                    environment.client_certificates.as_slice()
                }),
            collection: resolution.client_certificates,
        }
    }

    pub(crate) fn select(
        &self,
        request: &PreparedHttpRequest,
        mut interpolate: impl FnMut(&str) -> Result<String, HttpError>,
    ) -> Result<Option<probe_http::ClientIdentity>, HttpError> {
        let request = request.request();
        let Some(raw_url) = request.url.as_deref() else {
            return Ok(None);
        };
        let resolved = probe_core::apply_path_parameters(raw_url, &request.path_parameters);
        let Ok(url) = url::Url::parse(&resolved) else {
            return Ok(None);
        };
        if url.scheme() != "https" {
            return Ok(None);
        }
        let Some(host) = url.host_str() else {
            return Ok(None);
        };
        let port = url.port_or_known_default().unwrap_or(443);
        for certificate in self.environment.iter().chain(self.collection) {
            let (domain, disabled) = match certificate {
                probe_core::ClientCertificate::Pem {
                    domain, disabled, ..
                }
                | probe_core::ClientCertificate::Pkcs12 {
                    domain, disabled, ..
                } => (domain, disabled),
            };
            if *disabled {
                continue;
            }
            let domain = interpolate(domain)?;
            if !probe_core::client_certificate_domain_matches(&domain, host, port) {
                continue;
            }
            return match certificate {
                probe_core::ClientCertificate::Pem {
                    certificate_file_path,
                    private_key_file_path,
                    passphrase,
                    ..
                } => Ok(Some(probe_http::ClientIdentity::new(
                    domain,
                    interpolate(certificate_file_path)?,
                    interpolate(private_key_file_path)?,
                    passphrase.as_deref().map(&mut interpolate).transpose()?,
                ))),
                probe_core::ClientCertificate::Pkcs12 {
                    pkcs12_file_path,
                    passphrase,
                    ..
                } => Ok(Some(probe_http::ClientIdentity::pkcs12(
                    domain,
                    interpolate(pkcs12_file_path)?,
                    passphrase.as_deref().map(&mut interpolate).transpose()?,
                ))),
            };
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NoSecrets, prepare_request};
    use probe_core::{ClientCertificate, Environment, Request};

    fn pem(domain: &str, path: &str, disabled: bool) -> ClientCertificate {
        ClientCertificate::Pem {
            domain: domain.into(),
            certificate_file_path: path.into(),
            private_key_file_path: "unused-key.pem".into(),
            passphrase: None,
            disabled,
        }
    }

    fn select(
        url: &str,
        certificates: &[ClientCertificate],
        environments: &[Environment],
        environment: Option<&str>,
    ) -> Result<Option<probe_http::ClientIdentity>, HttpError> {
        let prepared = prepare_request(
            &Request {
                method: Some("GET".into()),
                url: Some(url.into()),
                ..Request::default()
            },
            &RequestResolution {
                client_certificates: certificates,
                environments,
                environment,
                ..RequestResolution::default()
            },
            &NoSecrets,
        )
        .unwrap();
        let request = prepared.presentation().clone().into_http().unwrap();
        CertificateScopes::new(&RequestResolution {
            client_certificates: certificates,
            environments,
            environment,
            ..RequestResolution::default()
        })
        .select(&request, |value| Ok(value.to_owned()))
    }

    fn identity(domain: &str, path: &str) -> Option<probe_http::ClientIdentity> {
        Some(probe_http::ClientIdentity::new(
            domain.into(),
            path.into(),
            "unused-key.pem".into(),
            None,
        ))
    }

    #[test]
    fn selection_skips_disabled_and_uses_first_matching_declaration() {
        let certificates = [
            pem("*.example.com", "disabled", true),
            pem("elsewhere.test", "unrelated", false),
            pem("*.example.com", "first", false),
            pem("api.example.com", "second", false),
        ];
        assert_eq!(
            select(
                "https://api.example.com/path?host=elsewhere.test",
                &certificates,
                &[],
                None
            )
            .unwrap(),
            identity("*.example.com", "first")
        );
        assert_eq!(
            select("http://api.example.com", &certificates, &[], None).unwrap(),
            None
        );
        assert_eq!(
            select("https://elsewhere.invalid", &certificates, &[], None).unwrap(),
            None
        );
        assert_eq!(
            select(
                "https://api.example.com.evil.test",
                &certificates,
                &[],
                None
            )
            .unwrap(),
            None
        );
    }

    #[test]
    fn parsed_trailing_dot_and_ipv6_hosts_follow_probe_matching_rules() {
        let localhost = [pem("localhost", "plain", false)];
        assert_eq!(
            select("https://localhost./", &localhost, &[], None).unwrap(),
            None
        );
        let dotted = [pem("localhost.", "dotted", false)];
        assert_eq!(
            select("https://localhost./", &dotted, &[], None).unwrap(),
            identity("localhost.", "dotted")
        );
        let ipv6 = [pem("::1", "ipv6", false)];
        assert_eq!(
            select("https://[::1]:8443/", &ipv6, &[], None).unwrap(),
            identity("::1", "ipv6")
        );
        assert_eq!(select("https://[::2]/", &ipv6, &[], None).unwrap(), None);
    }

    #[test]
    fn only_direct_selected_environment_precedes_collection() {
        let certificates = [pem("localhost", "collection", false)];
        let mut environments = vec![
            Environment {
                name: "parent".into(),
                client_certificates: vec![pem("localhost", "parent", false)],
                color: None,
                description: None,
                dot_env_file_path: None,
                variables: Vec::new(),
                extends: None,
            },
            Environment {
                name: "child".into(),
                extends: Some("parent".into()),
                client_certificates: vec![pem("localhost", "direct", false)],
                color: None,
                description: None,
                dot_env_file_path: None,
                variables: Vec::new(),
            },
        ];
        assert_eq!(
            select(
                "https://localhost",
                &certificates,
                &environments,
                Some("child")
            )
            .unwrap(),
            identity("localhost", "direct")
        );
        assert_eq!(
            select("https://localhost", &certificates, &environments, None).unwrap(),
            identity("localhost", "collection")
        );
        environments[1].client_certificates = vec![
            pem("localhost", "disabled", true),
            pem("unrelated.test", "unrelated", false),
        ];
        assert_eq!(
            select(
                "https://localhost",
                &certificates,
                &environments,
                Some("child")
            )
            .unwrap(),
            identity("localhost", "collection")
        );
        assert_eq!(
            select("https://localhost", &[], &environments, Some("child")).unwrap(),
            None
        );
    }

    #[test]
    fn winning_pkcs12_is_selected_and_never_falls_through() {
        let pkcs12 = ClientCertificate::Pkcs12 {
            domain: "localhost".into(),
            pkcs12_file_path: "unused.p12".into(),
            passphrase: Some("secret-marker".into()),
            disabled: false,
        };
        let certificates = [pkcs12.clone(), pem("localhost", "fallback", false)];
        let expected = Some(probe_http::ClientIdentity::pkcs12(
            "localhost".into(),
            "unused.p12".into(),
            Some("secret-marker".into()),
        ));
        assert_eq!(
            select("https://localhost", &certificates, &[], None).unwrap(),
            expected
        );
        let environments = [Environment {
            client_certificates: vec![pkcs12],
            name: "local".into(),
            color: None,
            description: None,
            dot_env_file_path: None,
            variables: Vec::new(),
            extends: None,
        }];
        assert_eq!(
            select(
                "https://localhost",
                &certificates[1..],
                &environments,
                Some("local")
            )
            .unwrap(),
            expected
        );
    }

    #[test]
    fn selection_rejects_unparseable_urls_and_nondefault_port_at_443() {
        let certificates = [pem("localhost:8443", "missing.pem", false)];
        for url in [
            "not a URL",
            "https://[invalid",
            "https://localhost",
            "https://localhost:443",
        ] {
            assert_eq!(select(url, &certificates, &[], None).unwrap(), None);
        }
        assert_eq!(
            select("https://localhost:8443", &certificates, &[], None).unwrap(),
            identity("localhost:8443", "missing.pem")
        );
    }

    #[test]
    fn selection_keeps_the_certificate_host_when_path_parameters_contain_urls() {
        let certificates = [pem("localhost:8443", "missing.pem", false)];
        let request = Request {
            method: Some("GET".into()),
            url: Some("https://localhost:8443/:resource".into()),
            path_parameters: vec![probe_core::QueryParameter {
                name: "resource".into(),
                value: "https://evil.example/private".into(),
                disabled: false,
            }],
            ..Request::default()
        }
        .into_http()
        .unwrap();
        assert_eq!(
            CertificateScopes::new(&RequestResolution {
                client_certificates: &certificates,
                ..RequestResolution::default()
            })
            .select(&request, |value| Ok(value.to_owned()))
            .unwrap(),
            identity("localhost:8443", "missing.pem")
        );
    }

    #[test]
    fn selection_uses_resolved_variables_and_path_parameters_without_reading_files() {
        let certificates = [pem("localhost:8443", "missing-cert.pem", false)];
        let request = Request {
            method: Some("GET".into()),
            url: Some("https://{{host}}:8443/:resource".into()),
            path_parameters: vec![probe_core::QueryParameter {
                name: "resource".into(),
                value: "private".into(),
                disabled: false,
            }],
            ..Request::default()
        };
        let overrides = [("host".into(), "localhost".into())];
        let prepared = prepare_request(
            &request,
            &RequestResolution {
                client_certificates: &certificates,
                overrides: &overrides,
                ..RequestResolution::default()
            },
            &NoSecrets,
        )
        .unwrap();
        let request = prepared.presentation().clone().into_http().unwrap();
        assert_eq!(
            CertificateScopes::new(&RequestResolution {
                client_certificates: &certificates,
                ..RequestResolution::default()
            })
            .select(&request, |value| Ok(value.to_owned()))
            .unwrap(),
            identity("localhost:8443", "missing-cert.pem")
        );
    }
}

#[cfg(test)]
mod execution_tests {
    use super::*;
    use crate::{NoSecrets, prepare_request};
    use probe_http::{ExecutionOptions, HttpEngine};

    #[tokio::test]
    async fn opencollection_http_and_graphql_share_selection_and_defer_file_io() {
        let loaded = probe_opencollection::load_workspace_from_str(include_str!(
            "../../../tests/fixtures/opencollection/mtls-execution.yml"
        ))
        .unwrap();
        let engine = HttpEngine::new().unwrap();
        for item in loaded.requests() {
            let prepared = prepare_request(
                loaded.workspace().request(item.key()).unwrap(),
                &RequestResolution {
                    client_certificates: loaded.workspace().client_certificates(),
                    ..RequestResolution::default()
                },
                &NoSecrets,
            )
            .unwrap();
            // Presentation succeeds even though both configured identity files are absent.
            assert!(
                prepared
                    .presentation()
                    .url
                    .as_ref()
                    .unwrap()
                    .contains("localhost:8443")
            );
            let error = prepared
                .into_http()
                .unwrap()
                .execute(
                    &engine,
                    ExecutionOptions::default(),
                    None,
                    std::future::pending::<()>(),
                    |_| {},
                )
                .await
                .unwrap_err();
            assert_eq!(
                error,
                HttpError::ClientConfiguration(
                    "relative client identity path requires a base directory".into()
                )
            );
        }
    }
}

/// Resolve one editor declaration without host selection or an HTTP request.
/// Errors deliberately omit names and values supplied by the resolver.
pub fn inspect_certificate(
    certificate: &probe_core::ClientCertificate,
    environments: &[probe_core::Environment],
    selected: Option<&str>,
    provider: &dyn probe_core::SecretProvider,
    options: &probe_http::ExecutionOptions,
    public_only: bool,
) -> probe_http::CertificateInspection {
    inspect_certificate_observing(
        certificate,
        environments,
        selected,
        provider,
        options,
        public_only,
        |_| {},
    )
}

/// Installs background observation after resolution and before reading sources.
/// The observer must not expose resolved identity configuration.
pub fn inspect_certificate_observing(
    certificate: &probe_core::ClientCertificate,
    environments: &[probe_core::Environment],
    selected: Option<&str>,
    provider: &dyn probe_core::SecretProvider,
    options: &probe_http::ExecutionOptions,
    public_only: bool,
    observe: impl FnOnce(&probe_http::ClientIdentity),
) -> probe_http::CertificateInspection {
    let failure = || probe_http::CertificateInspection {
        metadata: None,
        validation: Err(HttpError::ClientConfiguration(
            "Certificate metadata unavailable: cannot resolve variables or secrets.".into(),
        )),
    };
    let (path, key, password) = match certificate {
        probe_core::ClientCertificate::Pem {
            certificate_file_path,
            private_key_file_path,
            passphrase,
            ..
        } => (
            certificate_file_path.as_str(),
            Some(private_key_file_path.as_str()),
            passphrase.as_deref(),
        ),
        probe_core::ClientCertificate::Pkcs12 {
            pkcs12_file_path,
            passphrase,
            ..
        } => (pkcs12_file_path.as_str(), None, passphrase.as_deref()),
    };
    let templates = if public_only {
        vec![path]
    } else {
        vec![path, key.unwrap_or(""), password.unwrap_or("")]
    };
    let provider = if public_only {
        &crate::NoSecrets as &dyn probe_core::SecretProvider
    } else {
        provider
    };
    let Ok(resolved) = probe_core::resolve_environment_for_templates_with_provider(
        templates,
        environments,
        selected,
        &[],
        provider,
        None,
    ) else {
        return failure();
    };
    let interpolate = |value: &str| resolved.interpolate_strict(value);
    let Ok(path) = interpolate(path) else {
        return failure();
    };
    let identity = if public_only {
        if key.is_some() {
            probe_http::ClientIdentity::new(String::new(), path, String::new(), None)
        } else {
            probe_http::ClientIdentity::pkcs12(String::new(), path, None)
        }
    } else {
        let Ok(password) = password.map(interpolate).transpose() else {
            return failure();
        };
        if let Some(key) = key {
            let Ok(key) = interpolate(key) else {
                return failure();
            };
            probe_http::ClientIdentity::new(String::new(), path, key, password)
        } else {
            probe_http::ClientIdentity::pkcs12(String::new(), path, password)
        }
    };
    observe(&identity);
    let mut result = if public_only {
        match identity.public_metadata(options) {
            Ok(metadata) => probe_http::CertificateInspection {
                metadata: Some(metadata),
                validation: Ok(()),
            },
            Err(error) => probe_http::CertificateInspection {
                metadata: None,
                validation: Err(error),
            },
        }
    } else {
        identity.inspect(options)
    };
    if let Some(metadata) = &mut result.metadata {
        for text in [
            &mut metadata.subject,
            &mut metadata.issuer,
            &mut metadata.valid_from,
            &mut metadata.expires,
            &mut metadata.serial,
            &mut metadata.fingerprint,
        ] {
            *text = resolved.redact_secrets(text);
        }
    }
    result
}

#[cfg(test)]
mod inspection_tests {
    use super::*;
    use probe_core::{
        EnvironmentVariable, SecretContext, SecretError, SecretProvider, SecretValue,
        SecretVariable,
    };
    use std::cell::Cell;
    struct Provider {
        calls: Cell<usize>,
        value: &'static str,
    }
    impl SecretProvider for Provider {
        fn resolve_secret(
            &self,
            _: &SecretContext<'_>,
        ) -> Result<Option<SecretValue>, SecretError> {
            self.calls.set(self.calls.get() + 1);
            Ok(Some(SecretValue::new(self.value.into())))
        }
    }
    #[test]
    fn inspection_uses_draft_templates_and_reachable_secrets_without_mutation_or_disclosure() {
        let mut loaded = probe_opencollection::load_workspace_from_str(include_str!(
            "../../../tests/fixtures/opencollection/mtls-variables.yml"
        ))
        .unwrap()
        .workspace()
        .environments()
        .to_vec();
        let child = loaded.iter_mut().find(|env| env.name == "child").unwrap();
        child
            .variables
            .push(EnvironmentVariable::Secret(SecretVariable {
                name: Some("password".into()),
                value_type: None,
                disabled: false,
            }));
        let certificate = probe_core::ClientCertificate::Pem {
            domain: "{{unusedDomain}}".into(),
            certificate_file_path: "{{filename}}".into(),
            private_key_file_path: "client-encrypted-key.pem".into(),
            passphrase: Some("{{password}}".into()),
            disabled: true,
        };
        child
            .variables
            .push(EnvironmentVariable::Plain(probe_core::Variable {
                name: Some("filename".into()),
                value: Some(probe_core::VariableValueSet::Single(
                    probe_core::VariableValue::String("client.pem".into()),
                )),
                disabled: false,
            }));
        let original = loaded.clone();
        let provider = Provider {
            calls: Cell::new(0),
            value: "probe-test-only-password",
        };
        let options = probe_http::ExecutionOptions {
            base_directory: Some(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/mtls"),
            ),
            ..Default::default()
        };
        let public = inspect_certificate(
            &certificate,
            &loaded,
            Some("child"),
            &provider,
            &options,
            true,
        );
        assert!(public.metadata.is_some());
        assert_eq!(
            provider.calls.get(),
            0,
            "previews never resolve passphrase secrets"
        );
        let result = inspect_certificate(
            &certificate,
            &loaded,
            Some("child"),
            &provider,
            &options,
            false,
        );
        assert!(result.validation.is_ok());
        assert_eq!(provider.calls.get(), 1);
        assert_eq!(loaded, original);
        assert!(!format!("{result:?}").contains(provider.value));
        let wrong = Provider {
            calls: Cell::new(0),
            value: "wrong-secret-marker",
        };
        let result = inspect_certificate(
            &certificate,
            &loaded,
            Some("child"),
            &wrong,
            &options,
            false,
        );
        assert!(result.validation.is_err());
        assert!(!format!("{result:?}").contains(wrong.value));
        let mut unencrypted = certificate.clone();
        if let probe_core::ClientCertificate::Pem {
            private_key_file_path,
            ..
        } = &mut unencrypted
        {
            *private_key_file_path = "client-key.pem".into();
        }
        let subject_secret = Provider {
            calls: Cell::new(0),
            value: "Probe test-only client",
        };
        let result = inspect_certificate(
            &unencrypted,
            &loaded,
            Some("child"),
            &subject_secret,
            &options,
            false,
        );
        assert!(result.validation.is_ok());
        assert!(!format!("{result:?}").contains(subject_secret.value));
        let missing = inspect_certificate(
            &certificate,
            &loaded,
            Some("child"),
            &crate::NoSecrets,
            &options,
            false,
        );
        assert!(missing.validation.is_err());
        assert!(!format!("{missing:?}").contains("password"));
    }
}
