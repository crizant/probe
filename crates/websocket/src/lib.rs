//! Async WebSocket transport. No interface concerns or variable resolution live here.
//! Errors intentionally contain no peer diagnostics or request-derived values.

#![forbid(unsafe_code)]

use std::{fmt, sync::Arc, time::Duration};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use futures_util::{SinkExt, StreamExt};
use http::{HeaderMap, HeaderName, HeaderValue};
use probe_core::{Authentication, AuthenticationKind, AuthenticationValue, Request};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::{
    net::TcpStream,
    time::{Instant, Interval, MissedTickBehavior},
};
use tokio_websockets::{Connector, Error as WireError, MaybeTlsStream, Message, WebSocketStream};
use url::Url;

/// Stable error categories, safe even when configuration and peer data contain secrets.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WebSocketError {
    NotWebSocket,
    MissingUrl,
    InvalidUrl,
    InvalidHeader,
    UnsupportedAuthentication,
    MissingAuthenticationProperty {
        scheme: &'static str,
        property: &'static str,
    },
    InvalidApiKeyPlacement,
    Timeout,
    CloseTimeout,
    Connection,
    Tls,
    Handshake,
    Protocol,
    Capacity,
    Closed,
}

impl fmt::Display for WebSocketError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotWebSocket => f.write_str("request is not a native WebSocket request"),
            Self::MissingUrl => f.write_str("WebSocket URL is missing"),
            Self::InvalidUrl => f.write_str("invalid WebSocket URL (expected ws:// or wss://)"),
            Self::InvalidHeader => f.write_str("invalid WebSocket handshake header"),
            Self::UnsupportedAuthentication => {
                f.write_str("unsupported WebSocket authentication scheme")
            }
            Self::MissingAuthenticationProperty { scheme, property } => {
                write!(f, "{scheme} authentication requires {property}")
            }
            Self::InvalidApiKeyPlacement => {
                f.write_str("apikey placement must be 'header' or 'query'")
            }
            Self::Timeout => f.write_str("WebSocket handshake timed out"),
            Self::CloseTimeout => f.write_str("WebSocket close handshake timed out"),
            Self::Connection => f.write_str("WebSocket connection failed"),
            Self::Tls => f.write_str("WebSocket TLS failed"),
            Self::Handshake => f.write_str("WebSocket upgrade failed"),
            Self::Protocol => f.write_str("WebSocket protocol error"),
            Self::Capacity => f.write_str("WebSocket message exceeds transport capacity"),
            Self::Closed => f.write_str("WebSocket connection is closed"),
        }
    }
}
impl std::error::Error for WebSocketError {}

fn classify(error: WireError) -> WebSocketError {
    match error {
        WireError::AlreadyClosed => WebSocketError::Closed,
        WireError::Io(_) | WireError::CannotResolveHost => WebSocketError::Connection,
        WireError::Rustls(_) | WireError::InvalidDNSName(_) => WebSocketError::Tls,
        WireError::Upgrade(_) => WebSocketError::Handshake,
        WireError::PayloadTooLong { .. } => WebSocketError::Capacity,
        _ => WebSocketError::Protocol,
    }
}

/// Validated opening handshake input. Debug deliberately omits execution material.
pub struct WebSocketRequest {
    url: Url,
    headers: HeaderMap,
    timeout: Option<Duration>,
    keep_alive: Option<Duration>,
}
impl fmt::Debug for WebSocketRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WebSocketRequest").finish_non_exhaustive()
    }
}

impl WebSocketRequest {
    /// Prepare already-resolved fields without network I/O. Header and authentication
    /// semantics match probe-http: enabled named headers append; Basic/Bearer replace
    /// Authorization; API-key headers append and query keys append with form encoding.
    pub fn new(request: &Request) -> Result<Self, WebSocketError> {
        if !request.kind.is_websocket() {
            return Err(WebSocketError::NotWebSocket);
        }
        let raw_url = request.url.as_deref().ok_or(WebSocketError::MissingUrl)?;
        let raw_url = probe_core::apply_path_parameters(raw_url, &request.path_parameters);
        let mut url = Url::parse(&raw_url).map_err(|_| WebSocketError::InvalidUrl)?;
        if !matches!(url.scheme(), "ws" | "wss")
            || url.host_str().is_none()
            || url.fragment().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(WebSocketError::InvalidUrl);
        }
        for parameter in request
            .query_parameters
            .iter()
            .filter(|p| !p.disabled && !p.name.is_empty())
        {
            url.query_pairs_mut()
                .append_pair(&parameter.name, &parameter.value);
        }
        let auth = request.authentication.as_ref();
        if let Some(auth) = auth.filter(|a| a.kind == AuthenticationKind::ApiKey) {
            let (key, value, placement) = api_key(auth)?;
            if placement == "query" {
                url.query_pairs_mut().append_pair(key, value);
            }
        }
        let mut headers = HeaderMap::new();
        for header in request
            .headers
            .iter()
            .filter(|h| !h.disabled && !h.name.is_empty())
        {
            append_header(&mut headers, &header.name, &header.value)?;
        }
        if let Some(auth) = auth {
            apply_auth(&mut headers, auth)?;
        }
        Ok(Self {
            url,
            headers,
            timeout: request.settings.timeout.filter(|d| !d.is_zero()),
            keep_alive: request
                .settings
                .keep_alive_interval
                .filter(|d| !d.is_zero()),
        })
    }

    /// Connect with Rustls and web PKI roots for wss; the deadline includes TCP, TLS,
    /// and HTTP upgrade. Dropping this future cancels the opening handshake.
    pub async fn connect(self) -> Result<WebSocketConnection, WebSocketError> {
        // Select the same crypto implementation as Probe HTTP explicitly. Do not
        // depend on another workspace crate installing a global Rustls provider.
        let roots =
            rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let tls = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::aws_lc_rs::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(|_| WebSocketError::Tls)?
        .with_root_certificates(roots)
        .with_no_client_auth();
        let connector = Connector::Rustls(tokio_rustls::TlsConnector::from(Arc::new(tls)));
        let connect = async {
            let host = self
                .url
                .host_str()
                .ok_or(WebSocketError::InvalidUrl)?
                .trim_matches(['[', ']']);
            let port = self
                .url
                .port_or_known_default()
                .ok_or(WebSocketError::InvalidUrl)?;
            let stream = TcpStream::connect((host, port))
                .await
                .map_err(|_| WebSocketError::Connection)?;
            let stream = if self.url.scheme() == "wss" {
                connector.wrap(host, stream).await.map_err(classify)?
            } else {
                MaybeTlsStream::Plain(stream)
            };
            let mut stream = BufReader::new(stream);
            handshake(&mut stream, &self.url, self.headers).await?;
            Ok(tokio_websockets::ClientBuilder::new().take_over(stream))
        };
        let socket = match self.timeout {
            Some(timeout) => tokio::time::timeout(timeout, connect)
                .await
                .map_err(|_| WebSocketError::Timeout)?,
            None => connect.await,
        }?;
        let keep_alive = self.keep_alive.map(|period| {
            let mut interval = tokio::time::interval_at(Instant::now() + period, period);
            interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
            interval
        });
        Ok(WebSocketConnection {
            socket,
            keep_alive,
            remote_closed: false,
        })
    }
}

fn property<'a>(
    auth: &'a Authentication,
    scheme: &'static str,
    property: &'static str,
) -> Result<&'a str, WebSocketError> {
    match auth.properties.get(property) {
        Some(AuthenticationValue::String(value)) => Ok(value),
        _ => Err(WebSocketError::MissingAuthenticationProperty { scheme, property }),
    }
}
fn api_key(auth: &Authentication) -> Result<(&str, &str, &str), WebSocketError> {
    let key = property(auth, "apikey", "key")?;
    if key.is_empty() {
        return Err(WebSocketError::MissingAuthenticationProperty {
            scheme: "apikey",
            property: "key",
        });
    }
    let value = property(auth, "apikey", "value")?;
    let placement = property(auth, "apikey", "placement")?;
    if !matches!(placement, "header" | "query") {
        return Err(WebSocketError::InvalidApiKeyPlacement);
    }
    Ok((key, value, placement))
}
fn append_header(headers: &mut HeaderMap, name: &str, value: &str) -> Result<(), WebSocketError> {
    let name =
        HeaderName::from_bytes(name.as_bytes()).map_err(|_| WebSocketError::InvalidHeader)?;
    let value = HeaderValue::from_str(value).map_err(|_| WebSocketError::InvalidHeader)?;
    headers.append(name, value);
    Ok(())
}
fn apply_auth(headers: &mut HeaderMap, auth: &Authentication) -> Result<(), WebSocketError> {
    let authorization = match auth.kind {
        AuthenticationKind::Basic => {
            let username = property(auth, "basic", "username")?;
            let password = property(auth, "basic", "password")?;
            format!(
                "Basic {}",
                STANDARD.encode(format!("{username}:{password}"))
            )
        }
        AuthenticationKind::Bearer => format!("Bearer {}", property(auth, "bearer", "token")?),
        AuthenticationKind::ApiKey => {
            let (key, value, placement) = api_key(auth)?;
            if placement == "header" {
                append_header(headers, key, value)?;
            }
            return Ok(());
        }
        _ => return Err(WebSocketError::UnsupportedAuthentication),
    };
    let value = HeaderValue::from_str(&authorization).map_err(|_| WebSocketError::InvalidHeader)?;
    headers.insert(http::header::AUTHORIZATION, value);
    Ok(())
}

/// Wire data without tungstenite types. These bytes have NOT been redacted;
/// presentation consumers must use probe-application's Session instead.
pub enum WebSocketData {
    Text(String),
    Binary(Vec<u8>),
}
/// Transport receive result. Ping/pong are consumed internally.
pub enum WebSocketEvent {
    Data(WebSocketData),
    Closed { code: Option<u16>, reason: String },
}

/// Owned socket; no background tasks. Drop releases the connection immediately.
pub struct WebSocketConnection {
    socket: WebSocketStream<BufReader<MaybeTlsStream<TcpStream>>>,
    keep_alive: Option<Interval>,
    remote_closed: bool,
}
impl fmt::Debug for WebSocketConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WebSocketConnection")
            .finish_non_exhaustive()
    }
}
impl WebSocketConnection {
    pub async fn send(&mut self, data: WebSocketData) -> Result<(), WebSocketError> {
        if self.remote_closed {
            return Err(WebSocketError::Closed);
        }
        let message = match data {
            WebSocketData::Text(text) => Message::text(text),
            WebSocketData::Binary(bytes) => Message::binary(bytes),
        };
        self.socket.send(message).await.map_err(classify)
    }
    /// Keep polling while connected to drive ping/pong and keep-alive. The
    /// application runtime owns this polling independently of interface activity.
    /// On Closed, call close() to flush the peer acknowledgement before dropping
    /// the socket. Returning Close itself is cancellation-safe.
    pub async fn receive(&mut self) -> Result<WebSocketEvent, WebSocketError> {
        loop {
            let incoming = tokio::select! {
                incoming = self.socket.next() => incoming,
                _ = async { match &mut self.keep_alive { Some(interval) => { interval.tick().await; }, None => std::future::pending().await } } => {
                    self.socket.send(Message::ping(Vec::new())).await.map_err(classify)?;
                    continue;
                }
            };
            let message = incoming
                .ok_or(WebSocketError::Connection)?
                .map_err(classify)?;
            if let Some(text) = message.as_text() {
                return Ok(WebSocketEvent::Data(WebSocketData::Text(text.to_owned())));
            }
            if message.is_binary() {
                return Ok(WebSocketEvent::Data(WebSocketData::Binary(
                    message.into_payload().to_vec(),
                )));
            }
            if let Some((code, reason)) = message.as_close() {
                self.remote_closed = true;
                // Return without another await so cancellation cannot lose Close.
                // close() flushes the queued acknowledgement before socket cleanup.
                return Ok(WebSocketEvent::Closed {
                    code: (code != tokio_websockets::CloseCode::NO_STATUS_RECEIVED)
                        .then(|| u16::from(code)),
                    reason: reason.to_owned(),
                });
            }
            // Reading Ping queues its Pong; flush before reading another frame.
            self.socket.flush().await.map_err(classify)?;
        }
    }
    /// Initiate a normal close and drive the peer reply. A noncooperative peer
    /// cannot retain the runtime indefinitely: shutdown has a five-second bound.
    pub async fn close(&mut self) -> Result<(), WebSocketError> {
        tokio::time::timeout(Duration::from_secs(5), async {
            if self.remote_closed {
                self.socket.flush().await.map_err(classify)?;
                return Ok(());
            }
            match self.socket.send(Message::close(None, "")).await {
                Ok(()) => {}
                Err(WireError::AlreadyClosed) => return Ok(()),
                Err(error) => return Err(classify(error)),
            }
            while let Some(message) = self.socket.next().await {
                match message {
                    Ok(message) if message.is_close() => return Ok(()),
                    Err(WireError::AlreadyClosed) => return Ok(()),
                    Err(error) => return Err(classify(error)),
                    _ => {
                        self.socket.flush().await.map_err(classify)?;
                    }
                }
            }
            Ok(())
        })
        .await
        .unwrap_or(Err(WebSocketError::CloseTimeout))
    }
}

/// The library's client builder replaces duplicate headers. Keep HTTP's append
/// semantics with a small, bounded opening handshake, then hand the buffered stream
/// to the frame implementation. BufReader preserves any first frame read with the
/// upgrade response. Neither this code nor tokio-websockets logs network values.
async fn handshake(
    stream: &mut BufReader<MaybeTlsStream<TcpStream>>,
    url: &Url,
    headers: HeaderMap,
) -> Result<(), WebSocketError> {
    let mut random = [0; 16];
    getrandom::fill(&mut random).map_err(|_| WebSocketError::Connection)?;
    let key = STANDARD.encode(random);
    let host = &url[url::Position::BeforeHost..url::Position::AfterPort];
    let path = &url[url::Position::BeforePath..url::Position::AfterQuery];
    let mut request = format!(
        "GET {path} HTTP/1.1\r\nHost: {host}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n"
    ).into_bytes();
    for (name, value) in &headers {
        request.extend_from_slice(name.as_str().as_bytes());
        request.extend_from_slice(b": ");
        request.extend_from_slice(value.as_bytes());
        request.extend_from_slice(b"\r\n");
    }
    request.extend_from_slice(b"\r\n");
    stream
        .write_all(&request)
        .await
        .map_err(|_| WebSocketError::Connection)?;
    stream
        .flush()
        .await
        .map_err(|_| WebSocketError::Connection)?;
    let mut response = Vec::new();
    // Bound both header bytes and parsed header count before accepting an upgrade.
    let mut bounded = (&mut *stream).take(64 * 1024);
    loop {
        let start = response.len();
        if bounded
            .read_until(b'\n', &mut response)
            .await
            .map_err(|_| WebSocketError::Connection)?
            == 0
        {
            return Err(WebSocketError::Handshake);
        }
        if &response[start..] == b"\r\n" {
            break;
        }
    }
    let mut headers = [httparse::EMPTY_HEADER; 128];
    let mut parsed = httparse::Response::new(&mut headers);
    if !parsed
        .parse(&response)
        .map_err(|_| WebSocketError::Handshake)?
        .is_complete()
        || parsed.code != Some(101)
        || parsed.version != Some(1)
    {
        return Err(WebSocketError::Handshake);
    }
    let header = |name: &str| {
        parsed
            .headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(name))
            .map(|h| h.value)
    };
    let expected = STANDARD.encode(
        sha1_smol::Sha1::from(format!("{key}258EAFA5-E914-47DA-95CA-C5AB0DC85B11"))
            .digest()
            .bytes(),
    );
    if header("Sec-WebSocket-Accept") != Some(expected.as_bytes())
        || !header("Upgrade").is_some_and(|v| v.eq_ignore_ascii_case(b"websocket"))
        || !parsed
            .headers
            .iter()
            .filter(|h| h.name.eq_ignore_ascii_case("Connection"))
            .any(|h| {
                h.value
                    .split(|b| *b == b',')
                    .any(|token| token.trim_ascii().eq_ignore_ascii_case(b"upgrade"))
            })
        || header("Sec-WebSocket-Extensions").is_some()
        || header("Sec-WebSocket-Protocol").is_some()
    {
        return Err(WebSocketError::Handshake);
    }
    Ok(())
}
