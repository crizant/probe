use crate::RequestResolution;
use probe_core::PreparedHttpRequest;
use probe_http::HttpError;

/// A transient choice, deliberately separate from collection and session data.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum CertificateOverride {
    #[default]
    Automatic,
    NoCertificate,
    Selected(CertificateRef),
}

/// Exact declaration plus its original scope. Equality survives reordering, but
/// rejects edits (including paths, passphrases and enablement) and scope changes.
/// No resolved secrets or list positions are retained.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CertificateRef {
    pub certificate: probe_core::ClientCertificate,
    pub environment: Option<String>,
}

fn invalid_override() -> HttpError {
    HttpError::ClientConfiguration(
        "Selected client certificate is unavailable or no longer matches this HTTPS target. Reset to Automatic or select another certificate.".into(),
    )
}

/// Only the selected environment's direct declarations are retained. This never
/// traverses `extends` or reads identity files.
pub(crate) struct CertificateScopes<'a> {
    environment: &'a [probe_core::ClientCertificate],
    collection: &'a [probe_core::ClientCertificate],
    selected_environment: Option<&'a str>,
    selection: Option<&'a CertificateOverride>,
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
            selected_environment: resolution.environment,
            selection: resolution.certificate_override,
        }
    }

    pub(crate) fn select(
        &self,
        request: &PreparedHttpRequest,
        mut interpolate: impl FnMut(&str) -> Result<String, HttpError>,
    ) -> Result<Option<probe_http::ClientIdentity>, HttpError> {
        let request = request.request();
        let Some(raw_url) = request.url.as_deref() else {
            return if matches!(self.selection, Some(CertificateOverride::Selected(_))) {
                Err(invalid_override())
            } else {
                Ok(None)
            };
        };
        let resolved = probe_core::apply_path_parameters(raw_url, &request.path_parameters);
        let Some((_, certificate, domain)) =
            self.select_declaration(&resolved, &mut interpolate)?
        else {
            return Ok(None);
        };
        match certificate {
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
        }
    }

    fn select_declaration(
        &self,
        resolved: &str,
        mut interpolate: impl FnMut(&str) -> Result<String, HttpError>,
    ) -> Result<Option<(usize, &'a probe_core::ClientCertificate, String)>, HttpError> {
        if matches!(self.selection, Some(CertificateOverride::NoCertificate)) {
            return Ok(None);
        }
        let selected = match self.selection {
            Some(CertificateOverride::Selected(reference)) => Some(
                self.find_declaration(reference)
                    .ok_or_else(invalid_override)?,
            ),
            _ => None,
        };
        let no_match = || {
            if selected.is_some() {
                Err(invalid_override())
            } else {
                Ok(None)
            }
        };
        let Ok(url) = url::Url::parse(resolved) else {
            return no_match();
        };
        if let Some((index, certificate)) = selected {
            return match matching_domain(certificate, &url, &mut interpolate)? {
                Some(domain) => Ok(Some((index, certificate, domain))),
                None => no_match(),
            };
        }
        for (index, certificate) in self.environment.iter().chain(self.collection).enumerate() {
            if let Some(domain) = matching_domain(certificate, &url, &mut interpolate)? {
                return Ok(Some((index, certificate, domain)));
            }
        }
        Ok(None)
    }

    fn find_declaration(
        &self,
        reference: &CertificateRef,
    ) -> Option<(usize, &'a probe_core::ClientCertificate)> {
        let (certificates, offset) = match reference.environment.as_deref() {
            None => (self.collection, self.environment.len()),
            Some(name) if Some(name) == self.selected_environment => (self.environment, 0),
            _ => return None,
        };
        certificates
            .iter()
            .enumerate()
            .find_map(|(index, certificate)| {
                (certificate == &reference.certificate && enabled_domain(certificate).is_some())
                    .then_some((index + offset, certificate))
            })
    }
}

fn enabled_domain(certificate: &probe_core::ClientCertificate) -> Option<&str> {
    match certificate {
        probe_core::ClientCertificate::Pem {
            domain, disabled, ..
        }
        | probe_core::ClientCertificate::Pkcs12 {
            domain, disabled, ..
        } => (!disabled).then_some(domain),
    }
}

/// Shared by execution, preview and candidate enumeration. The resolver controls
/// whether secrets may be accessed; matching itself never reads identity material.
fn matching_domain(
    certificate: &probe_core::ClientCertificate,
    url: &url::Url,
    mut interpolate: impl FnMut(&str) -> Result<String, HttpError>,
) -> Result<Option<String>, HttpError> {
    if url.scheme() != "https" {
        return Ok(None);
    }
    let Some(host) = url.host_str() else {
        return Ok(None);
    };
    let Some(domain) = enabled_domain(certificate) else {
        return Ok(None);
    };
    let domain = interpolate(domain)?;
    Ok(probe_core::client_certificate_domain_matches(
        &domain,
        host,
        url.port_or_known_default().unwrap_or(443),
    )
    .then_some(domain))
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
            assert!(matches!(
                preview_client_certificate(
                    loaded.workspace().request(item.key()).unwrap(),
                    &RequestResolution {
                        client_certificates: loaded.workspace().client_certificates(),
                        ..Default::default()
                    }
                ),
                CertificatePreview::Selected { .. }
            ));
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

/// Informational selection only: no credential access or identity material loading.
#[derive(Clone, Debug)]
pub enum CertificatePreview {
    Hidden,
    NoCertificate,
    Invalid,
    Unknown,
    Unmatched,
    Selected {
        certificate: probe_core::ClientCertificate,
        environment: Option<String>,
        /// Resolved rule returned by the selector that chose this declaration.
        match_rule: String,
    },
}

pub fn preview_client_certificate(
    request: &probe_core::Request,
    resolution: &RequestResolution<'_>,
) -> CertificatePreview {
    use CertificatePreview::*;
    if request.kind.is_websocket() {
        return Hidden;
    }
    if matches!(
        resolution.certificate_override,
        Some(CertificateOverride::NoCertificate)
    ) {
        return NoCertificate;
    }
    let scopes = CertificateScopes::new(resolution);
    let selected = match resolution.certificate_override {
        Some(CertificateOverride::Selected(reference)) => {
            let Some(declaration) = scopes.find_declaration(reference) else {
                return Invalid;
            };
            Some(declaration)
        }
        _ if scopes.environment.is_empty() && scopes.collection.is_empty() => return Hidden,
        _ => None,
    };
    let url = match preview_target(request, resolution) {
        Ok(Some(url)) => url,
        Ok(None) => return if selected.is_some() { Invalid } else { Hidden },
        Err(()) => return Unknown,
    };
    let resolve = |value: &str| resolve_preview_value(value, resolution);
    let declaration = if let Some((index, certificate)) = selected {
        match matching_domain(certificate, &url, resolve) {
            Ok(Some(domain)) => Ok(Some((index, certificate, domain))),
            Ok(None) => return Invalid,
            Err(_) => return Unknown,
        }
    } else {
        scopes.select_declaration(url.as_str(), resolve)
    };
    match declaration {
        Ok(Some((index, certificate, match_rule))) => Selected {
            certificate: certificate.clone(),
            match_rule,
            environment: (index < scopes.environment.len())
                .then(|| resolution.environment.unwrap_or_default().to_owned()),
        },
        Ok(None) => Unmatched,
        Err(_) => Unknown,
    }
}

/// Enumerate in source order, resolving the target once and each enabled domain
/// at most once. Unknown domains are omitted without credential or key access.
pub fn certificate_candidates(
    request: &probe_core::Request,
    resolution: &RequestResolution<'_>,
) -> Vec<CertificateRef> {
    let Ok(Some(url)) = preview_target(request, resolution) else {
        return Vec::new();
    };
    let scopes = CertificateScopes::new(resolution);
    scopes
        .environment
        .iter()
        .chain(scopes.collection)
        .enumerate()
        .filter_map(|(index, certificate)| {
            matching_domain(certificate, &url, |value| {
                resolve_preview_value(value, resolution)
            })
            .ok()
            .flatten()
            .map(|_| CertificateRef {
                certificate: certificate.clone(),
                environment: (index < scopes.environment.len())
                    .then(|| resolution.environment.unwrap_or_default().to_owned()),
            })
        })
        .collect()
}

fn resolve_preview_value(
    value: &str,
    resolution: &RequestResolution<'_>,
) -> Result<String, HttpError> {
    probe_core::resolve_environment_for_templates_with_provider(
        [value],
        resolution.environments,
        resolution.environment,
        resolution.overrides,
        &crate::NoSecrets,
        None,
    )
    .and_then(|environment| environment.interpolate_strict(value))
    .map_err(|_| HttpError::ClientConfiguration("Selection unavailable".into()))
}

// None is a known non-HTTPS target; Err means public resolution is unavailable.
fn preview_target(
    request: &probe_core::Request,
    resolution: &RequestResolution<'_>,
) -> Result<Option<url::Url>, ()> {
    if request.kind.is_websocket() {
        return Ok(None);
    }
    let raw = request.url.as_deref().unwrap_or_default();
    if raw
        .trim()
        .split_once("://")
        .is_some_and(|(scheme, _)| !scheme.contains("{{") && !scheme.eq_ignore_ascii_case("https"))
    {
        return Ok(None);
    }
    // Resolve only fields that participate in the target URL, through the same
    // request transformer as execution (including parameter names).
    let target = probe_core::Request {
        url: request.url.clone(),
        path_parameters: request
            .path_parameters
            .iter()
            .filter(|parameter| !parameter.disabled)
            .cloned()
            .collect(),
        ..Default::default()
    };
    let templates = std::iter::once(raw).chain(
        target
            .path_parameters
            .iter()
            .flat_map(|parameter| [parameter.name.as_str(), parameter.value.as_str()]),
    );
    let Ok(environment) = probe_core::resolve_environment_for_templates_with_provider(
        templates,
        resolution.environments,
        resolution.environment,
        resolution.overrides,
        &crate::NoSecrets,
        None,
    ) else {
        return Err(());
    };
    let Ok(target) = probe_core::resolve_request_strict(&target, &environment) else {
        return Err(());
    };
    let url = probe_core::apply_path_parameters(
        target.url.as_deref().unwrap_or_default(),
        &target.path_parameters,
    );
    let Ok(parsed) = url::Url::parse(&url) else {
        return Err(());
    };
    if parsed.scheme() != "https" {
        return Ok(None);
    }
    Ok(Some(parsed))
}

#[cfg(test)]
mod preview_tests {
    use super::*;
    use probe_core::{ClientCertificate, EnvironmentVariable, Request, SecretVariable};

    fn fixture() -> probe_opencollection::LoadedWorkspace {
        probe_opencollection::load_workspace_from_str(include_str!(
            "../../../tests/fixtures/opencollection/mtls-variables.yml"
        ))
        .unwrap()
    }

    #[test]
    fn preview_uses_execution_precedence_and_ignores_unreachable_material_secrets() {
        let loaded = fixture();
        let workspace = loaded.workspace();
        let request = workspace.request(loaded.requests()[0].key()).unwrap();
        let resolution = RequestResolution {
            environments: workspace.environments(),
            environment: Some("child"),
            client_certificates: workspace.client_certificates(),
            ..Default::default()
        };
        let CertificatePreview::Selected {
            certificate,
            environment,
            match_rule,
        } = preview_client_certificate(request, &resolution)
        else {
            panic!("collection must win; child does not inherit certificates");
        };
        assert!(environment.is_none());
        assert_eq!(
            match_rule, "*.example.com",
            "the resolved rule, not its template, is presented"
        );
        assert_eq!(certificate, workspace.client_certificates()[2]);
        let CertificatePreview::Selected {
            certificate,
            environment,
            match_rule,
        } = preview_client_certificate(
            request,
            &RequestResolution {
                environment: Some("parent"),
                ..resolution
            },
        )
        else {
            panic!("direct environment must win");
        };
        assert_eq!(environment.as_deref(), Some("parent"));
        assert_eq!(match_rule, "api.example.com");
        assert_eq!(
            certificate,
            workspace.environments()[0].client_certificates[0]
        );
    }

    #[test]
    fn preview_distinguishes_hidden_unmatched_and_unknown_and_tracks_input_changes() {
        let loaded = fixture();
        let mut environments = loaded.workspace().environments().to_vec();
        environments[1]
            .variables
            .push(EnvironmentVariable::Secret(SecretVariable {
                name: Some("host".into()),
                value_type: None,
                disabled: false,
            }));
        let mut request = Request {
            url: Some("https://{{host}}/:resource".into()),
            ..Default::default()
        };
        let certificates = [ClientCertificate::Pem {
            domain: "api.example.com".into(),
            certificate_file_path: "missing.pem".into(),
            private_key_file_path: "{{unused}}".into(),
            passphrase: Some("{{unused}}".into()),
            disabled: false,
        }];
        let resolution = RequestResolution {
            environments: &environments,
            environment: Some("child"),
            client_certificates: &certificates,
            ..Default::default()
        };
        assert!(matches!(
            preview_client_certificate(&request, &resolution),
            CertificatePreview::Unknown
        ));
        request.url = Some("https://{{undefined}}".into());
        assert!(matches!(
            preview_client_certificate(&request, &resolution),
            CertificatePreview::Unknown
        ));
        request.url = Some("https://api.example.com/:resource".into());
        request.path_parameters.push(probe_core::QueryParameter {
            name: "resource".into(),
            value: "users".into(),
            disabled: false,
        });
        assert!(matches!(
            preview_client_certificate(&request, &resolution),
            CertificatePreview::Selected {
                environment: None,
                ..
            }
        ));
        request.path_parameters[0].value = "{{host}}".into();
        assert!(matches!(
            preview_client_certificate(&request, &resolution),
            CertificatePreview::Unknown
        ));
        request.path_parameters[0].disabled = true;
        assert!(matches!(
            preview_client_certificate(&request, &resolution),
            CertificatePreview::Selected { .. }
        ));
        request.path_parameters.clear();
        request.url = Some("https://other.test".into());
        assert!(matches!(
            preview_client_certificate(&request, &resolution),
            CertificatePreview::Unmatched
        ));
        for url in [
            "http://api.example.com",
            "http://{{host}}",
            "wss://api.example.com",
        ] {
            request.url = Some(url.into());
            assert!(matches!(
                preview_client_certificate(&request, &resolution),
                CertificatePreview::Hidden
            ));
        }
        request.url = Some("https://api.example.com".into());
        assert!(matches!(
            preview_client_certificate(&request, &RequestResolution::default()),
            CertificatePreview::Hidden
        ));
        request.kind = probe_core::RequestKind::WebSocket { message: None };
        assert!(matches!(
            preview_client_certificate(&request, &resolution),
            CertificatePreview::Hidden
        ));
        request.kind = Default::default();
        let mut secret_domain = certificates.clone();
        if let ClientCertificate::Pem { domain, .. } = &mut secret_domain[0] {
            *domain = "{{host}}".into();
        }
        assert!(matches!(
            preview_client_certificate(
                &request,
                &RequestResolution {
                    client_certificates: &secret_domain,
                    ..resolution
                }
            ),
            CertificatePreview::Unknown
        ));
        let mut disabled = certificates.clone();
        if let ClientCertificate::Pem { disabled, .. } = &mut disabled[0] {
            *disabled = true;
        }
        assert!(matches!(
            preview_client_certificate(
                &request,
                &RequestResolution {
                    client_certificates: &disabled,
                    ..resolution
                }
            ),
            CertificatePreview::Unmatched
        ));
    }
}

#[cfg(test)]
mod override_tests {
    use super::*;
    use probe_core::{ClientCertificate, EnvironmentVariable, Request, SecretVariable};

    fn pem(path: &str) -> ClientCertificate {
        ClientCertificate::Pem {
            domain: "localhost:8443".into(),
            certificate_file_path: path.into(),
            private_key_file_path: "key.pem".into(),
            passphrase: None,
            disabled: false,
        }
    }

    #[test]
    fn overrides_select_exact_scope_survive_reordering_and_never_fall_back() {
        let loaded = probe_opencollection::load_workspace_from_str(include_str!(
            "../../../tests/fixtures/opencollection/mtls-variables.yml"
        ))
        .unwrap();
        let mut environments = loaded.workspace().environments().to_vec();
        environments[0].client_certificates = vec![pem("environment.pem")];
        let mut certificates = vec![pem("first.pem"), pem("second.pem")];
        let request = Request {
            url: Some("https://localhost:8443/".into()),
            ..Default::default()
        };
        let reference = CertificateRef {
            certificate: certificates[1].clone(),
            environment: None,
        };
        let selection = CertificateOverride::Selected(reference.clone());
        let base = RequestResolution {
            environments: &environments,
            environment: Some("parent"),
            ..Default::default()
        };
        let check =
            |certificates: &[ClientCertificate], selection: &CertificateOverride, url: &str| {
                let resolution = RequestResolution {
                    certificate_override: Some(selection),
                    client_certificates: certificates,
                    ..base
                };
                CertificateScopes::new(&resolution)
                    .select_declaration(url, |value| Ok(value.to_owned()))
                    .map(|result| result.map(|(_, certificate, _)| certificate.clone()))
            };
        let url = request.url.as_deref().unwrap();
        assert_eq!(
            check(&certificates, &CertificateOverride::Automatic, url).unwrap(),
            Some(pem("environment.pem"))
        );
        let candidates = certificate_candidates(
            &request,
            &RequestResolution {
                client_certificates: &certificates,
                ..base
            },
        );
        assert_eq!(candidates.len(), 3);
        assert_eq!(candidates[0].environment.as_deref(), Some("parent"));
        for reference in candidates {
            let selected = CertificateOverride::Selected(reference.clone());
            let preview = preview_client_certificate(
                &request,
                &RequestResolution {
                    certificate_override: Some(&selected),
                    client_certificates: &certificates,
                    ..base
                },
            );
            assert!(
                matches!(preview, CertificatePreview::Selected { certificate, environment, .. }
                if certificate == reference.certificate && environment == reference.environment)
            );
            assert_eq!(
                check(
                    &certificates,
                    &CertificateOverride::Selected(reference.clone()),
                    url
                )
                .unwrap(),
                Some(reference.certificate)
            );
        }
        certificates.reverse();
        assert_eq!(
            check(&certificates, &selection, url).unwrap(),
            Some(reference.certificate.clone())
        );
        for url in [
            "https://elsewhere.test:8443/",
            "https://localhost:443/",
            "http://localhost:8443/",
        ] {
            assert!(check(&certificates, &selection, url).is_err());
        }
        // An identical declaration in the current environment cannot stand in
        // for one selected from a different environment.
        let changed_environment = CertificateOverride::Selected(CertificateRef {
            environment: Some("child".into()),
            certificate: environments[0].client_certificates[0].clone(),
        });
        assert!(check(&certificates, &changed_environment, url).is_err());
        certificates.remove(0);
        assert!(check(&certificates, &selection, url).is_err());
        let mut changed = reference.certificate.clone();
        if let ClientCertificate::Pem { disabled, .. } = &mut changed {
            *disabled = true;
        }
        certificates.push(changed);
        assert!(check(&certificates, &selection, url).is_err());
        if let ClientCertificate::Pem {
            certificate_file_path,
            disabled,
            ..
        } = certificates.last_mut().unwrap()
        {
            *disabled = false;
            *certificate_file_path = "replacement.pem".into();
        }
        assert!(check(&certificates, &selection, url).is_err());
        assert!(matches!(
            preview_client_certificate(
                &request,
                &RequestResolution {
                    certificate_override: Some(&selection),
                    client_certificates: &certificates,
                    ..base
                }
            ),
            CertificatePreview::Invalid
        ));
        assert_eq!(
            check(&certificates, &CertificateOverride::NoCertificate, url).unwrap(),
            None
        );
    }

    #[test]
    fn no_certificate_does_not_resolve_domains_or_material_and_candidates_omit_unknown_domains() {
        let loaded = probe_opencollection::load_workspace_from_str(include_str!(
            "../../../tests/fixtures/opencollection/mtls-variables.yml"
        ))
        .unwrap();
        let mut environments = loaded.workspace().environments().to_vec();
        environments[1]
            .variables
            .push(EnvironmentVariable::Secret(SecretVariable {
                name: Some("secretDomain".into()),
                value_type: None,
                disabled: false,
            }));
        let mut certificates = vec![pem("first.pem"), pem("second.pem"), pem("disabled.pem")];
        if let ClientCertificate::Pem { domain, .. } = &mut certificates[0] {
            *domain = "{{secretDomain}}".into();
        }
        if let ClientCertificate::Pem { disabled, .. } = &mut certificates[2] {
            *disabled = true;
        }
        let request = Request {
            url: Some("https://localhost:8443".into()),
            ..Default::default()
        };
        let resolution = RequestResolution {
            environments: &environments,
            environment: Some("child"),
            client_certificates: &certificates,
            certificate_override: Some(&CertificateOverride::NoCertificate),
            ..Default::default()
        };
        let prepared = request.clone().into_http().unwrap();
        assert!(
            CertificateScopes::new(&resolution)
                .select(&prepared, |_| panic!(
                    "No Certificate must skip all certificate resolution"
                ))
                .unwrap()
                .is_none()
        );
        assert!(matches!(
            preview_client_certificate(&request, &resolution),
            CertificatePreview::NoCertificate
        ));
        assert_eq!(
            certificate_candidates(&request, &resolution),
            vec![CertificateRef {
                certificate: certificates[1].clone(),
                environment: None
            }]
        );
        let mut changed = request;
        changed.kind = probe_core::RequestKind::WebSocket { message: None };
        assert!(matches!(
            preview_client_certificate(&changed, &resolution),
            CertificatePreview::Hidden
        ));
        changed.kind = Default::default();
        changed.url = Some("https://{{secretDomain}}".into());
        assert!(certificate_candidates(&changed, &resolution).is_empty());
    }
}
