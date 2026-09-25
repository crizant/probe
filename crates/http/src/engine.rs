use std::{
    collections::VecDeque,
    future::Future,
    future::pending,
    path::Path,
    sync::{Arc, Mutex},
    time::Instant,
};

use probe_core::{HttpRequest, RequestSettings};
use reqwest::{Client, header::HeaderMap, redirect::Policy};

use crate::{
    ExecutionOptions, HttpError, HttpResponse, ResponseHeader,
    request::build_request,
    response::{collect_bounded, map_reqwest_error, stream_to_file},
};

const DEFAULT_MAX_REDIRECTS: usize = 10;
const MAX_CACHED_REDIRECT_CLIENTS: usize = 16;
type RedirectClientCache = Arc<Mutex<VecDeque<((bool, usize), Client)>>>;

/// Progress reported while an HTTP response is being received.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HttpProgress {
    /// Response headers have arrived and the body is about to be read.
    ResponseStarted {
        /// Numeric HTTP status code.
        status: u16,
        /// Canonical reason phrase when one is defined.
        reason: String,
        /// Expected response-body size when supplied by the server.
        content_length: Option<u64>,
    },
    /// The decoded response-body byte count received so far.
    BodyReceived {
        /// Total decoded bytes received so far.
        bytes: u64,
    },
}

/// Reusable asynchronous HTTP engine shared by every interface.
#[derive(Clone, Debug)]
pub struct HttpEngine {
    default_client: Client,
    redirect_clients: RedirectClientCache,
}

/// A completed response streamed to a caller-owned file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StreamedHttpResponse {
    /// Response metadata and bounded body preview.
    pub response: HttpResponse,
    /// SHA-256 digest of the complete bytes written to the output file.
    pub body_sha256: [u8; 32],
}

impl HttpEngine {
    /// Creates an engine with the default redirect policy.
    pub fn new() -> Result<Self, HttpError> {
        Ok(Self {
            default_client: build_client(true, DEFAULT_MAX_REDIRECTS)?,
            redirect_clients: Arc::new(Mutex::new(VecDeque::new())),
        })
    }

    /// Executes a request until completion.
    pub async fn execute(
        &self,
        request: &HttpRequest,
        options: &ExecutionOptions,
    ) -> Result<HttpResponse, HttpError> {
        self.execute_cancellable(request, options, pending::<()>())
            .await
    }

    /// Executes a request while streaming its response body to a file.
    ///
    /// The destination is replaced only after the full response body has been written.
    pub async fn execute_to_file(
        &self,
        request: &HttpRequest,
        options: &ExecutionOptions,
        output: &Path,
    ) -> Result<HttpResponse, HttpError> {
        self.execute_cancellable_to_file(request, options, output, pending::<()>())
            .await
    }

    /// Executes a request, cancelling it when `cancellation` completes.
    ///
    /// Dropping the execution future also cancels the underlying reqwest request.
    pub async fn execute_cancellable<C>(
        &self,
        request: &HttpRequest,
        options: &ExecutionOptions,
        cancellation: C,
    ) -> Result<HttpResponse, HttpError>
    where
        C: Future + Send,
    {
        self.execute_with_cancellation(request, options, None, cancellation, |_| {})
            .await
            .map(|executed| executed.response)
    }

    /// Executes a cancellable request and reports response-header and body progress.
    pub async fn execute_cancellable_with_progress<C, P>(
        &self,
        request: &HttpRequest,
        options: &ExecutionOptions,
        cancellation: C,
        progress: P,
    ) -> Result<HttpResponse, HttpError>
    where
        C: Future + Send,
        P: FnMut(HttpProgress) + Send,
    {
        self.execute_with_cancellation(request, options, None, cancellation, progress)
            .await
            .map(|executed| executed.response)
    }

    /// Executes a cancellable request while streaming its response body to a file.
    pub async fn execute_cancellable_to_file<C>(
        &self,
        request: &HttpRequest,
        options: &ExecutionOptions,
        output: &Path,
        cancellation: C,
    ) -> Result<HttpResponse, HttpError>
    where
        C: Future + Send,
    {
        self.execute_with_cancellation(request, options, Some(output), cancellation, |_| {})
            .await
            .map(|executed| executed.response)
    }

    /// Executes a cancellable request, streams its body to a file, and reports progress.
    pub async fn execute_cancellable_to_file_with_progress<C, P>(
        &self,
        request: &HttpRequest,
        options: &ExecutionOptions,
        output: &Path,
        cancellation: C,
        progress: P,
    ) -> Result<StreamedHttpResponse, HttpError>
    where
        C: Future + Send,
        P: FnMut(HttpProgress) + Send,
    {
        let executed = self
            .execute_with_cancellation(request, options, Some(output), cancellation, progress)
            .await?;
        Ok(StreamedHttpResponse {
            response: executed.response,
            body_sha256: executed
                .body_sha256
                .expect("file responses always include a body digest"),
        })
    }

    async fn execute_with_cancellation<C, P>(
        &self,
        request: &HttpRequest,
        options: &ExecutionOptions,
        output: Option<&Path>,
        cancellation: C,
        mut progress: P,
    ) -> Result<ExecutedHttpResponse, HttpError>
    where
        C: Future + Send,
        P: FnMut(HttpProgress) + Send,
    {
        tokio::pin!(cancellation);
        tokio::select! {
            biased;
            _ = &mut cancellation => Err(HttpError::Cancelled),
            response = self.execute_inner(request, options, output, &mut progress) => response,
        }
    }

    async fn execute_inner<P>(
        &self,
        request: &HttpRequest,
        options: &ExecutionOptions,
        output: Option<&Path>,
        progress: &mut P,
    ) -> Result<ExecutedHttpResponse, HttpError>
    where
        P: FnMut(HttpProgress),
    {
        let client = self.client_for(&request.settings)?;
        let builder = build_request(&client, request, options).await?;
        let started = Instant::now();
        let mut response = builder.send().await.map_err(map_reqwest_error)?;
        let expected_size = response.content_length();
        let status = response.status();
        let url = response.url().to_string();
        let headers = response_headers(response.headers());
        progress(HttpProgress::ResponseStarted {
            status: status.as_u16(),
            reason: status.canonical_reason().unwrap_or_default().to_owned(),
            content_length: expected_size,
        });
        let body = match output {
            Some(output) => {
                stream_to_file(&mut response, output, |bytes| {
                    progress(HttpProgress::BodyReceived { bytes });
                })
                .await?
            }
            None => {
                collect_bounded(
                    &mut response,
                    options.response_cache.as_ref(),
                    expected_size,
                    |bytes| progress(HttpProgress::BodyReceived { bytes }),
                )
                .await?
            }
        };
        let body_sha256 = body.sha256;
        Ok(ExecutedHttpResponse {
            response: HttpResponse {
                status: status.as_u16(),
                reason: status.canonical_reason().unwrap_or_default().to_owned(),
                url,
                duration: started.elapsed(),
                size: body.size,
                headers,
                body: body.preview,
                body_complete: body.complete,
                body_file: body.file,
                body_retention_error: body.retention_error,
            },
            body_sha256,
        })
    }

    fn client_for(&self, settings: &RequestSettings) -> Result<Client, HttpError> {
        let follow = settings.follow_redirects.unwrap_or(true);
        let maximum = settings.max_redirects.unwrap_or(DEFAULT_MAX_REDIRECTS);
        if follow && maximum == DEFAULT_MAX_REDIRECTS {
            Ok(self.default_client.clone())
        } else {
            let key = (follow, if follow { maximum } else { 0 });
            let mut clients = self
                .redirect_clients
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some((_, client)) = clients.iter().find(|(cached, _)| *cached == key) {
                return Ok(client.clone());
            }
            let client = build_client(follow, maximum)?;
            if clients.len() == MAX_CACHED_REDIRECT_CLIENTS {
                clients.pop_front();
            }
            clients.push_back((key, client.clone()));
            Ok(client)
        }
    }
}

struct ExecutedHttpResponse {
    response: HttpResponse,
    body_sha256: Option<[u8; 32]>,
}

fn build_client(follow_redirects: bool, maximum: usize) -> Result<Client, HttpError> {
    let policy = if follow_redirects {
        Policy::limited(maximum)
    } else {
        Policy::none()
    };
    Client::builder()
        .redirect(policy)
        .build()
        .map_err(|error| HttpError::ClientConfiguration(error.to_string()))
}

fn response_headers(headers: &HeaderMap) -> Vec<ResponseHeader> {
    let mut headers: Vec<_> = headers
        .iter()
        .map(|(name, value)| ResponseHeader {
            name: name.as_str().to_owned(),
            value: String::from_utf8_lossy(value.as_bytes()).into_owned(),
        })
        .collect();
    headers.sort_by(|left, right| (&left.name, &left.value).cmp(&(&right.name, &right.value)));
    headers
}

#[cfg(test)]
mod tests {
    use probe_core::RequestSettings;

    use super::{HttpEngine, MAX_CACHED_REDIRECT_CLIENTS};

    #[test]
    fn redirect_clients_are_reused_and_bounded() {
        let engine = HttpEngine::new().unwrap();
        let shared = engine.clone();
        let settings = RequestSettings {
            follow_redirects: Some(false),
            max_redirects: Some(3),
            ..RequestSettings::default()
        };
        engine.client_for(&settings).unwrap();
        shared.client_for(&settings).unwrap();
        assert_eq!(engine.redirect_clients.lock().unwrap().len(), 1);

        for maximum in 1..=MAX_CACHED_REDIRECT_CLIENTS + 2 {
            engine
                .client_for(&RequestSettings {
                    max_redirects: Some(maximum),
                    ..RequestSettings::default()
                })
                .unwrap();
        }
        assert_eq!(
            engine.redirect_clients.lock().unwrap().len(),
            MAX_CACHED_REDIRECT_CLIENTS
        );
    }
}
