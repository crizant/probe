//! Request execution shared by every interface.
//!
//! Preparation resolves one request into an execution request that carries runtime
//! secret material, a presentation request that keeps secret references, and a
//! private disclosure context used only to redact what execution returns.

use std::{cell::RefCell, collections::BTreeMap, fmt, future::Future, path::Path};

use probe_core::{
    AuthenticationKind, AuthenticationValue, Environment, EnvironmentResolutionError,
    PreparedHttpRequest, Request, RequestProtocolError, ResolvedEnvironment, SecretContext,
    SecretError, SecretProvider, SecretValue, VariableUsage, request_secret_usages,
    resolve_environment_for_request_with_provider, resolve_request,
    resolve_request_for_presentation, resolve_request_strict,
};
use probe_http::{ExecutionOptions, HttpEngine, HttpError, HttpProgress, HttpResponse};

use crate::client_certificate::CertificateScopes;

/// Diagnostic reported instead of HTTP failure details when a runtime secret was used.
pub const SECRET_DIAGNOSTIC_WITHHELD: &str =
    "diagnostic withheld because the request used a secret variable";

/// Environment inputs for resolving one request.
///
/// Variables are resolved only when an environment is selected, overrides are
/// supplied, or strict resolution is requested; otherwise templates stay literal
/// and no provider is consulted. `Debug` is deliberately absent because an override
/// may supply a declared secret.
#[derive(Clone, Copy, Default)]
pub struct RequestResolution<'a> {
    /// Certificates declared at collection scope, in source order.
    pub client_certificates: &'a [probe_core::ClientCertificate],
    pub environments: &'a [Environment],
    pub environment: Option<&'a str>,
    pub overrides: &'a [(String, String)],
    pub strict_variables: bool,
    /// Passed to providers that scope values by workspace.
    pub workspace_identity: Option<&'a str>,
}

impl RequestResolution<'_> {
    const fn resolves_variables(&self) -> bool {
        self.environment.is_some() || !self.overrides.is_empty() || self.strict_variables
    }
}

/// A provider for interfaces or invocations without a secret backend.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoSecrets;

impl SecretProvider for NoSecrets {
    fn resolve_secret(&self, _: &SecretContext<'_>) -> Result<Option<SecretValue>, SecretError> {
        Ok(None)
    }
}

/// Resolves `request` for execution and presentation.
///
/// The provider is asked only for secrets the request can reach, including through
/// plain variables. It may block, so interactive interfaces must call this away from
/// their UI thread.
pub fn prepare_request<'a>(
    request: &Request,
    resolution: &RequestResolution<'a>,
    secrets: &'a dyn SecretProvider,
) -> Result<PreparedRequest<'a>, EnvironmentResolutionError> {
    let secrets = CachedSecrets::new(secrets);
    if !resolution.resolves_variables() {
        return Ok(PreparedRequest {
            execution: request.clone(),
            presentation: request.clone(),
            disclosure: SecretDisclosure {
                presentation_url: request.url.clone().unwrap_or_default(),
                ..SecretDisclosure::default()
            },
            resolution: *resolution,
            provider: secrets,
        });
    }
    let environment = resolve_environment_for_request_with_provider(
        request,
        resolution.environments,
        resolution.environment,
        resolution.overrides,
        &secrets,
        resolution.workspace_identity,
    )?;
    let execution = if resolution.strict_variables {
        resolve_request_strict(request, &environment)
    } else {
        resolve_request(request, &environment)
    }?;
    let presentation =
        resolve_request_for_presentation(request, &environment, resolution.strict_variables)?;
    let disclosure = SecretDisclosure::new(environment, presentation.url.as_deref());
    Ok(PreparedRequest {
        execution,
        presentation,
        disclosure,
        resolution: *resolution,
        provider: secrets,
    })
}

/// Each reachable provider value (or failure) is read at most once per preparation,
/// including when request and certificate fields share nested variable references.
pub(crate) struct CachedSecrets<'a> {
    provider: &'a dyn SecretProvider,
    values: RefCell<BTreeMap<String, Result<Option<SecretValue>, SecretError>>>,
}

impl<'a> CachedSecrets<'a> {
    pub(crate) fn new(provider: &'a dyn SecretProvider) -> Self {
        Self {
            provider,
            values: RefCell::new(BTreeMap::new()),
        }
    }
}

impl SecretProvider for CachedSecrets<'_> {
    fn resolve_secret(
        &self,
        context: &SecretContext<'_>,
    ) -> Result<Option<SecretValue>, SecretError> {
        self.values
            .borrow_mut()
            .entry(context.variable_name.to_owned())
            .or_insert_with(|| self.provider.resolve_secret(context))
            .clone()
    }
}

/// A resolved request whose secret-bearing execution form is not observable.
pub struct PreparedRequest<'a> {
    resolution: RequestResolution<'a>,
    provider: CachedSecrets<'a>,
    execution: Request,
    presentation: Request,
    disclosure: SecretDisclosure,
}

impl<'a> PreparedRequest<'a> {
    /// The request with secret references retained, safe for dry runs and summaries.
    #[must_use]
    pub const fn presentation(&self) -> &Request {
        &self.presentation
    }

    /// Whether runtime secret material was resolved for this request.
    #[must_use]
    pub const fn uses_secrets(&self) -> bool {
        self.disclosure.uses_secrets()
    }

    /// Converts the execution request into HTTP engine input.
    ///
    /// Resolves reachable certificate variables, including secrets, without reading
    /// identity files. The provider may block; call this away from the UI thread.
    pub fn into_http(self) -> Result<HttpExecution, RequestProtocolError> {
        self.into_http_parts().map(|(execution, _, _)| execution)
    }

    /// Executes HTTP/GraphQL assertions against original response bytes, returning
    /// only value-free results and the redacted response. Uses the preparation's
    /// cached provider for the entire batch, including request/certificate secrets.
    pub async fn execute_with_assertions<C, P>(
        self,
        engine: &HttpEngine,
        options: ExecutionOptions,
        output: Option<&Path>,
        cancellation: C,
        progress: P,
        assertions: &[probe_core::Assertion],
    ) -> Result<(ExecutedResponse, Vec<crate::AssertionResult>), HttpError>
    where
        C: Future + Send,
        P: FnMut(HttpProgress) + Send,
    {
        let (execution, resolution, provider) = self
            .into_http_parts()
            .map_err(|error| HttpError::InvalidRequest(error.to_string()))?;
        let mut options = options;
        if !assertions.is_empty() {
            // Assertion-only secrets are resolved after the exchange; prevent raw
            // bytes from reaching the response spool cache in the meantime.
            options.response_cache = None;
        }
        let (mut executed, disclosure) = execution
            .execute_raw(engine, options, output, cancellation, progress)
            .await?;
        let mut environments = Vec::new();
        let outcomes = crate::assertions::evaluate_with_disclosure(
            assertions,
            &executed.response,
            &resolution,
            &provider,
            |environment| environments.push(environment),
        );
        if environments
            .iter()
            .any(ResolvedEnvironment::has_resolved_secrets)
        {
            executed
                .response
                .url
                .clone_from(&disclosure.presentation_url);
            executed.response.url_changed = false;
        }
        executed.response = disclosure.redact_response(executed.response);
        for environment in environments {
            let url = environment.redact_secrets(&executed.response.url);
            executed.response =
                SecretDisclosure::new(environment, Some(&url)).redact_response(executed.response);
        }
        Ok((executed, outcomes))
    }

    fn into_http_parts(
        mut self,
    ) -> Result<(HttpExecution, RequestResolution<'a>, CachedSecrets<'a>), RequestProtocolError>
    {
        let mut templates = Vec::new();
        if self.resolution.resolves_variables() {
            // Seed reachability from the presentation request's retained references,
            // so rebuilding the environment for certificate fields retains request
            // disclosure.
            templates = probe_core::discover_request_variables(
                &self.presentation,
                self.resolution.environments,
                self.resolution.environment,
            )
            .expect("prepared request references were validated during resolution")
            .into_iter()
            .map(|variable| format!("{{{{{}}}}}", variable.name))
            .collect();
        }
        let request = self.execution.into_http()?;
        let client_identity = CertificateScopes::new(&self.resolution).select(&request, |value| {
            if !self.resolution.resolves_variables() {
                return Ok(value.to_owned());
            }
            templates.push(value.to_owned());
            let environment = probe_core::resolve_environment_for_templates_with_provider(
                templates.iter().map(String::as_str),
                self.resolution.environments,
                self.resolution.environment,
                self.resolution.overrides,
                &self.provider,
                self.resolution.workspace_identity,
            )
            .map_err(|error| HttpError::ClientConfiguration(error.to_string()))?;
            let result = if self.resolution.strict_variables {
                environment.interpolate_strict(value)
            } else {
                environment.interpolate(value)
            };
            self.disclosure = SecretDisclosure::new(environment, self.presentation.url.as_deref());
            result.map_err(|error| HttpError::ClientConfiguration(error.to_string()))
        });
        if let Some(environment) = &self.disclosure.secrets {
            self.disclosure.initial_url_uses_secrets =
                initial_url_depends_on_secret(self.presentation, request.request(), environment)?;
        }
        Ok((
            HttpExecution {
                client_identity,
                request,
                disclosure: self.disclosure,
            },
            self.resolution,
            self.provider,
        ))
    }
}

/// Classifies initial URL values and structure using symbolic secret provenance.
fn initial_url_depends_on_secret(
    mut presentation: Request,
    execution: &Request,
    environment: &ResolvedEnvironment,
) -> Result<bool, RequestProtocolError> {
    let method_controls_url = presentation.kind.is_graphql();
    let presentation_method = presentation.method.clone();
    // Use the actual method so GraphQL GET fields become query parameters
    // even when the method itself was resolved from a secret.
    presentation.method.clone_from(&execution.method);
    let mut presentation = presentation.into_http()?.request().clone();
    // Scan the symbolic method, since GraphQL URL construction depends on
    // it even when the generated query parameters contain no secret values.
    presentation.method = presentation_method;
    presentation
        .query_parameters
        .retain(|parameter| !parameter.disabled);
    presentation
        .path_parameters
        .retain(|parameter| !parameter.disabled);
    let usages = request_secret_usages(&presentation, environment)
        .expect("prepared presentation references were validated during resolution");
    let api_key_auth = execution
        .authentication
        .as_ref()
        .filter(|auth| auth.kind == AuthenticationKind::ApiKey);
    let query_auth = api_key_auth.is_some_and(|auth| {
        auth.properties.get("placement") == Some(&AuthenticationValue::String("query".to_owned()))
    });
    Ok(usages.iter().any(|usage| {
        matches!(
            usage,
            VariableUsage::Url
                | VariableUsage::PathParameter { .. }
                | VariableUsage::QueryParameter { .. }
        ) || (query_auth && matches!(usage, VariableUsage::Authentication { .. }))
            || (api_key_auth.is_some()
                && matches!(usage, VariableUsage::Authentication { name } if name == "placement"))
            || (method_controls_url && matches!(usage, VariableUsage::Method))
    }))
}

impl fmt::Debug for PreparedRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedRequest")
            .field("presentation", &self.presentation)
            .field("uses_secrets", &self.uses_secrets())
            .finish_non_exhaustive()
    }
}

/// HTTP engine input whose results are redacted before they are returned.
pub struct HttpExecution {
    client_identity: Result<Option<probe_http::ClientIdentity>, HttpError>,
    request: PreparedHttpRequest,
    disclosure: SecretDisclosure,
}

impl HttpExecution {
    /// Whether runtime secret material was resolved for this request.
    #[must_use]
    pub const fn uses_secrets(&self) -> bool {
        self.disclosure.uses_secrets()
    }

    /// Executes the request in memory, or streams the original response bytes to `output`.
    ///
    /// When a secret was used, the response cache is bypassed, the returned response
    /// is redacted, its final URL is the presentation URL, and failure diagnostics
    /// are withheld while the failure kind is kept. The initial URL is replaced
    /// only when URL values or construction are secret-derived.
    pub async fn execute<C, P>(
        self,
        engine: &HttpEngine,
        options: ExecutionOptions,
        output: Option<&Path>,
        cancellation: C,
        progress: P,
    ) -> Result<ExecutedResponse, HttpError>
    where
        C: Future + Send,
        P: FnMut(HttpProgress) + Send,
    {
        let (mut executed, disclosure) = self
            .execute_raw(engine, options, output, cancellation, progress)
            .await?;
        executed.response = disclosure.redact_response(executed.response);
        Ok(executed)
    }

    // Only application code can observe the original response or its disclosure
    // context. Keeping assertion inputs out of this future preserves the desktop's
    // Send execution contract with providers that are deliberately not Sync.
    async fn execute_raw<C, P>(
        self,
        engine: &HttpEngine,
        mut options: ExecutionOptions,
        output: Option<&Path>,
        cancellation: C,
        progress: P,
    ) -> Result<(ExecutedResponse, SecretDisclosure), HttpError>
    where
        C: Future + Send,
        P: FnMut(HttpProgress) + Send,
    {
        // Selection errors contain only fixed configuration diagnostics or typed
        // resolver errors (variable names, never resolved values). Preserve these
        // safe errors.
        options.client_identity = self.client_identity?;
        if self.disclosure.uses_secrets() {
            // Cache spool files would retain unredacted response bytes.
            options.response_cache = None;
        }
        let result = match output {
            Some(output) => engine
                .execute_cancellable_to_file_with_progress(
                    &self.request,
                    &options,
                    output,
                    cancellation,
                    progress,
                )
                .await
                .map(|streamed| ExecutedResponse {
                    response: streamed.response,
                    body_sha256: Some(streamed.body_sha256),
                }),
            None => engine
                .execute_cancellable_with_progress(&self.request, &options, cancellation, progress)
                .await
                .map(|response| ExecutedResponse {
                    response,
                    body_sha256: None,
                }),
        };
        match result {
            Ok(executed) => Ok((executed, self.disclosure)),
            Err(error) => Err(self.disclosure.redact_error(error)),
        }
    }
}

impl fmt::Debug for HttpExecution {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HttpExecution")
            .field("uses_secrets", &self.uses_secrets())
            .finish_non_exhaustive()
    }
}

/// A completed execution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutedResponse {
    /// Response metadata and bounded body, redacted when a secret was used.
    pub response: HttpResponse,
    /// SHA-256 of the complete, unredacted bytes written to the requested output file.
    pub body_sha256: Option<[u8; 32]>,
}

/// Secret-bearing context retained only to redact execution results.
#[derive(Default)]
struct SecretDisclosure {
    secrets: Option<ResolvedEnvironment>,
    presentation_url: String,
    initial_url_uses_secrets: bool,
}

impl SecretDisclosure {
    fn new(environment: ResolvedEnvironment, presentation_url: Option<&str>) -> Self {
        let has_secrets = environment.has_resolved_secrets();
        Self {
            secrets: has_secrets.then_some(environment),
            initial_url_uses_secrets: false,
            presentation_url: presentation_url.unwrap_or_default().to_owned(),
        }
    }

    const fn uses_secrets(&self) -> bool {
        self.secrets.is_some()
    }

    fn redact_response(&self, mut response: HttpResponse) -> HttpResponse {
        let Some(secrets) = &self.secrets else {
            return response;
        };
        // A redirect may encode or transform secret bytes, so only the
        // reference-preserving URL is safe to present.
        if self.initial_url_uses_secrets {
            response.initial_url.clone_from(&self.presentation_url);
        } else {
            response.initial_url = secrets.redact_secrets(&response.initial_url);
        }
        response.url.clone_from(&self.presentation_url);
        response.url_changed = false;
        response.reason = secrets.redact_secrets(&response.reason);
        for header in &mut response.headers {
            header.name = secrets.redact_secrets(&header.name);
            header.value = secrets.redact_secrets(&header.value);
        }
        response.body = secrets.redact_secret_bytes(&response.body);
        response
    }

    fn redact_error(&self, error: HttpError) -> HttpError {
        if !self.uses_secrets() {
            return error;
        }
        match error {
            HttpError::MissingMethod
            | HttpError::MissingUrl
            | HttpError::Timeout
            | HttpError::Cancelled
            | HttpError::ClientCertificateRedirectBlocked
            | HttpError::Pkcs12(_) => error,
            HttpError::ResponseOutput { path, .. } => HttpError::ResponseOutput {
                path,
                message: SECRET_DIAGNOSTIC_WITHHELD.to_owned(),
            },
            error if error.is_configuration() => {
                HttpError::InvalidRequest(SECRET_DIAGNOSTIC_WITHHELD.to_owned())
            }
            _ => HttpError::Transport(SECRET_DIAGNOSTIC_WITHHELD.to_owned()),
        }
    }
}

/// Exports the current request, resolving plain variables while retaining secret placeholders.
/// No credential backend is consulted and no files are read or requests sent.
pub fn copy_as_curl(
    request: &Request,
    resolution: &RequestResolution<'_>,
    engine: &HttpEngine,
    options: &ExecutionOptions,
) -> Result<String, String> {
    // Symbolic values let the shared resolver classify secret-derived variables
    // without consulting credentials. Export only the presentation request.
    struct SecretPlaceholders;
    impl SecretProvider for SecretPlaceholders {
        fn resolve_secret(
            &self,
            context: &SecretContext<'_>,
        ) -> Result<Option<SecretValue>, SecretError> {
            Ok(Some(SecretValue::new(format!(
                "{{{{{}}}}}",
                context.variable_name
            ))))
        }
    }
    let request = prepare_request(request, resolution, &SecretPlaceholders)
        .map_err(|error| error.to_string())?
        .presentation;
    let prepared = request.into_http().map_err(|error| error.to_string())?;
    engine
        .curl_command(&prepared, options)
        .map_err(|error| error.to_string())
}

#[path = "session.rs"]
mod session;
pub use session::{
    CloseOrigin, Session, SessionData, SessionError, SessionEvent, SessionSender,
    WebSocketExecution,
};

#[cfg(test)]
mod certificate_variable_tests {
    use super::*;
    use probe_core::ClientCertificate;

    const PASSWORD: &str = "certificate-password-marker";

    #[derive(Default)]
    struct Provider {
        calls: RefCell<Vec<String>>,
        unavailable: bool,
        failure: bool,
    }
    impl SecretProvider for Provider {
        fn resolve_secret(
            &self,
            context: &SecretContext<'_>,
        ) -> Result<Option<SecretValue>, SecretError> {
            assert_eq!(context.environment_name, Some("child"));
            assert_eq!(context.workspace_identity, Some("workspace"));
            self.calls.borrow_mut().push(context.variable_name.into());
            assert_eq!(context.variable_name, "certPassword");
            if self.failure {
                return Err(SecretError);
            }
            Ok((!self.unavailable).then(|| SecretValue::new(PASSWORD.into())))
        }
    }

    fn fixture() -> probe_opencollection::LoadedWorkspace {
        probe_opencollection::load_workspace_from_str(include_str!(
            "../../../tests/fixtures/opencollection/mtls-variables.yml"
        ))
        .unwrap()
    }

    fn resolution(loaded: &probe_opencollection::LoadedWorkspace) -> RequestResolution<'_> {
        RequestResolution {
            environments: loaded.workspace().environments(),
            client_certificates: loaded.workspace().client_certificates(),
            environment: Some("child"),
            workspace_identity: Some("workspace"),
            ..RequestResolution::default()
        }
    }

    fn request() -> Request {
        Request {
            method: Some("GET".into()),
            url: Some("https://api.example.com/private".into()),
            ..Request::default()
        }
    }

    #[test]
    fn inherited_nested_variables_resolve_only_selected_certificate_and_join_disclosure() {
        let loaded = fixture();
        let provider = Provider::default();
        let prepared = prepare_request(&request(), &resolution(&loaded), &provider).unwrap();
        assert!(!prepared.uses_secrets());
        assert!(provider.calls.borrow().is_empty());
        assert!(!format!("{prepared:?}").contains(PASSWORD));
        let execution = prepared.into_http().unwrap();
        assert!(execution.uses_secrets());
        assert_eq!(*provider.calls.borrow(), ["certPassword"]);
        assert_eq!(
            execution.client_identity,
            Ok(Some(probe_http::ClientIdentity::new(
                "*.example.com".into(),
                "inherited/client.pem".into(),
                "inherited/client-key.pem".into(),
                Some(PASSWORD.into()),
            )))
        );
        assert!(!format!("{execution:?}").contains(PASSWORD));
        let error = execution
            .disclosure
            .redact_error(HttpError::ClientConfiguration(PASSWORD.into()));
        assert_eq!(
            error,
            HttpError::InvalidRequest(SECRET_DIAGNOSTIC_WITHHELD.into())
        );
        assert_eq!(
            execution
                .disclosure
                .secrets
                .as_ref()
                .unwrap()
                .redact_secrets(PASSWORD),
            "[REDACTED]"
        );
    }

    #[test]
    fn invocation_overrides_apply_to_domain_paths_and_secret_passphrase() {
        let loaded = fixture();
        let provider = Provider::default();
        let overrides = [
            ("mtlsDomain".into(), "api.example.com:443".into()),
            ("directory".into(), "override".into()),
            ("certPassword".into(), "override-password".into()),
        ];
        let prepared = prepare_request(
            &request(),
            &RequestResolution {
                overrides: &overrides,
                ..resolution(&loaded)
            },
            &provider,
        )
        .unwrap();
        let execution = prepared.into_http().unwrap();
        assert_eq!(
            execution.client_identity,
            Ok(Some(probe_http::ClientIdentity::new(
                "api.example.com:443".into(),
                "override/client.pem".into(),
                "override/client-key.pem".into(),
                Some("override-password".into()),
            )))
        );
        assert!(execution.uses_secrets());
        assert!(provider.calls.borrow().is_empty());
    }

    #[test]
    fn request_and_certificate_shared_secrets_are_read_once() {
        let loaded = fixture();
        let provider = Provider::default();
        let mut request = request();
        request.headers.push(probe_core::Header {
            name: "X-Secret".into(),
            value: "{{passwordAlias}}".into(),
            disabled: false,
        });
        let execution = prepare_request(&request, &resolution(&loaded), &provider)
            .unwrap()
            .into_http()
            .unwrap();
        assert!(execution.uses_secrets());
        assert_eq!(*provider.calls.borrow(), ["certPassword"]);
        assert!(execution.client_identity.is_ok());
        assert_eq!(
            execution
                .disclosure
                .secrets
                .as_ref()
                .unwrap()
                .redact_secrets(PASSWORD),
            "[REDACTED]"
        );
    }

    #[tokio::test]
    async fn unavailable_certificate_secret_fails_before_file_or_network_access() {
        let loaded = fixture();
        for failure in [false, true] {
            let provider = Provider {
                unavailable: true,
                failure,
                ..Provider::default()
            };
            let execution = prepare_request(&request(), &resolution(&loaded), &provider)
                .unwrap()
                .into_http()
                .unwrap();
            let error = execution
                .execute(
                    &HttpEngine::new().unwrap(),
                    ExecutionOptions::default(),
                    None,
                    std::future::pending::<()>(),
                    |_| {},
                )
                .await
                .unwrap_err();
            assert!(matches!(error, HttpError::ClientConfiguration(_)));
            let message = error.to_string();
            let diagnostic = if failure {
                "secret provider failed for variable: certPassword"
            } else {
                "secret variable has no runtime value: certPassword"
            };
            assert_eq!(
                message,
                format!("cannot configure HTTP client: {diagnostic}")
            );
            assert!(!message.contains(PASSWORD));
            assert!(!format!("{error:?}").contains(PASSWORD));
            assert_eq!(*provider.calls.borrow(), ["certPassword"]);
        }
    }

    #[test]
    fn presentation_and_curl_do_not_resolve_certificate_only_secrets_or_read_files() {
        let loaded = fixture();
        let provider = Provider::default();
        let prepared = prepare_request(&request(), &resolution(&loaded), &provider).unwrap();
        assert_eq!(prepared.presentation(), &request());
        let curl = copy_as_curl(
            &request(),
            &resolution(&loaded),
            &HttpEngine::new().unwrap(),
            &ExecutionOptions::default(),
        )
        .unwrap();
        assert!(curl.contains("https://api.example.com/private"));
        assert!(!curl.contains(PASSWORD));
        assert!(provider.calls.borrow().is_empty());
    }

    #[test]
    fn interpolation_preserves_domain_matching_and_selects_pkcs12() {
        let loaded = fixture();
        let provider = Provider::default();
        for host in [
            "example.com",
            "api.example.com.evil.test",
            "elsewhere.invalid",
        ] {
            let request = Request {
                url: Some(format!("https://{host}")),
                ..request()
            };
            // Remove the trailing enabled variable-domain entry: an unknown domain
            // necessarily needs its own secret to decide whether it matches.
            let certificates = &loaded.workspace().client_certificates()[..3];
            let execution = prepare_request(
                &request,
                &RequestResolution {
                    client_certificates: certificates,
                    ..resolution(&loaded)
                },
                &provider,
            )
            .unwrap()
            .into_http()
            .unwrap();
            assert_eq!(execution.client_identity, Ok(None));
        }
        assert!(provider.calls.borrow().is_empty());
        let certificates = [ClientCertificate::Pkcs12 {
            domain: "{{mtlsDomain}}".into(),
            pkcs12_file_path: "{{directory}}/client.p12".into(),
            passphrase: Some("{{passwordAlias}}".into()),
            disabled: false,
        }];
        let execution = prepare_request(
            &request(),
            &RequestResolution {
                client_certificates: &certificates,
                ..resolution(&loaded)
            },
            &provider,
        )
        .unwrap()
        .into_http()
        .unwrap();
        assert_eq!(
            execution.client_identity,
            Ok(Some(probe_http::ClientIdentity::pkcs12(
                "*.example.com".into(),
                "inherited/client.p12".into(),
                Some(PASSWORD.into())
            )))
        );
        assert_eq!(*provider.calls.borrow(), ["certPassword"]);
        assert!(execution.uses_secrets());
    }

    #[test]
    fn interpolated_domains_ignore_blank_values_and_accept_broad_patterns() {
        let loaded = fixture();
        let provider = Provider::default();
        let certificates = [ClientCertificate::Pem {
            domain: "{{mtlsDomain}}".into(),
            certificate_file_path: "missing-cert.pem".into(),
            private_key_file_path: "missing-key.pem".into(),
            passphrase: None,
            disabled: false,
        }];
        for (domain, host, matches) in [
            ("", "api.example.com", false),
            (" \t\r\n", "api.example.com", false),
            ("*.com", "api.example.com", true),
            ("*.com", "example.com", true),
            ("*.com", "com", false),
            ("*.com", "api.example.net", false),
        ] {
            let overrides = [("mtlsDomain".into(), domain.into())];
            let request = Request {
                url: Some(format!("https://{host}/private")),
                ..request()
            };
            let execution = prepare_request(
                &request,
                &RequestResolution {
                    client_certificates: &certificates,
                    overrides: &overrides,
                    strict_variables: true,
                    ..resolution(&loaded)
                },
                &provider,
            )
            .unwrap()
            .into_http()
            .unwrap();
            let expected = matches.then(|| {
                probe_http::ClientIdentity::new(
                    domain.into(),
                    "missing-cert.pem".into(),
                    "missing-key.pem".into(),
                    None,
                )
            });
            assert_eq!(
                execution.client_identity,
                Ok(expected),
                "domain={domain:?}, host={host}"
            );
        }
        assert!(provider.calls.borrow().is_empty());
    }

    #[tokio::test]
    async fn pkcs12_passphrase_participates_in_disclosure_and_safe_errors() {
        let loaded = fixture();
        let provider = Provider::default();
        let mut request = request();
        request.headers.push(probe_core::Header {
            name: "X-Secret".into(),
            value: "{{passwordAlias}}".into(),
            disabled: false,
        });
        let certificates = [ClientCertificate::Pkcs12 {
            domain: "{{mtlsDomain}}".into(),
            pkcs12_file_path: Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/mtls/client-protected.p12")
                .to_str()
                .unwrap()
                .into(),
            passphrase: Some("{{passwordAlias}}".into()),
            disabled: false,
        }];
        let execution = prepare_request(
            &request,
            &RequestResolution {
                client_certificates: &certificates,
                ..resolution(&loaded)
            },
            &provider,
        )
        .unwrap()
        .into_http()
        .unwrap();
        assert!(execution.uses_secrets());
        let error = execution
            .execute(
                &HttpEngine::new().unwrap(),
                ExecutionOptions::default(),
                None,
                std::future::pending::<()>(),
                |_| {},
            )
            .await
            .unwrap_err();
        assert_eq!(error, HttpError::Pkcs12(probe_http::Pkcs12Error::Integrity));
        assert_eq!(*provider.calls.borrow(), ["certPassword"]);
    }

    #[test]
    fn missing_certificate_variables_follow_strict_and_nonstrict_semantics() {
        let certificates = [ClientCertificate::Pem {
            domain: "{{missing}}".into(),
            certificate_file_path: "{{unused}}".into(),
            private_key_file_path: "unused".into(),
            passphrase: None,
            disabled: false,
        }];
        for strict_variables in [false, true] {
            let overrides = [("unrelated".into(), "value".into())];
            let execution = prepare_request(
                &request(),
                &RequestResolution {
                    client_certificates: &certificates,
                    overrides: &overrides,
                    strict_variables,
                    ..RequestResolution::default()
                },
                &NoSecrets,
            )
            .unwrap()
            .into_http()
            .unwrap();
            if strict_variables {
                assert!(matches!(
                    execution.client_identity,
                    Err(HttpError::ClientConfiguration(_))
                ));
            } else {
                assert_eq!(execution.client_identity, Ok(None));
            }
        }
    }
}
