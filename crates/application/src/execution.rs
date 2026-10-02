//! Request execution shared by every interface.
//!
//! Preparation resolves one request into an execution request that carries runtime
//! secret material, a presentation request that keeps secret references, and a
//! private disclosure context used only to redact what execution returns.

use std::{fmt, future::Future, path::Path};

use probe_core::{
    Environment, EnvironmentResolutionError, GraphqlRequestError, PreparedHttpRequest, Request,
    ResolvedEnvironment, SecretContext, SecretError, SecretProvider, SecretValue,
    resolve_environment_for_request_with_provider, resolve_request,
    resolve_request_for_presentation, resolve_request_strict,
};
use probe_http::{ExecutionOptions, HttpEngine, HttpError, HttpProgress, HttpResponse};

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
pub fn prepare_request(
    request: &Request,
    resolution: &RequestResolution<'_>,
    secrets: &dyn SecretProvider,
) -> Result<PreparedRequest, EnvironmentResolutionError> {
    if !resolution.resolves_variables() {
        return Ok(PreparedRequest {
            execution: request.clone(),
            presentation: request.clone(),
            disclosure: SecretDisclosure::default(),
        });
    }
    let environment = resolve_environment_for_request_with_provider(
        request,
        resolution.environments,
        resolution.environment,
        resolution.overrides,
        secrets,
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
    })
}

/// A resolved request whose secret-bearing execution form is not observable.
pub struct PreparedRequest {
    execution: Request,
    presentation: Request,
    disclosure: SecretDisclosure,
}

impl PreparedRequest {
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
    pub fn into_http(self) -> Result<HttpExecution, GraphqlRequestError> {
        Ok(HttpExecution {
            request: self.execution.into_http()?,
            disclosure: self.disclosure,
        })
    }
}

impl fmt::Debug for PreparedRequest {
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
    /// is redacted, its URL is the presentation URL, and failure diagnostics are
    /// withheld while the failure kind is kept.
    pub async fn execute<C, P>(
        self,
        engine: &HttpEngine,
        mut options: ExecutionOptions,
        output: Option<&Path>,
        cancellation: C,
        progress: P,
    ) -> Result<ExecutedResponse, HttpError>
    where
        C: Future + Send,
        P: FnMut(HttpProgress) + Send,
    {
        if self.uses_secrets() {
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
            Ok(executed) => Ok(ExecutedResponse {
                response: self.disclosure.redact_response(executed.response),
                body_sha256: executed.body_sha256,
            }),
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
}

impl SecretDisclosure {
    fn new(environment: ResolvedEnvironment, presentation_url: Option<&str>) -> Self {
        if !environment.has_resolved_secrets() {
            return Self::default();
        }
        Self {
            secrets: Some(environment),
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
        response.url.clone_from(&self.presentation_url);
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
            | HttpError::Cancelled => error,
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
    if !matches!(request.kind, probe_core::RequestKind::Http { .. }) {
        return Err("Copy as cURL is available only for HTTP requests".into());
    }
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
