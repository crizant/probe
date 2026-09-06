use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use crate::filesystem::workspace_base_directory;
use atomic_write_file::AtomicWriteFile;
use directories::{ProjectDirs, UserDirs};
use probe_core::{HttpRequest, RequestKey};
use probe_http::{
    ExecutionOptions, HttpEngine, HttpError, HttpProgress, HttpResponse, ResponseBodyFile,
    ResponseCache,
};
use sha2::{Digest, Sha256};
use tokio::sync::oneshot;

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
        response: HttpResponse,
        saved_to: Option<SavedResponseBody>,
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
}

#[derive(Default)]
pub(crate) struct ExecutionState {
    next_generation: u64,
    responses: BTreeMap<RequestKey, ResponseState>,
    active: BTreeMap<RequestKey, ActiveRequest>,
    key_aliases: BTreeMap<RequestKey, RequestKey>,
}

impl ExecutionState {
    pub(crate) fn begin(&mut self, key: RequestKey, cancellation: oneshot::Sender<()>) -> u64 {
        self.cancel(key);
        self.next_generation = self.next_generation.wrapping_add(1);
        let generation = self.next_generation;
        self.active.insert(
            key,
            ActiveRequest {
                generation,
                cancellation,
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

    pub(crate) fn finish(
        &mut self,
        key: RequestKey,
        generation: u64,
        result: Result<HttpResponse, HttpError>,
        saved_to: Option<SavedResponseBody>,
    ) {
        let key = self.resolve_key(key);
        if self
            .active
            .get(&key)
            .is_none_or(|active| active.generation != generation)
        {
            return;
        }
        self.active.remove(&key);
        self.key_aliases.retain(|_, target| *target != key);
        let response = match result {
            Ok(response) => ResponseState::Complete { response, saved_to },
            Err(HttpError::Cancelled) => ResponseState::Cancelled,
            Err(error) => ResponseState::Failed(error.to_string()),
        };
        self.responses.insert(key, response);
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

pub(crate) fn execute_http_request<P>(
    request: HttpRequest,
    options: ExecutionOptions,
    output: Option<PathBuf>,
    cancellation: oneshot::Receiver<()>,
    progress: P,
) -> Result<(HttpResponse, Option<SavedResponseBody>), HttpError>
where
    P: FnMut(HttpProgress) + Send,
{
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| HttpError::ClientConfiguration(error.to_string()))?;
    runtime.block_on(async move {
        let engine = HttpEngine::new()?;
        let cancellation = async move {
            let _ = cancellation.await;
        };
        match output {
            Some(output) => {
                let streamed = engine
                    .execute_cancellable_to_file_with_progress(
                        &request,
                        &options,
                        &output,
                        cancellation,
                        progress,
                    )
                    .await?;
                Ok((
                    streamed.response,
                    Some(SavedResponseBody {
                        path: output,
                        sha256: streamed.body_sha256,
                    }),
                ))
            }
            None => {
                let response = engine
                    .execute_cancellable_with_progress(&request, &options, cancellation, progress)
                    .await?;
                Ok((response, None))
            }
        }
    })
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

pub(crate) fn suggested_request_filename(request: &HttpRequest) -> String {
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
        format!("{:.1} MB", size as f64 / (1024.0 * 1024.0))
    } else if size >= 1024 {
        format!("{:.1} KB", size as f64 / 1024.0)
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
        collections::BTreeMap,
        fs,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    use probe_core::HttpRequest;
    use probe_http::ExecutionOptions;
    use probe_http::{HttpError, HttpProgress, HttpResponse, ResponseHeader};
    use sha2::{Digest, Sha256};
    use tokio::sync::oneshot;

    use super::{
        ExecutionState, ResponseState, SavedResponseBody, body_file_path_for_storage,
        execute_http_request, format_duration, format_size, format_transfer_progress,
        save_response_body, suggested_request_filename, suggested_response_filename,
    };

    fn key() -> probe_core::RequestKey {
        let workspace = probe_core::Workspace::from_collection(probe_core::Collection {
            items: vec![probe_core::CollectionItem::HttpRequest(
                probe_core::HttpRequest::default(),
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
                probe_core::CollectionItem::HttpRequest(probe_core::HttpRequest::default()),
                probe_core::CollectionItem::HttpRequest(probe_core::HttpRequest::default()),
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

        state.finish(key, first_generation, Ok(response(201)), None);
        assert!(matches!(
            state.response(key),
            Some(ResponseState::Running { .. })
        ));
        state.finish(key, second_generation, Ok(response(204)), None);
        assert!(matches!(
            state.response(key),
            Some(ResponseState::Complete { response, .. }) if response.status == 204
        ));
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

        state.finish(key, generation, Ok(response(200)), None);
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

        state.finish(old_key, generation, Ok(response(202)), None);

        assert!(matches!(
            state.response(new_key),
            Some(ResponseState::Complete { response, .. }) if response.status == 202
        ));
        assert!(state.response(old_key).is_none());
    }

    #[test]
    fn metadata_units_are_readable() {
        assert_eq!(format_duration(Duration::from_millis(83)), "83 ms");
        assert_eq!(format_duration(Duration::from_millis(1250)), "1.25 s");
        assert_eq!(format_size(812), "812 B");
        assert_eq!(format_size(2048), "2.0 KB");
        assert_eq!(
            format_transfer_progress(40_265_318, Some(120 * 1024 * 1024)),
            "38.4 MB of 120.0 MB"
        );
        assert_eq!(format_transfer_progress(2048, None), "2.0 KB received");
    }

    #[test]
    fn download_filenames_prefer_headers_then_url_and_are_sanitized() {
        let request = HttpRequest {
            url: Some("https://example.test/files/monthly%20report.csv?token=secret".to_owned()),
            ..HttpRequest::default()
        };
        assert_eq!(suggested_request_filename(&request), "monthly report.csv");
        assert_eq!(
            suggested_request_filename(&HttpRequest {
                url: Some("https://example.test".to_owned()),
                ..HttpRequest::default()
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
        let result = execute_http_request(
            HttpRequest {
                method: Some("GET".to_owned()),
                url: Some("http://127.0.0.1:1/phase-12".to_owned()),
                ..HttpRequest::default()
            },
            ExecutionOptions::default(),
            None,
            cancellation,
            |_| {},
        );
        assert_eq!(result, Err(HttpError::Cancelled));
    }

    fn response(status: u16) -> HttpResponse {
        HttpResponse {
            status,
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
