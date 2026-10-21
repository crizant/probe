use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use crate::credentials::{CredentialId, CredentialStore, NativeSecretProvider};
use crate::filesystem::workspace_base_directory;
use atomic_write_file::AtomicWriteFile;
use directories::{ProjectDirs, UserDirs};
use probe_application::{
    AssertionResult, HttpExecutionError, NoSecrets, RequestResolution, prepare_request,
};
use probe_core::{Environment, Request, RequestKey, SecretContext, SecretError, SecretProvider};
use probe_http::{
    ExecutionOptions, HttpEngine, HttpError, HttpProgress, HttpResponse, ResponseBodyFile,
    ResponseCache,
};
use sha2::{Digest, Sha256};
use tokio::sync::{mpsc, oneshot};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SavedResponseBody {
    path: PathBuf,
    sha256: [u8; 32],
}

impl SavedResponseBody {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ResponseState {
    Running {
        started_at: Instant,
        progress: Option<ResponseProgress>,
    },
    Complete {
        response: Box<HttpResponse>,
        saved_to: Option<SavedResponseBody>,
        assertions: Vec<AssertionResult>,
        assertion_declarations: Arc<Vec<probe_core::Assertion>>,
    },
    Failed(String),
    Cancelled,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResponseProgress {
    pub(crate) status: u16,
    pub(crate) reason: String,
    pub(crate) received_bytes: u64,
    pub(crate) content_length: Option<u64>,
}

impl ResponseState {
    pub(crate) const fn is_running(&self) -> bool {
        matches!(self, Self::Running { .. })
    }

    pub(crate) fn elapsed(&self) -> Option<Duration> {
        match self {
            Self::Running { started_at, .. } => Some(started_at.elapsed()),
            Self::Complete { response, .. } => Some(response.duration),
            Self::Failed(_) | Self::Cancelled => None,
        }
    }
}

pub(crate) struct ActiveRequest {
    pub(crate) generation: u64,
    pub(crate) cancellation: oneshot::Sender<()>,
    assertion_declarations: Vec<probe_core::Assertion>,
}

#[derive(Default)]
pub(crate) struct ExecutionState {
    next_generation: u64,
    responses: BTreeMap<RequestKey, ResponseState>,
    active: BTreeMap<RequestKey, ActiveRequest>,
    key_aliases: BTreeMap<RequestKey, RequestKey>,
}

impl ExecutionState {
    #[cfg(test)]
    pub(crate) fn begin(&mut self, key: RequestKey, cancellation: oneshot::Sender<()>) -> u64 {
        self.begin_with_assertions(key, cancellation, Vec::new())
    }

    pub(crate) fn begin_with_assertions(
        &mut self,
        key: RequestKey,
        cancellation: oneshot::Sender<()>,
        assertion_declarations: Vec<probe_core::Assertion>,
    ) -> u64 {
        self.cancel(key);
        self.next_generation = self.next_generation.wrapping_add(1);
        let generation = self.next_generation;
        self.active.insert(
            key,
            ActiveRequest {
                generation,
                cancellation,
                assertion_declarations,
            },
        );
        self.responses.insert(
            key,
            ResponseState::Running {
                started_at: Instant::now(),
                progress: None,
            },
        );
        generation
    }

    /// Records the result of execution `generation` and returns the request's
    /// current key, or `None` when the execution was superseded, cancelled, or
    /// removed.
    pub(crate) fn finish(
        &mut self,
        key: RequestKey,
        generation: u64,
        result: Result<(HttpResponse, Vec<AssertionResult>), HttpError>,
        saved_to: Option<SavedResponseBody>,
    ) -> Option<RequestKey> {
        let key = self.resolve_key(key);
        if self
            .active
            .get(&key)
            .is_none_or(|active| active.generation != generation)
        {
            return None;
        }
        let active = self.active.remove(&key).expect("generation was checked");
        self.key_aliases.retain(|_, target| *target != key);
        let response = match result {
            Ok((response, assertions)) => ResponseState::Complete {
                response: Box::new(response),
                saved_to,
                assertions,
                assertion_declarations: Arc::new(active.assertion_declarations),
            },
            Err(HttpError::Cancelled) => ResponseState::Cancelled,
            Err(error) => ResponseState::Failed(error.to_string()),
        };
        self.responses.insert(key, response);
        Some(key)
    }

    pub(crate) fn report_progress(
        &mut self,
        key: RequestKey,
        generation: u64,
        update: HttpProgress,
    ) {
        let key = self.resolve_key(key);
        if self
            .active
            .get(&key)
            .is_none_or(|active| active.generation != generation)
        {
            return;
        }
        let Some(ResponseState::Running { progress, .. }) = self.responses.get_mut(&key) else {
            return;
        };
        match update {
            HttpProgress::ResponseStarted {
                status,
                reason,
                content_length,
            } => {
                *progress = Some(ResponseProgress {
                    status,
                    reason,
                    received_bytes: 0,
                    content_length,
                });
            }
            HttpProgress::BodyReceived { bytes } => {
                if let Some(progress) = progress {
                    progress.received_bytes = bytes;
                }
            }
        }
    }

    pub(crate) fn fail(&mut self, key: RequestKey, message: String) {
        let key = self.resolve_key(key);
        self.cancel(key);
        self.responses.insert(key, ResponseState::Failed(message));
    }

    pub(crate) fn cancel(&mut self, key: RequestKey) {
        let key = self.resolve_key(key);
        if let Some(active) = self.active.remove(&key) {
            let _ = active.cancellation.send(());
            self.responses.insert(key, ResponseState::Cancelled);
            self.key_aliases.retain(|_, target| *target != key);
        }
    }

    pub(crate) fn cancel_all(&mut self) {
        for (_, active) in std::mem::take(&mut self.active) {
            let _ = active.cancellation.send(());
        }
        self.key_aliases.clear();
        for response in self.responses.values_mut() {
            if response.is_running() {
                *response = ResponseState::Cancelled;
            }
        }
    }

    pub(crate) fn clear(&mut self) {
        self.cancel_all();
        self.responses.clear();
        self.key_aliases.clear();
    }

    pub(crate) fn remove(&mut self, key: RequestKey) {
        let key = self.resolve_key(key);
        if let Some(active) = self.active.remove(&key) {
            let _ = active.cancellation.send(());
        }
        self.responses.remove(&key);
        self.key_aliases
            .retain(|alias, target| *alias != key && *target != key);
    }

    pub(crate) fn response(&self, key: RequestKey) -> Option<&ResponseState> {
        self.responses.get(&self.resolve_key(key))
    }

    pub(crate) fn remap_requests(&mut self, key_remaps: &BTreeMap<RequestKey, RequestKey>) {
        if key_remaps.is_empty() {
            self.cancel_all();
            self.responses.clear();
            return;
        }

        self.key_aliases = self
            .key_aliases
            .iter()
            .filter_map(|(alias, target)| key_remaps.get(target).map(|new| (*alias, *new)))
            .collect();

        self.responses = std::mem::take(&mut self.responses)
            .into_iter()
            .filter_map(|(key, response)| key_remaps.get(&key).map(|new| (*new, response)))
            .collect();

        let mut active = BTreeMap::new();
        for (key, request) in std::mem::take(&mut self.active) {
            if let Some(new_key) = key_remaps.get(&key).copied() {
                self.key_aliases.insert(key, new_key);
                active.insert(new_key, request);
            } else {
                let _ = request.cancellation.send(());
            }
        }
        self.active = active;
    }

    fn resolve_key(&self, key: RequestKey) -> RequestKey {
        let mut key = key;
        while let Some(next) = self.key_aliases.get(&key).copied() {
            if next == key {
                break;
            }
            key = next;
        }
        key
    }
}

pub(crate) fn body_file_path_for_storage(selected: &Path, workspace_path: Option<&Path>) -> String {
    if let Some(workspace_path) = workspace_path
        && let Some(base) = workspace_base_directory(workspace_path)
        && let Ok(relative) = selected.strip_prefix(&base)
    {
        let relative = relative
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        if relative.is_empty() {
            return selected.display().to_string();
        }
        if relative.contains('/') {
            return relative;
        }
        return format!("./{relative}");
    }
    selected.display().to_string()
}

pub(crate) struct ExecutionService {
    runtime: Option<tokio::runtime::Runtime>,
    engine: HttpEngine,
    credentials: Arc<dyn CredentialStore>,
}

type ExecutionResult = Result<
    (
        HttpResponse,
        Option<SavedResponseBody>,
        Vec<AssertionResult>,
    ),
    HttpError,
>;
type ExecutionReceivers = (
    oneshot::Receiver<ExecutionResult>,
    mpsc::UnboundedReceiver<HttpProgress>,
    oneshot::Receiver<SecretPresenceReconciliation>,
);

/// Opaque credential identities observed while resolving a request.
///
/// `found` secrets were returned by the native store. `missing` secrets were
/// looked up and absent. Store errors are omitted so a transient failure does
/// not erase presence metadata.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct SecretPresenceReconciliation {
    pub(crate) found: BTreeSet<String>,
    pub(crate) missing: BTreeSet<String>,
}

#[derive(Default)]
struct SecretPresenceObserver {
    found: Mutex<BTreeSet<String>>,
    missing: Mutex<BTreeSet<String>>,
}

impl SecretPresenceObserver {
    fn snapshot(&self) -> SecretPresenceReconciliation {
        SecretPresenceReconciliation {
            found: self.found.lock().unwrap().clone(),
            missing: self.missing.lock().unwrap().clone(),
        }
    }
}

struct ObservingSecretProvider<'a, S: CredentialStore + ?Sized> {
    inner: NativeSecretProvider<'a, S>,
    observer: &'a SecretPresenceObserver,
}

impl<S: CredentialStore + ?Sized> SecretProvider for ObservingSecretProvider<'_, S> {
    fn resolve_secret(
        &self,
        context: &SecretContext<'_>,
    ) -> Result<Option<probe_core::SecretValue>, SecretError> {
        let value = self.inner.resolve_secret(context)?;
        if let Some(environment) = context.environment_name
            && let Ok(id) = CredentialId::for_workspace(
                self.inner.workspace,
                environment,
                context.variable_name,
            )
        {
            let key = id.persistence_key().to_owned();
            if value.is_some() {
                self.observer.found.lock().unwrap().insert(key);
            } else {
                self.observer.missing.lock().unwrap().insert(key);
            }
        }
        Ok(value)
    }
}

impl Drop for ExecutionService {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}

impl ExecutionService {
    pub(crate) fn with_credentials(
        credentials: Arc<dyn CredentialStore>,
    ) -> Result<Self, HttpError> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .map_err(|error| HttpError::ClientConfiguration(error.to_string()))?;
        let engine = {
            let _guard = runtime.enter();
            HttpEngine::new()?
        };
        Ok(Self {
            runtime: Some(runtime),
            engine,
            credentials,
        })
    }

    pub(crate) fn connect_websocket(
        &self,
        input: ExecutionInput,
        mut cancellation: oneshot::Receiver<()>,
    ) -> (
        mpsc::Receiver<crate::websocket::Update>,
        oneshot::Receiver<SecretPresenceReconciliation>,
    ) {
        use crate::websocket::{BRIDGE_CAPACITY, TimelineEntry, Update};
        use probe_application::{SessionError, SessionEvent};
        let (events, receiver) = mpsc::channel(BRIDGE_CAPACITY);
        let (presence_tx, presence) = oneshot::channel();
        let credentials = Arc::clone(&self.credentials);
        self.runtime.as_ref().expect("execution runtime is active").spawn(async move {
            let observer = Arc::new(SecretPresenceObserver::default());
            let observed = Arc::clone(&observer);
            let resolution = tokio::task::spawn_blocking(move || {
                let native;
                let provider: &dyn SecretProvider = match &input.credential_workspace {
                    Some(workspace) => {
                        native = ObservingSecretProvider {
                            inner: NativeSecretProvider { store: credentials.as_ref(), workspace },
                            observer: &observed,
                        }; &native
                    }
                    None => &NoSecrets,
                };
                let resolution = RequestResolution {
                    environments: &input.environments,
                    client_certificates: &input.client_certificates,
                    environment: input.environment.as_deref(),
                    ..RequestResolution::default()
                };
                prepare_request(&input.request, &resolution, provider)
                    .map_err(|_| SessionError::Configuration)
                    .and_then(|prepared| prepared.into_websocket())
            });
            let connect = async {
                let prepared = resolution.await;
                let _ = presence_tx.send(observer.snapshot());
                prepared.map_err(|_| SessionError::Configuration)??.connect().await
            };
            let result = tokio::select! { biased; _ = &mut cancellation => return, result = connect => result };
            let mut session = match result {
                Ok(session) => session,
                Err(error) => {
                    for event in [SessionEvent::Error(error), SessionEvent::Closed { origin: probe_application::CloseOrigin::Error, code: None, reason: String::new() }] {
                        tokio::select! { biased; _ = &mut cancellation => return, _ = events.send(Update::Event(TimelineEntry::new(event))) => {} }
                    }
                    return;
                }
            };
            tokio::select! { biased; _ = &mut cancellation => return, result = events.send(Update::Ready(session.sender())) => { if result.is_err() { return; } } }
            loop {
                let event = tokio::select! { biased; _ = &mut cancellation => return, event = session.next_event() => event };
                let Some(event) = event else { break; };
                let terminal = matches!(event, SessionEvent::Closed { .. });
                let entry = TimelineEntry::new(event);
                tokio::select! { biased; _ = &mut cancellation => return, result = events.send(Update::Event(entry)) => { if result.is_err() { return; } } }
                if terminal { break; }
            }
        });
        (receiver, presence)
    }

    pub(crate) fn send_websocket(
        &self,
        sender: probe_application::SessionSender,
        data: probe_application::SessionData,
    ) -> oneshot::Receiver<Result<(), probe_application::SessionError>> {
        let (tx, rx) = oneshot::channel();
        self.runtime
            .as_ref()
            .expect("execution runtime is active")
            .spawn(async move {
                let _ = tx.send(sender.send(data).await);
            });
        rx
    }

    pub(crate) fn execute(
        &self,
        input: ExecutionInput,
        options: ExecutionOptions,
        output: Option<PathBuf>,
        cancellation: oneshot::Receiver<()>,
    ) -> ExecutionReceivers {
        let (result_sender, result_receiver) = oneshot::channel();
        let (progress_sender, progress_receiver) = mpsc::unbounded_channel();
        let (presence_sender, presence_receiver) = oneshot::channel();
        let engine = self.engine.clone();
        let credentials = Arc::clone(&self.credentials);
        let runtime = self.runtime.as_ref().expect("execution runtime is active");
        let handle = runtime.handle().clone();
        // PreparedRequest borrows a non-Send provider and owns a RefCell cache.
        // Keep preparation, async execution, and evaluation on one blocking worker;
        // Handle::block_on drives I/O on the existing runtime, never on GPUI.
        runtime.spawn_blocking(move || {
            let observer = SecretPresenceObserver::default();
            let native;
            let provider: &dyn SecretProvider = match &input.credential_workspace {
                Some(workspace) => {
                    native = ObservingSecretProvider {
                        inner: NativeSecretProvider {
                            store: credentials.as_ref(),
                            workspace,
                        },
                        observer: &observer,
                    };
                    &native
                }
                None => &NoSecrets,
            };
            let resolution = RequestResolution {
                environments: &input.environments,
                client_certificates: &input.client_certificates,
                environment: input.environment.as_deref(),
                ..RequestResolution::default()
            };
            let result = handle.block_on(async {
                let prepared = prepare_request(&input.request, &resolution, provider)
                    .map_err(|error| HttpError::InvalidBody(error.to_string()))?;
                let assertions = input
                    .request
                    .runtime
                    .assertions
                    .as_deref()
                    .unwrap_or_default();
                let (executed, assertions) = prepared
                    .reject_secret_output()
                    .execute_with_assertions(
                        &engine,
                        options,
                        output.as_deref(),
                        async move {
                            let _ = cancellation.await;
                        },
                        move |progress| {
                            let _ = progress_sender.send(progress);
                        },
                        assertions,
                    )
                    .await
                    .map_err(|error| match error {
                        HttpExecutionError::Http(error) => error,
                        HttpExecutionError::Protocol(error) => {
                            HttpError::InvalidBody(error.to_string())
                        }
                    })?;
                let saved_to = output
                    .zip(executed.body_sha256)
                    .map(|(path, sha256)| SavedResponseBody { path, sha256 });
                Ok((executed.response, saved_to, assertions))
            });
            // Includes secrets reached only by assertion values.
            let _ = presence_sender.send(observer.snapshot());
            let _ = result_sender.send(result);
        });
        (result_receiver, progress_receiver, presence_receiver)
    }
}

/// Request and environment inputs owned by one desktop execution.
pub(crate) struct ExecutionInput {
    pub(crate) request: Request,
    pub(crate) environments: Vec<Environment>,
    pub(crate) client_certificates: Vec<probe_core::ClientCertificate>,
    pub(crate) environment: Option<String>,
    /// Saved workspace whose native credentials supply secret values.
    pub(crate) credential_workspace: Option<PathBuf>,
}

pub(crate) fn save_response_body(
    response: &HttpResponse,
    saved_to: Option<&SavedResponseBody>,
    destination: &Path,
) -> Result<(), String> {
    let mut output = AtomicWriteFile::open(destination).map_err(|error| error.to_string())?;
    if response.body_complete {
        output
            .write_all(&response.body)
            .map_err(|error| error.to_string())?;
    } else if let Some(body_file) = &response.body_file {
        copy_file(body_file.path(), &mut output)?;
    } else if let Some(saved_to) = saved_to {
        copy_verified_file(saved_to, &mut output)?;
    } else {
        return Err("The complete response body is no longer available.".to_owned());
    }
    output.sync_all().map_err(|error| error.to_string())?;
    output.commit().map_err(|error| error.to_string())
}

fn copy_verified_file(
    source: &SavedResponseBody,
    destination: &mut AtomicWriteFile,
) -> Result<(), String> {
    let mut source_file = File::open(&source.path).map_err(|error| error.to_string())?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = source_file
            .read(&mut buffer)
            .map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
        destination
            .write_all(&buffer[..read])
            .map_err(|error| error.to_string())?;
    }
    let actual: [u8; 32] = digest.finalize().into();
    if actual != source.sha256 {
        return Err(
            "The saved response body has changed since this response completed.".to_owned(),
        );
    }
    Ok(())
}

fn copy_file(source: &Path, destination: &mut AtomicWriteFile) -> Result<(), String> {
    let mut source = File::open(source).map_err(|error| error.to_string())?;
    std::io::copy(&mut source, destination)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

pub(crate) fn download_directory() -> PathBuf {
    UserDirs::new()
        .and_then(|directories| directories.download_dir().map(Path::to_owned))
        .unwrap_or_else(|| PathBuf::from("."))
}

pub(crate) fn suggested_request_filename(request: &Request) -> String {
    suggested_filename(request.url.as_deref(), None, None)
}

pub(crate) fn suggested_response_filename(response: &HttpResponse) -> String {
    let disposition = header_value(response, "content-disposition");
    let content_type = header_value(response, "content-type");
    suggested_filename(Some(&response.url), disposition, content_type)
}

fn suggested_filename(
    url: Option<&str>,
    content_disposition: Option<&str>,
    content_type: Option<&str>,
) -> String {
    let disposition_name = content_disposition.and_then(|value| {
        disposition_parameter(value, "filename*")
            .and_then(|value| {
                value
                    .split_once("''")
                    .map(|(_, encoded)| encoded)
                    .or(Some(value))
            })
            .map(percent_decode)
            .or_else(|| disposition_parameter(value, "filename").map(percent_decode))
    });
    let url_name = url.and_then(|url| {
        let path = url.split(['?', '#']).next().unwrap_or(url);
        let path = path
            .split_once("://")
            .map_or(Some(path), |(_, remainder)| {
                remainder.find('/').map(|index| &remainder[index..])
            })?;
        path.rsplit('/')
            .next()
            .filter(|segment| !segment.is_empty())
            .map(percent_decode)
    });
    let fallback = content_type.and_then(content_type_extension).map_or_else(
        || "response.bin".to_owned(),
        |extension| format!("response.{extension}"),
    );
    sanitize_filename(
        disposition_name
            .or(url_name)
            .as_deref()
            .unwrap_or(&fallback),
    )
}

fn disposition_parameter<'a>(value: &'a str, name: &str) -> Option<&'a str> {
    value.split(';').skip(1).find_map(|parameter| {
        let (candidate, value) = parameter.trim().split_once('=')?;
        candidate
            .trim()
            .eq_ignore_ascii_case(name)
            .then(|| value.trim().trim_matches('"'))
    })
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && let Some(value) = bytes
                .get(index + 1..index + 3)
                .and_then(|value| std::str::from_utf8(value).ok())
                .and_then(|value| u8::from_str_radix(value, 16).ok())
        {
            decoded.push(value);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn sanitize_filename(value: &str) -> String {
    let leaf = value.rsplit(['/', '\\']).next().unwrap_or_default();
    let sanitized: String = leaf
        .chars()
        .map(|character| {
            if character.is_control()
                || matches!(character, '<' | '>' | ':' | '"' | '|' | '?' | '*')
            {
                '_'
            } else {
                character
            }
        })
        .collect();
    let sanitized = sanitized.trim_matches([' ', '.']);
    if sanitized.is_empty() {
        "response.bin".to_owned()
    } else {
        sanitized.to_owned()
    }
}

fn content_type_extension(value: &str) -> Option<&'static str> {
    match value
        .split(';')
        .next()?
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "application/json" => Some("json"),
        "application/pdf" => Some("pdf"),
        "application/zip" => Some("zip"),
        "image/gif" => Some("gif"),
        "image/jpeg" => Some("jpg"),
        "image/png" => Some("png"),
        "image/webp" => Some("webp"),
        "text/csv" => Some("csv"),
        "text/html" => Some("html"),
        "text/plain" => Some("txt"),
        _ => None,
    }
}

fn header_value<'a>(response: &'a HttpResponse, name: &str) -> Option<&'a str> {
    response
        .headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case(name))
        .map(|header| header.value.as_str())
}

pub(crate) const RESPONSE_CACHE_QUOTA_BYTES: u64 = 512 * 1024 * 1024;

pub(crate) fn response_cache() -> ResponseCache {
    #[cfg(feature = "startup-benchmark")]
    if let Some(directory) = crate::startup_benchmark::directory() {
        return ResponseCache::new(directory.join("responses"), RESPONSE_CACHE_QUOTA_BYTES);
    }
    let directory = ProjectDirs::from("dev", "Probe", "Probe")
        .map(|directories| directories.cache_dir().join("responses"))
        .unwrap_or_else(|| std::env::temp_dir().join("probe-responses"));
    ResponseCache::new(directory, RESPONSE_CACHE_QUOTA_BYTES)
}

pub(crate) fn read_response_page(
    file: &ResponseBodyFile,
    offset: usize,
    length: usize,
) -> Result<Vec<u8>, String> {
    let mut source = std::fs::File::open(file.path()).map_err(|error| error.to_string())?;
    source
        .seek(SeekFrom::Start(offset as u64))
        .map_err(|error| error.to_string())?;
    let mut body = vec![0; length];
    source
        .read_exact(&mut body)
        .map_err(|error| error.to_string())?;
    Ok(body)
}

pub(crate) fn format_duration(duration: Duration) -> String {
    if duration.as_secs() > 0 {
        format!("{:.2} s", duration.as_secs_f64())
    } else {
        format!("{} ms", duration.as_millis())
    }
}

pub(crate) fn format_size(size: u64) -> String {
    if size >= 1024 * 1024 {
        format!("{:.1} MiB", size as f64 / (1024.0 * 1024.0))
    } else if size >= 1024 {
        format!("{:.1} KiB", size as f64 / 1024.0)
    } else {
        format!("{size} B")
    }
}

pub(crate) fn format_transfer_progress(received: u64, total: Option<u64>) -> String {
    let received = format_size(received);
    total.map_or_else(
        || format!("{received} received"),
        |total| format!("{received} of {}", format_size(total)),
    )
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{BTreeMap, HashMap},
        fs,
        io::{Read, Write},
        net::TcpListener,
        path::{Path, PathBuf},
        sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering},
        },
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    use crate::credentials::{
        CredentialId, CredentialStore, CredentialStoreError, NativeCredentialStore,
    };
    use probe_core::{
        Environment, EnvironmentVariable, Header, Request, SecretValue, SecretVariable, Variable,
        VariableValue, VariableValueSet,
    };
    use probe_http::ExecutionOptions;
    use probe_http::{HttpError, HttpProgress, HttpResponse, ResponseCache, ResponseHeader};
    use sha2::{Digest, Sha256};
    use tokio::sync::oneshot;

    use super::{
        ExecutionInput, ExecutionService, ExecutionState, ResponseState, SavedResponseBody,
        body_file_path_for_storage, format_duration, format_size, format_transfer_progress,
        save_response_body, suggested_request_filename, suggested_response_filename,
    };

    fn key() -> probe_core::RequestKey {
        let workspace = probe_core::Workspace::from_collection(probe_core::Collection {
            items: vec![probe_core::CollectionItem::Request(
                probe_core::Request::default(),
            )],
            ..probe_core::Collection::default()
        });
        let probe_core::WorkspaceItemRef::Request(key) = workspace.root_items()[0] else {
            panic!("expected request key");
        };
        key
    }

    fn two_keys() -> (probe_core::RequestKey, probe_core::RequestKey) {
        let workspace = probe_core::Workspace::from_collection(probe_core::Collection {
            items: vec![
                probe_core::CollectionItem::Request(probe_core::Request::default()),
                probe_core::CollectionItem::Request(probe_core::Request::default()),
            ],
            ..probe_core::Collection::default()
        });
        let probe_core::WorkspaceItemRef::Request(first) = workspace.root_items()[0] else {
            panic!("expected first request key");
        };
        let probe_core::WorkspaceItemRef::Request(second) = workspace.root_items()[1] else {
            panic!("expected second request key");
        };
        (first, second)
    }

    #[test]
    fn stale_completion_cannot_replace_a_newer_execution() {
        let key = key();
        let mut state = ExecutionState::default();
        let (first, _) = oneshot::channel();
        let first_generation = state.begin(key, first);
        let (second, _) = oneshot::channel();
        let second_generation = state.begin(key, second);

        assert_eq!(
            state.finish(key, first_generation, Ok((response(201), Vec::new())), None),
            None
        );
        assert!(matches!(
            state.response(key),
            Some(ResponseState::Running { .. })
        ));
        state.finish(
            key,
            second_generation,
            Ok((response(204), Vec::new())),
            None,
        );
        assert!(matches!(
            state.response(key),
            Some(ResponseState::Complete { response, .. }) if response.status == 204
        ));
    }

    fn outcomes(
        outcome: probe_application::AssertionOutcome,
    ) -> Vec<probe_application::AssertionResult> {
        vec![probe_application::AssertionResult {
            actual: None,
            actual_truncated: false,
            index: 0,
            outcome,
        }]
    }

    #[test]
    fn assertion_snapshots_follow_generation_and_key_lifecycle() {
        use probe_application::AssertionOutcome::{Failed, Passed};
        let (a, b) = two_keys();
        let (new_a, new_b) = two_keys();
        let mut state = ExecutionState::default();
        let (tx, _) = oneshot::channel();
        let first = state.begin(a, tx);
        state.finish(a, first, Ok((response(200), outcomes(Passed))), None);
        let (tx, _) = oneshot::channel();
        let declaration = probe_core::Assertion {
            expression: "response.status".into(),
            operator: "equals".into(),
            value: Some("{{original}}".into()),
            disabled: None,
            description: None,
        };
        let second = state.begin_with_assertions(a, tx, vec![declaration.clone()]);
        assert!(matches!(
            state.response(a),
            Some(ResponseState::Running { .. })
        ));
        assert_eq!(
            state.finish(a, first, Ok((response(200), outcomes(Passed))), None),
            None
        );
        let (tx, _) = oneshot::channel();
        let other = state.begin(b, tx);
        state.finish(b, other, Ok((response(201), outcomes(Passed))), None);
        state.remap_requests(&BTreeMap::from([(a, new_a), (b, new_b)]));
        state.finish(a, second, Ok((response(202), outcomes(Failed))), None);
        assert!(
            matches!(state.response(new_a), Some(ResponseState::Complete { response, assertions, assertion_declarations, .. }) if response.status == 202 && assertions == &outcomes(Failed) && assertion_declarations.as_slice() == [declaration])
        );
        assert!(
            matches!(state.response(new_b), Some(ResponseState::Complete { response, assertions, .. }) if response.status == 201 && assertions == &outcomes(Passed))
        );
        let (tx, _) = oneshot::channel();
        let cancelled = state.begin(new_a, tx);
        state.cancel(new_a);
        state.finish(
            new_a,
            cancelled,
            Ok((response(200), outcomes(Passed))),
            None,
        );
        assert_eq!(state.response(new_a), Some(&ResponseState::Cancelled));
        state.remove(new_b);
        assert_eq!(
            state.finish(b, other, Ok((response(201), outcomes(Passed))), None),
            None
        );
        state.clear();
        assert!(state.response(new_a).is_none());
        assert!(state.response(new_b).is_none());
    }

    #[test]
    fn response_progress_updates_only_the_matching_execution() {
        let key = key();
        let mut state = ExecutionState::default();
        let (first, _) = oneshot::channel();
        let first_generation = state.begin(key, first);
        state.report_progress(
            key,
            first_generation,
            HttpProgress::ResponseStarted {
                status: 200,
                reason: "OK".to_owned(),
                content_length: Some(120 * 1024 * 1024),
            },
        );
        state.report_progress(
            key,
            first_generation,
            HttpProgress::BodyReceived {
                bytes: 38 * 1024 * 1024,
            },
        );

        assert!(matches!(
            state.response(key),
            Some(ResponseState::Running {
                progress: Some(progress),
                ..
            }) if progress.status == 200
                && progress.received_bytes == 38 * 1024 * 1024
                && progress.content_length == Some(120 * 1024 * 1024)
        ));

        let (second, _) = oneshot::channel();
        state.begin(key, second);
        state.report_progress(
            key,
            first_generation,
            HttpProgress::BodyReceived { bytes: 1 },
        );
        assert!(matches!(
            state.response(key),
            Some(ResponseState::Running { progress: None, .. })
        ));
    }

    #[test]
    fn cancellation_is_visible_and_normalized() {
        let key = key();
        let mut state = ExecutionState::default();
        let (sender, mut receiver) = oneshot::channel();
        let generation = state.begin(key, sender);
        state.cancel(key);
        assert!(receiver.try_recv().is_ok());
        assert_eq!(state.response(key), Some(&ResponseState::Cancelled));
        state.finish(key, generation, Err(HttpError::Cancelled), None);
        assert_eq!(state.response(key), Some(&ResponseState::Cancelled));
    }

    #[test]
    fn removing_an_execution_cancels_it_without_retaining_a_response() {
        let key = key();
        let mut state = ExecutionState::default();
        let (sender, mut receiver) = oneshot::channel();
        let generation = state.begin(key, sender);

        state.remove(key);

        assert!(receiver.try_recv().is_ok());
        assert!(state.response(key).is_none());

        state.finish(key, generation, Ok((response(200), Vec::new())), None);
        assert!(state.response(key).is_none());
    }

    #[test]
    fn reloaded_workspace_keeps_in_flight_execution_completable() {
        let (old_key, new_key) = two_keys();
        let mut state = ExecutionState::default();
        let (sender, mut receiver) = oneshot::channel();
        let generation = state.begin(old_key, sender);

        state.remap_requests(&BTreeMap::from([(old_key, new_key)]));

        assert!(receiver.try_recv().is_err());
        assert!(matches!(
            state.response(new_key),
            Some(ResponseState::Running { .. })
        ));
        assert!(matches!(
            state.response(old_key),
            Some(ResponseState::Running { .. })
        ));

        assert_eq!(
            state.finish(old_key, generation, Ok((response(202), Vec::new())), None),
            Some(new_key)
        );

        assert!(matches!(
            state.response(new_key),
            Some(ResponseState::Complete { response, .. }) if response.status == 202
        ));
        assert!(state.response(old_key).is_none());
    }

    #[test]
    fn swapped_requests_keep_their_own_in_flight_executions() {
        let (old_a, old_b) = two_keys();
        let (new_b, new_a) = two_keys();
        assert_eq!(
            (old_a.slot(), old_a.generation()),
            (new_b.slot(), new_b.generation())
        );
        assert_ne!(old_a, new_b);
        let mut state = ExecutionState::default();
        let (sender_a, mut receiver_a) = oneshot::channel();
        let (sender_b, mut receiver_b) = oneshot::channel();
        let generation_a = state.begin(old_a, sender_a);
        let generation_b = state.begin(old_b, sender_b);

        state.remap_requests(&BTreeMap::from([(old_a, new_a), (old_b, new_b)]));
        assert!(receiver_a.try_recv().is_err());
        assert!(receiver_b.try_recv().is_err());
        state.finish(old_a, generation_a, Ok((response(201), Vec::new())), None);
        state.finish(old_b, generation_b, Ok((response(202), Vec::new())), None);

        assert!(
            matches!(state.response(new_a), Some(ResponseState::Complete { response, .. }) if response.status == 201)
        );
        assert!(
            matches!(state.response(new_b), Some(ResponseState::Complete { response, .. }) if response.status == 202)
        );
        assert!(state.response(old_a).is_none());
        assert!(state.response(old_b).is_none());
    }

    #[test]
    fn removed_request_cannot_attach_to_a_reused_arena_position() {
        let (removed, retained) = two_keys();
        let (replacement, new_retained) = two_keys();
        let mut state = ExecutionState::default();
        let (removed_sender, mut removed_receiver) = oneshot::channel();
        let (retained_sender, _) = oneshot::channel();
        let removed_generation = state.begin(removed, removed_sender);
        let retained_generation = state.begin(retained, retained_sender);

        state.remap_requests(&BTreeMap::from([(retained, new_retained)]));
        assert!(removed_receiver.try_recv().is_ok());
        assert_eq!(
            state.finish(
                removed,
                removed_generation,
                Ok((response(201), Vec::new())),
                None
            ),
            None
        );
        assert!(state.response(replacement).is_none());
        state.finish(
            retained,
            retained_generation,
            Ok((response(202), Vec::new())),
            None,
        );
        assert!(
            matches!(state.response(new_retained), Some(ResponseState::Complete { response, .. }) if response.status == 202)
        );
    }

    #[test]
    fn metadata_units_are_readable() {
        assert_eq!(format_duration(Duration::from_millis(83)), "83 ms");
        assert_eq!(format_duration(Duration::from_millis(1250)), "1.25 s");
        assert_eq!(format_size(812), "812 B");
        assert_eq!(format_size(1023), "1023 B");
        assert_eq!(format_size(1024), "1.0 KiB");
        assert_eq!(format_size(2048), "2.0 KiB");
        assert_eq!(format_size(1024 * 1024), "1.0 MiB");
        assert_eq!(
            format_transfer_progress(40_265_318, Some(120 * 1024 * 1024)),
            "38.4 MiB of 120.0 MiB"
        );
        assert_eq!(format_transfer_progress(2048, None), "2.0 KiB received");
    }

    #[test]
    fn download_filenames_prefer_headers_then_url_and_are_sanitized() {
        let request = Request {
            url: Some("https://example.test/files/monthly%20report.csv?token=secret".to_owned()),
            ..Request::default()
        };
        assert_eq!(suggested_request_filename(&request), "monthly report.csv");
        assert_eq!(
            suggested_request_filename(&Request {
                url: Some("https://example.test".to_owned()),
                ..Request::default()
            }),
            "response.bin"
        );

        let mut response = response(200);
        response.url = "https://example.test/fallback.json".to_owned();
        response.headers = vec![ResponseHeader {
            name: "Content-Disposition".to_owned(),
            value: "attachment; filename*=UTF-8''..%2Funsafe%3Fname.pdf".to_owned(),
        }];
        assert_eq!(suggested_response_filename(&response), "unsafe_name.pdf");

        response.url = "https://example.test/".to_owned();
        response.headers = vec![ResponseHeader {
            name: "content-type".to_owned(),
            value: "application/zip; charset=binary".to_owned(),
        }];
        assert_eq!(suggested_response_filename(&response), "response.zip");
    }

    #[test]
    fn response_body_can_be_saved_from_memory_or_an_existing_download() {
        let directory = std::env::temp_dir().join(format!(
            "probe-response-save-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock should follow the Unix epoch")
                .as_nanos()
        ));
        fs::create_dir(&directory).expect("temporary directory should be created");

        let mut inline = response(200);
        inline.body = b"inline response".to_vec();
        inline.size = inline.body.len();
        let inline_destination = directory.join("inline.bin");
        save_response_body(&inline, None, &inline_destination)
            .expect("inline response should save");
        assert_eq!(fs::read(&inline_destination).unwrap(), inline.body);

        let existing = directory.join("existing.bin");
        fs::write(&existing, b"complete downloaded response").unwrap();
        let saved = SavedResponseBody {
            path: existing.clone(),
            sha256: Sha256::digest(b"complete downloaded response").into(),
        };
        let mut preview = response(200);
        preview.body = b"preview".to_vec();
        preview.size = b"complete downloaded response".len();
        preview.body_complete = false;
        let copied_destination = directory.join("copied.bin");
        save_response_body(&preview, Some(&saved), &copied_destination)
            .expect("existing download should be copied");
        assert_eq!(
            fs::read(&copied_destination).unwrap(),
            b"complete downloaded response"
        );

        fs::write(&existing, b"altered! downloaded response").unwrap();
        let rejected_destination = directory.join("rejected.bin");
        let error = save_response_body(&preview, Some(&saved), &rejected_destination)
            .expect_err("a changed download must not be exported as the old response");
        assert!(error.contains("changed since this response completed"));
        assert!(!rejected_destination.exists());

        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn desktop_adapter_forwards_cancellation_to_the_shared_http_engine() {
        let (cancel, cancellation) = oneshot::channel();
        cancel.send(()).expect("cancellation should be delivered");
        let service = ExecutionService::with_credentials(Arc::new(NativeCredentialStore))
            .expect("execution service should start");
        let (result, _, _) = service.execute(
            ExecutionInput {
                request: Request {
                    method: Some("GET".to_owned()),
                    url: Some("http://127.0.0.1:1/phase-12".to_owned()),
                    ..Request::default()
                },
                environments: Vec::new(),
                client_certificates: Vec::new(),
                environment: None,
                credential_workspace: None,
            },
            ExecutionOptions::default(),
            None,
            cancellation,
        );
        let result = service
            .runtime
            .as_ref()
            .unwrap()
            .block_on(result)
            .expect("execution should complete");
        assert_eq!(result, Err(HttpError::Cancelled));
    }

    struct FakeCredentials {
        values: Mutex<HashMap<CredentialId, String>>,
        reads: AtomicUsize,
        fail: bool,
    }

    impl FakeCredentials {
        fn new(fail: bool) -> Self {
            Self {
                values: Mutex::new(HashMap::new()),
                reads: AtomicUsize::new(0),
                fail,
            }
        }
    }

    impl CredentialStore for FakeCredentials {
        fn set(&self, id: &CredentialId, value: &str) -> Result<(), CredentialStoreError> {
            self.values
                .lock()
                .unwrap()
                .insert(id.clone(), value.to_owned());
            Ok(())
        }
        fn delete(&self, id: &CredentialId) -> Result<(), CredentialStoreError> {
            self.values
                .lock()
                .unwrap()
                .remove(id)
                .map(|_| ())
                .ok_or(CredentialStoreError::NotFound)
        }
        fn get(&self, id: &CredentialId) -> Result<Option<SecretValue>, CredentialStoreError> {
            self.reads.fetch_add(1, Ordering::Relaxed);
            if self.fail {
                return Err(CredentialStoreError::Unavailable);
            }
            Ok(self
                .values
                .lock()
                .unwrap()
                .get(id)
                .cloned()
                .map(SecretValue::new))
        }
    }

    fn native_test_environment() -> Vec<Environment> {
        vec![Environment {
            name: "production".into(),
            color: None,
            extends: None,
            description: None,
            dot_env_file_path: None,
            client_certificates: Vec::new(),
            variables: vec![
                EnvironmentVariable::Secret(SecretVariable {
                    name: Some("token".into()),
                    value_type: None,
                    disabled: false,
                }),
                EnvironmentVariable::Secret(SecretVariable {
                    name: Some("unused".into()),
                    value_type: None,
                    disabled: false,
                }),
                EnvironmentVariable::Plain(Variable {
                    name: Some("outer".into()),
                    value: Some(VariableValueSet::Single(VariableValue::String(
                        "Bearer {{token}}".into(),
                    ))),
                    disabled: false,
                }),
            ],
        }]
    }

    fn native_input(request: Request, workspace: PathBuf) -> ExecutionInput {
        ExecutionInput {
            request,
            environments: native_test_environment(),
            client_certificates: Vec::new(),
            environment: Some("production".into()),
            credential_workspace: Some(workspace),
        }
    }

    #[test]
    fn desktop_send_resolves_native_credentials_through_shared_redaction() {
        let secret = "SUPER_SECRET_VALUE_THAT_MUST_NEVER_APPEAR";
        let workspace = std::env::temp_dir();
        let store = Arc::new(FakeCredentials::new(false));
        let id = CredentialId::for_workspace(&workspace, "production", "token").unwrap();
        store.set(&id, secret).unwrap();
        let service = ExecutionService::with_credentials(store.clone()).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut received = Vec::new();
            let mut buffer = [0; 4096];
            while !received.windows(4).any(|part| part == b"\r\n\r\n") {
                let count = stream.read(&mut buffer).unwrap();
                assert!(count > 0);
                received.extend_from_slice(&buffer[..count]);
            }
            assert!(
                received
                    .windows(secret.len())
                    .any(|part| part == secret.as_bytes())
            );
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nX-Echo: {secret}\r\nConnection: close\r\n\r\n{secret}",
                secret.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
        });
        let request = Request {
            method: Some("GET".into()),
            url: Some(format!("http://{address}/{{{{token}}}}")),
            headers: vec![Header {
                name: "Authorization".into(),
                value: "{{outer}}".into(),
                disabled: false,
            }],
            ..Request::default()
        };
        let (cancel_sender, cancellation) = oneshot::channel();
        let (receiver, _, _) = service.execute(
            native_input(request, workspace),
            ExecutionOptions::default(),
            None,
            cancellation,
        );
        let (response, _, _) = service
            .runtime
            .as_ref()
            .unwrap()
            .block_on(receiver)
            .unwrap()
            .unwrap();
        drop(cancel_sender);
        server.join().unwrap();
        assert_eq!(store.reads.load(Ordering::Relaxed), 1);
        assert!(response.url.contains("{{token}}"));
        assert!(!format!("{response:?}").contains(secret));
        assert_eq!(response.body, b"[REDACTED]");
    }

    #[test]
    fn desktop_assertions_use_original_bytes_and_assertion_only_native_secrets() {
        use probe_application::{AssertionError, AssertionOutcome};
        use probe_core::{Assertion, RequestKind, RequestRuntime};
        for graphql in [false, true] {
            let secret = "ASSERTION_ONLY_NATIVE_SECRET";
            let workspace = std::env::temp_dir();
            let store = Arc::new(FakeCredentials::new(false));
            let id = CredentialId::for_workspace(&workspace, "production", "token").unwrap();
            store.set(&id, secret).unwrap();
            let service = ExecutionService::with_credentials(store.clone()).unwrap();
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut buffer = [0; 4096];
                while !bytes.windows(4).any(|part| part == b"\r\n\r\n") {
                    let count = stream.read(&mut buffer).unwrap();
                    assert!(count > 0);
                    bytes.extend_from_slice(&buffer[..count]);
                }
                let body = format!("{{\"token\":\"{secret}\"}}");
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            });
            let assertion =
                |expression: &str, operator: &str, value: Option<&str>, disabled| Assertion {
                    expression: expression.into(),
                    operator: operator.into(),
                    value: value.map(str::to_owned),
                    disabled,
                    description: None,
                };
            let mut request = Request {
                url: Some(format!("http://{address}/")),
                method: Some("GET".into()),
                runtime: RequestRuntime {
                    assertions: Some(vec![
                        assertion("response.body.token", "equals", Some("{{token}}"), None),
                        assertion("response.status", "equals", Some("404"), None),
                        assertion("response.status", "equals", Some("{{missing}}"), None),
                        assertion(
                            "sensitive declaration",
                            "future",
                            Some("{{unused}}"),
                            Some(true),
                        ),
                        assertion("response.body.token", "isNotNull", Some("{{unused}}"), None),
                    ]),
                },
                ..Request::default()
            };
            if graphql {
                request.kind = RequestKind::Graphql { body: None };
            }
            let (_cancel, cancellation) = oneshot::channel();
            let (receiver, _, presence) = service.execute(
                native_input(request, workspace),
                ExecutionOptions::default(),
                None,
                cancellation,
            );
            let runtime = service.runtime.as_ref().unwrap();
            let (response, saved_to, results) = runtime.block_on(receiver).unwrap().unwrap();
            let observed = runtime.block_on(presence).unwrap();
            server.join().unwrap();
            assert!(saved_to.is_none());
            assert_eq!(
                results
                    .iter()
                    .map(|r| r.outcome.clone())
                    .collect::<Vec<_>>(),
                vec![
                    AssertionOutcome::Passed,
                    AssertionOutcome::Failed,
                    AssertionOutcome::Error(AssertionError::VariableResolution),
                    AssertionOutcome::Disabled,
                    AssertionOutcome::Passed,
                ]
            );
            assert_eq!(store.reads.load(Ordering::Relaxed), 1);
            assert!(observed.found.contains(id.persistence_key()));
            assert!(!format!("{response:?} {results:?}").contains(secret));
            assert!(String::from_utf8_lossy(&response.body).contains("[REDACTED]"));
        }
    }

    #[test]
    fn desktop_large_plain_assertion_responses_keep_paging_and_stream_complete_saves() {
        use probe_application::AssertionOutcome;
        use probe_core::{Assertion, RequestRuntime};
        for save in [false, true] {
            let mut body = vec![b'x'; probe_http::MAX_IN_MEMORY_RESPONSE_BYTES];
            body.extend(vec![b'y'; 1024]);
            let expected = body.clone();
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut buffer = [0; 4096];
                assert!(stream.read(&mut buffer).unwrap() > 0);
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .unwrap();
                stream.write_all(&body).unwrap();
            });
            let directory = tempfile::TempDir::new().unwrap();
            let output = directory.path().join("response.bin");
            let service =
                ExecutionService::with_credentials(Arc::new(FakeCredentials::new(false))).unwrap();
            let (_cancel, cancellation) = oneshot::channel();
            let (receiver, mut progress, _) = service.execute(
                ExecutionInput {
                    request: Request {
                        url: Some(format!("http://{address}/")),
                        method: Some("GET".into()),
                        runtime: RequestRuntime {
                            assertions: Some(vec![Assertion {
                                expression: "response.status".into(),
                                operator: "equals".into(),
                                value: Some("200".into()),
                                disabled: None,
                                description: None,
                            }]),
                        },
                        ..Request::default()
                    },
                    environments: Vec::new(),
                    client_certificates: Vec::new(),
                    environment: None,
                    credential_workspace: None,
                },
                ExecutionOptions {
                    response_cache: Some(ResponseCache::new(
                        directory.path().join("cache"),
                        64 * 1024 * 1024,
                    )),
                    ..ExecutionOptions::default()
                },
                save.then(|| output.clone()),
                cancellation,
            );
            let (response, saved_to, results) = service
                .runtime
                .as_ref()
                .unwrap()
                .block_on(receiver)
                .unwrap()
                .unwrap();
            server.join().unwrap();
            assert_eq!(response.status, 200);
            assert_eq!(results[0].outcome, AssertionOutcome::Passed);
            assert!(!response.body_complete);
            if save {
                assert_eq!(fs::read(&output).unwrap(), expected);
                assert_eq!(saved_to.as_ref().unwrap().path(), output);
                let exported = directory.path().join("exported.bin");
                save_response_body(&response, saved_to.as_ref(), &exported).unwrap();
                assert_eq!(fs::read(exported).unwrap(), expected);
            } else {
                let file = response
                    .body_file
                    .as_ref()
                    .expect("plain assertions must retain the complete paged body");
                assert_eq!(fs::read(file.path()).unwrap(), expected);
                let (document, _, _) = crate::response_viewer::prepare_document(&response, 1);
                assert!(document.file_backed);
                assert_eq!(document.total_size, expected.len());
                assert_eq!(
                    super::read_response_page(file, probe_http::MAX_IN_MEMORY_RESPONSE_BYTES, 1024)
                        .unwrap(),
                    vec![b'y'; 1024]
                );
            }
            assert!(matches!(
                progress.try_recv(),
                Ok(HttpProgress::ResponseStarted { status: 200, .. })
            ));
        }
    }

    #[test]
    fn desktop_sends_resolve_collection_client_certificate_scope() {
        for assertions in [
            None,
            Some(vec![probe_core::Assertion {
                expression: "response.status".into(),
                operator: "equals".into(),
                value: Some("200".into()),
                disabled: None,
                description: None,
            }]),
        ] {
            let directory = tempfile::TempDir::new().unwrap();
            let service =
                ExecutionService::with_credentials(Arc::new(FakeCredentials::new(false))).unwrap();
            let (_cancel, cancellation) = oneshot::channel();
            let (receiver, _, _) = service.execute(
                ExecutionInput {
                    request: Request {
                        url: Some("https://127.0.0.1:1/".into()),
                        method: Some("GET".into()),
                        runtime: probe_core::RequestRuntime { assertions },
                        ..Request::default()
                    },
                    environments: Vec::new(),
                    environment: None,
                    credential_workspace: None,
                    client_certificates: vec![probe_core::ClientCertificate::Pem {
                        domain: "127.0.0.1:1".into(),
                        disabled: false,
                        certificate_file_path: directory
                            .path()
                            .join("missing-cert.pem")
                            .display()
                            .to_string(),
                        private_key_file_path: directory
                            .path()
                            .join("missing-key.pem")
                            .display()
                            .to_string(),
                        passphrase: None,
                    }],
                },
                ExecutionOptions::default(),
                None,
                cancellation,
            );
            let result = service
                .runtime
                .as_ref()
                .unwrap()
                .block_on(receiver)
                .unwrap();
            assert!(
                matches!(result, Err(HttpError::ClientConfiguration(message)) if message == "cannot read client certificate file")
            );
        }
    }

    #[test]
    fn desktop_direct_save_refuses_request_assertion_and_certificate_secrets_consistently() {
        use probe_core::{Assertion, ClientCertificate};
        for source in ["request", "assertion", "certificate"] {
            for with_assertions in [false, true] {
                if source == "assertion" && !with_assertions {
                    continue;
                }
                let directory = tempfile::TempDir::new().unwrap();
                let store = Arc::new(FakeCredentials::new(false));
                let id =
                    CredentialId::for_workspace(directory.path(), "production", "token").unwrap();
                store.set(&id, "runtime-secret").unwrap();
                let service = ExecutionService::with_credentials(store).unwrap();
                let mut input = native_input(
                    Request {
                        url: Some(
                            if source == "request" {
                                "https://127.0.0.1:1/{{token}}"
                            } else {
                                "https://127.0.0.1:1/"
                            }
                            .into(),
                        ),
                        method: Some("GET".into()),
                        ..Request::default()
                    },
                    directory.path().to_owned(),
                );
                if with_assertions {
                    input.request.runtime.assertions = Some(vec![Assertion {
                        expression: "response.status".into(),
                        operator: "equals".into(),
                        value: Some(
                            if source == "assertion" {
                                "{{outer}}"
                            } else {
                                "200"
                            }
                            .into(),
                        ),
                        disabled: None,
                        description: None,
                    }]);
                }
                if source == "certificate" {
                    input.client_certificates = vec![ClientCertificate::Pem {
                        domain: "127.0.0.1:1".into(),
                        disabled: false,
                        certificate_file_path: "unused.pem".into(),
                        private_key_file_path: "unused-key.pem".into(),
                        passphrase: Some("{{token}}".into()),
                    }];
                }
                let output = directory.path().join("response.bin");
                let (_cancel, cancellation) = oneshot::channel();
                let (receiver, _, _) = service.execute(
                    input,
                    ExecutionOptions::default(),
                    Some(output.clone()),
                    cancellation,
                );
                let result = service
                    .runtime
                    .as_ref()
                    .unwrap()
                    .block_on(receiver)
                    .unwrap();
                assert!(
                    matches!(result, Err(HttpError::InvalidBody(message)) if message == "Saving a response directly to a file is unavailable when secret variables or unresolved assertion values are involved."),
                    "source: {source}, assertions: {with_assertions}"
                );
                assert!(!output.exists());
            }
        }
    }

    #[test]
    fn desktop_send_fails_closed_on_used_provider_failure() {
        let store = Arc::new(FakeCredentials::new(true));
        let service = ExecutionService::with_credentials(store.clone()).unwrap();
        let request = Request {
            method: Some("GET".into()),
            url: Some("http://127.0.0.1:1/{{token}}".into()),
            ..Request::default()
        };
        let (_cancel_sender, cancellation) = oneshot::channel();
        let (receiver, _, presence) = service.execute(
            native_input(request, Path::new("/").to_owned()),
            ExecutionOptions::default(),
            None,
            cancellation,
        );
        let runtime = service.runtime.as_ref().unwrap();
        let presence = runtime.block_on(presence).unwrap();
        let result = runtime.block_on(receiver).unwrap();
        assert!(matches!(result, Err(HttpError::InvalidBody(_))));
        assert!(presence.found.is_empty());
        assert!(
            presence.missing.is_empty(),
            "a store error must not clear credential presence"
        );
        assert_eq!(store.reads.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn execution_reports_a_missing_credential_without_calling_status() {
        let workspace = std::env::temp_dir();
        let store = Arc::new(FakeCredentials::new(false));
        let service = ExecutionService::with_credentials(store.clone()).unwrap();
        let request = Request {
            method: Some("GET".into()),
            url: Some("http://127.0.0.1:1/{{token}}".into()),
            ..Request::default()
        };
        let (_cancel_sender, cancellation) = oneshot::channel();
        let (receiver, _, presence) = service.execute(
            native_input(request, workspace.clone()),
            ExecutionOptions::default(),
            None,
            cancellation,
        );
        let runtime = service.runtime.as_ref().unwrap();
        let presence = runtime.block_on(presence).unwrap();
        let result = runtime.block_on(receiver).unwrap();
        assert!(result.is_err());
        let id = CredentialId::for_workspace(&workspace, "production", "token").unwrap();
        assert!(presence.missing.contains(id.persistence_key()));
        assert!(presence.found.is_empty());
        assert!(!id.persistence_key().contains("token"));
        assert_eq!(store.reads.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn execution_marks_an_empty_stored_secret_present_and_substitutes_it() {
        use std::net::TcpListener;
        let workspace = std::env::temp_dir();
        let store = Arc::new(FakeCredentials::new(false));
        let id = CredentialId::for_workspace(&workspace, "production", "token").unwrap();
        store.set(&id, "").unwrap();
        let service = ExecutionService::with_credentials(store.clone()).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut received = Vec::new();
            let mut buffer = [0; 1024];
            while !received.windows(4).any(|part| part == b"\r\n\r\n") {
                let count = stream.read(&mut buffer).unwrap();
                assert!(count > 0);
                received.extend_from_slice(&buffer[..count]);
            }
            let request = String::from_utf8_lossy(&received);
            assert!(
                request.contains("X-Empty: \r\n") || request.contains("x-empty: \r\n"),
                "an empty stored secret is substituted as an empty value: {request}"
            );
            stream
                .write_all(
                    b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .unwrap();
        });
        let request = Request {
            method: Some("GET".into()),
            url: Some(format!("http://{address}/")),
            headers: vec![Header {
                name: "X-Empty".into(),
                value: "{{token}}".into(),
                disabled: false,
            }],
            ..Request::default()
        };
        let (_cancel_sender, cancellation) = oneshot::channel();
        let (receiver, _, presence) = service.execute(
            native_input(request, workspace),
            ExecutionOptions::default(),
            None,
            cancellation,
        );
        let runtime = service.runtime.as_ref().unwrap();
        let presence = runtime.block_on(presence).unwrap();
        let result = runtime.block_on(receiver).unwrap();
        server.join().unwrap();
        assert!(result.is_ok());
        assert!(presence.found.contains(id.persistence_key()));
        assert!(presence.missing.is_empty());
        assert_eq!(store.reads.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn secret_send_rejects_direct_file_output_before_network_or_disk_io() {
        let workspace = std::env::temp_dir();
        let store = Arc::new(FakeCredentials::new(false));
        let id = CredentialId::for_workspace(&workspace, "production", "token").unwrap();
        store
            .set(&id, "SUPER_SECRET_VALUE_THAT_MUST_NEVER_APPEAR")
            .unwrap();
        let service = ExecutionService::with_credentials(store.clone()).unwrap();
        let output = workspace.join(format!(
            "probe-secret-output-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let request = Request {
            method: Some("GET".into()),
            url: Some("http://127.0.0.1:1/{{token}}".into()),
            ..Request::default()
        };
        let (_cancel_sender, cancellation) = oneshot::channel();
        let (receiver, _, _) = service.execute(
            native_input(request, workspace),
            ExecutionOptions::default(),
            Some(output.clone()),
            cancellation,
        );
        let result = service
            .runtime
            .as_ref()
            .unwrap()
            .block_on(receiver)
            .unwrap();
        assert!(matches!(result, Err(HttpError::InvalidBody(_))));
        assert!(!output.exists());
        assert_eq!(store.reads.load(Ordering::Relaxed), 1);
    }

    #[test]
    #[allow(clippy::result_large_err)] // Test peer's handshake callback fixes its HTTP error type.
    fn websocket_desktop_adapter_sends_initial_once_and_literal_runtime_then_closes() {
        use crate::websocket::Update;
        use futures_util::{SinkExt, StreamExt};
        use probe_application::{SessionData, SessionEvent};
        use probe_core::{
            RequestKind, WebSocketMessage, WebSocketMessageKind, WebSocketMessageSet,
        };
        let workspace = std::env::temp_dir();
        let secret = "WEBSOCKET_NATIVE_SECRET";
        let store = Arc::new(FakeCredentials::new(false));
        let id = CredentialId::for_workspace(&workspace, "production", "token").unwrap();
        store.set(&id, secret).unwrap();
        let service = ExecutionService::with_credentials(store).unwrap();
        service.runtime.as_ref().unwrap().block_on(async {
            tokio::time::timeout(Duration::from_secs(10), async {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let address = listener.local_addr().unwrap();
                let server = tokio::spawn(async move {
                    let (stream, _) = listener.accept().await.unwrap();
                    let mut socket = tokio_tungstenite::accept_hdr_async(stream, |request: &tokio_tungstenite::tungstenite::handshake::server::Request, response| {
                        assert!(request.uri().to_string().contains(secret));
                        assert_eq!(request.headers()["X-Secret"], secret);
                        Ok(response)
                    }).await.unwrap();
                    let first = socket.next().await.unwrap().unwrap();
                    assert_eq!(first.to_text().unwrap(), secret);
                    socket.send(first).await.unwrap();
                    let runtime = socket.next().await.unwrap().unwrap();
                    assert_eq!(runtime.to_text().unwrap(), "{{token}}");
                    socket.send(tokio_tungstenite::tungstenite::Message::binary(vec![0, 1, 2])).await.unwrap();
                    let close = socket.next().await.unwrap().unwrap();
                    assert!(close.is_close(), "desktop must not duplicate initial sends");
                    socket.flush().await.unwrap();
                });
                let request = Request { url: Some(format!("ws://{address}/{{{{token}}}}")), headers: vec![Header { name: "X-Secret".into(), value: "{{token}}".into(), disabled: false }], kind: RequestKind::WebSocket { message: Some(WebSocketMessageSet::Single(WebSocketMessage { kind: WebSocketMessageKind::Text, data: "{{token}}".into() })) }, ..Request::default() };
                let (_cancel, cancellation) = oneshot::channel();
                let (mut events, presence) = service.connect_websocket(native_input(request, workspace), cancellation);
                let presence = presence.await.unwrap();
                assert!(presence.found.contains(id.persistence_key()));
                let Some(Update::Ready(sender)) = events.recv().await else { panic!("expected command handle"); };
                let mut observed = Vec::new();
                for _ in 0..3 { let Some(Update::Event(entry)) = events.recv().await else { panic!("expected event"); }; assert!(!entry.display.contains(secret)); assert!(!format!("{:?}", entry.event).contains(secret)); observed.push(entry.event); }
                assert!(matches!(&observed[0], SessionEvent::Opened { url } if url.contains("{{token}}")));
                assert!(matches!(&observed[1], SessionEvent::Sent(SessionData::Text(text)) if !text.contains(secret)));
                assert!(matches!(&observed[2], SessionEvent::Received(SessionData::Text(text)) if text == "[REDACTED]"));
                service.send_websocket(sender.clone(), SessionData::Text("{{token}}".into())).await.unwrap().unwrap();
                let Some(Update::Event(sent)) = events.recv().await else { panic!(); };
                assert_eq!(sent.event, SessionEvent::Sent(SessionData::Text("{{token}}".into())));
                let Some(Update::Event(binary)) = events.recv().await else { panic!(); };
                assert!(binary.display.contains("AAEC"));
                sender.close();
                let Some(Update::Event(closed)) = events.recv().await else { panic!(); };
                assert!(matches!(closed.event, SessionEvent::Closed { origin: probe_application::CloseOrigin::Local, .. }));
                assert!(events.recv().await.is_none());
                server.await.unwrap();
            }).await.unwrap();
        });
    }

    #[test]
    fn websocket_remote_close_is_safe_and_desktop_bridge_backpressures() {
        use crate::websocket::{BRIDGE_CAPACITY, Update};
        use futures_util::SinkExt;
        use probe_application::{CloseOrigin, SessionData, SessionEvent};
        let workspace = std::env::temp_dir();
        let secret = "REMOTE_CLOSE_SECRET";
        let store = Arc::new(FakeCredentials::new(false));
        let id = CredentialId::for_workspace(&workspace, "production", "token").unwrap();
        store.set(&id, secret).unwrap();
        let service = ExecutionService::with_credentials(store).unwrap();
        service.runtime.as_ref().unwrap().block_on(async {
            tokio::time::timeout(Duration::from_secs(10), async {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let url = format!("ws://{}/{{{{token}}}}", listener.local_addr().unwrap());
                let (burst_sent_tx, burst_sent_rx) = oneshot::channel();
                let server = tokio::spawn(async move {
                    let (stream, _) = listener.accept().await.unwrap();
                    let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
                    for index in 0..100 { socket.send(tokio_tungstenite::tungstenite::Message::text(index.to_string())).await.unwrap(); }
                    socket.close(Some(tokio_tungstenite::tungstenite::protocol::CloseFrame { code: tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::Normal, reason: secret.into() })).await.unwrap();
                    burst_sent_tx.send(()).unwrap();
                    // Wait for the peer acknowledgement rather than dropping TCP.
                    use futures_util::StreamExt;
                    let _ = socket.next().await;
                });
                let (_cancel, cancellation) = oneshot::channel();
                let input = native_input(Request { url: Some(url), kind: probe_core::RequestKind::WebSocket { message: None }, ..Default::default() }, workspace);
                let (mut events, _) = service.connect_websocket(input, cancellation);
                burst_sent_rx.await.unwrap();
                let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
                while events.len() < BRIDGE_CAPACITY {
                    assert!(tokio::time::Instant::now() < deadline, "desktop bridge did not fill before the deadline");
                    tokio::task::yield_now().await;
                }
                assert_eq!(events.len(), BRIDGE_CAPACITY, "a stalled desktop bridge is bounded");
                let mut received = 0;
                let mut closed = false;
                while let Some(update) = events.recv().await {
                    if let Update::Event(entry) = update {
                        assert!(!entry.display.contains(secret));
                        match entry.event {
                            SessionEvent::Received(SessionData::Text(text)) => { assert_eq!(text, received.to_string()); received += 1; }
                            SessionEvent::Closed { origin, code, reason } => { assert_eq!(origin, CloseOrigin::Remote); assert_eq!(code, Some(1000)); assert_eq!(reason, "[REDACTED]"); closed = true; }
                            SessionEvent::Error(error) => panic!("unexpected {error}"),
                            _ => {}
                        }
                    }
                }
                assert_eq!(received, 100); assert!(closed); server.await.unwrap();
            }).await.unwrap();
        });
    }

    #[test]
    fn websocket_connect_can_be_cancelled_and_failure_has_one_typed_error() {
        use crate::websocket::Update;
        use probe_application::SessionEvent;
        let service = ExecutionService::with_credentials(Arc::new(NativeCredentialStore)).unwrap();
        service.runtime.as_ref().unwrap().block_on(async {
            tokio::time::timeout(Duration::from_secs(5), async {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let input = || ExecutionInput { request: Request { url: Some(format!("ws://{}", listener.local_addr().unwrap())), kind: probe_core::RequestKind::WebSocket { message: None }, ..Request::default() }, environments: vec![], client_certificates: vec![], environment: None, credential_workspace: None };
                let (cancel, cancellation) = oneshot::channel();
                let (mut events, _) = service.connect_websocket(input(), cancellation);
                let (_stream, _) = listener.accept().await.unwrap();
                cancel.send(()).unwrap();
                assert!(events.recv().await.is_none());
                let mut failed = input();
                failed.request.url = Some("ws://127.0.0.1:1".into());
                let (_cancel, cancellation) = oneshot::channel();
                let (mut events, _) = service.connect_websocket(failed, cancellation);
                assert!(matches!(events.recv().await, Some(Update::Event(entry)) if matches!(entry.event, SessionEvent::Error(_))));
                assert!(matches!(events.recv().await, Some(Update::Event(entry)) if matches!(entry.event, SessionEvent::Closed { .. })));
                assert!(events.recv().await.is_none());
            }).await.unwrap();
        });
    }

    fn response(status: u16) -> HttpResponse {
        HttpResponse {
            status,
            initial_url: "https://example.com/start".to_owned(),
            url_changed: false,
            reason: String::new(),
            url: String::new(),
            duration: Duration::ZERO,
            size: 0,
            headers: Vec::new(),
            body: Vec::new(),
            body_complete: true,
            body_file: None,
            body_retention_error: None,
        }
    }

    #[test]
    fn body_file_paths_prefer_workspace_relative_storage() {
        let workspace = std::path::Path::new("/tmp/collection/opencollection.yml");
        assert_eq!(
            body_file_path_for_storage(
                std::path::Path::new("/tmp/collection/archive.zip"),
                Some(workspace),
            ),
            "./archive.zip"
        );
        assert_eq!(
            body_file_path_for_storage(
                std::path::Path::new("/tmp/collection/assets/data.json"),
                Some(workspace),
            ),
            "assets/data.json"
        );
    }

    #[test]
    fn body_file_paths_fall_back_to_absolute_storage() {
        let workspace = std::path::Path::new("/tmp/collection/opencollection.yml");
        assert_eq!(
            body_file_path_for_storage(std::path::Path::new("/etc/hosts"), Some(workspace),),
            "/etc/hosts"
        );
    }
}
