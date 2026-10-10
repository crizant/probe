//! Protocol-neutral streaming boundary. One task serializes sends, receives and
//! events; interfaces never own a socket or see execution material.

use std::{collections::VecDeque, fmt};

use probe_core::WebSocketMessageKind;
use probe_websocket::{
    WebSocketConnection, WebSocketData, WebSocketError, WebSocketEvent, WebSocketRequest,
};
use tokio::sync::{mpsc, oneshot, watch};

use super::{PreparedRequest, SecretDisclosure};

const CHANNEL_CAPACITY: usize = 16;

/// Data supported by the shared streaming boundary. Outbound caller data is sent
/// literally; configured data goes through prepare_request's existing resolution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionData {
    Text(String),
    Binary(Vec<u8>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CloseOrigin {
    Local,
    Remote,
    Error,
}

/// Ordered presentation-safe events. A successful open is first; successful sends
/// precede subsequent receives. Terminal errors precede exactly one Closed event.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionEvent {
    Opened {
        url: String,
    },
    Sent(SessionData),
    Received(SessionData),
    Error(SessionError),
    Closed {
        origin: CloseOrigin,
        code: Option<u16>,
        reason: String,
    },
}

/// Stable session failures with no request-derived or peer diagnostic strings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionError {
    NotWebSocket,
    InvalidMessageSelection,
    UnsupportedBinaryMessage,
    Configuration,
    Timeout,
    CloseTimeout,
    KeepAliveTimeout,
    Connection,
    Tls,
    Handshake,
    Protocol,
    Capacity,
    Closed,
}
impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotWebSocket => "request is not a native WebSocket request",
            Self::InvalidMessageSelection => {
                "WebSocket message variants require exactly one selection"
            }
            Self::UnsupportedBinaryMessage => {
                "configured binary WebSocket messages have no supported encoding contract"
            }
            Self::Configuration => "invalid streaming session configuration",
            Self::Timeout => "streaming connection timed out",
            Self::CloseTimeout => "streaming close handshake timed out",
            Self::KeepAliveTimeout => "streaming keep-alive timed out",
            Self::Connection => "streaming connection failed",
            Self::Tls => "streaming TLS failed",
            Self::Handshake => "streaming handshake failed",
            Self::Protocol => "streaming protocol failed",
            Self::Capacity => "streaming message exceeds capacity",
            Self::Closed => "streaming session is closed",
        })
    }
}
impl std::error::Error for SessionError {}
impl From<WebSocketError> for SessionError {
    fn from(error: WebSocketError) -> Self {
        match error {
            WebSocketError::NotWebSocket => Self::NotWebSocket,
            WebSocketError::Timeout => Self::Timeout,
            WebSocketError::CloseTimeout => Self::CloseTimeout,
            WebSocketError::KeepAliveTimeout => Self::KeepAliveTimeout,
            WebSocketError::Connection => Self::Connection,
            WebSocketError::Tls => Self::Tls,
            WebSocketError::Handshake => Self::Handshake,
            WebSocketError::Protocol => Self::Protocol,
            WebSocketError::Capacity => Self::Capacity,
            WebSocketError::Closed => Self::Closed,
            _ => Self::Configuration,
        }
    }
}

/// Prepared WebSocket execution. Private fields retain resolved material only for
/// network I/O and redaction; Debug never prints it.
pub struct WebSocketExecution {
    request: WebSocketRequest,
    initial: Option<String>,
    initial_presentation: Option<String>,
    url: String,
    disclosure: SecretDisclosure,
}
impl fmt::Debug for WebSocketExecution {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WebSocketExecution")
            .field("uses_secrets", &self.disclosure.uses_secrets())
            .finish_non_exhaustive()
    }
}
impl PreparedRequest {
    /// Validate the native protocol and selected message before any network I/O.
    /// Binary collection data is deliberately rejected: its String representation
    /// does not establish a binary encoding contract.
    pub fn into_websocket(self) -> Result<WebSocketExecution, SessionError> {
        if !self.execution.kind.is_websocket() {
            return Err(SessionError::NotWebSocket);
        }
        let message = self
            .execution
            .selected_websocket_message()
            .map_err(|_| SessionError::InvalidMessageSelection)?;
        if message.is_some_and(|m| m.kind == WebSocketMessageKind::Binary) {
            return Err(SessionError::UnsupportedBinaryMessage);
        }
        let initial = message.map(|m| m.data.clone());
        let initial_presentation = self
            .presentation
            .selected_websocket_message()
            .map_err(|_| SessionError::InvalidMessageSelection)?
            .map(|m| self.disclosure.redact_text(&m.data));
        Ok(WebSocketExecution {
            request: WebSocketRequest::new(&self.execution)?,
            initial,
            initial_presentation,
            url: self
                .disclosure
                .redact_text(self.presentation.url.as_deref().unwrap_or_default()),
            disclosure: self.disclosure,
        })
    }
}
impl WebSocketExecution {
    /// Connect using the execution form and return a presentation-safe session.
    /// Dropping this future cancels connection; dropping Session requests bounded
    /// close-handshake cleanup even when the event queue is full.
    pub async fn connect(self) -> Result<Session, SessionError> {
        let connection = self.request.connect().await?;
        let (commands, command_rx) = mpsc::channel(CHANNEL_CAPACITY);
        let (event_tx, events) = mpsc::channel(CHANNEL_CAPACITY);
        let (close, close_rx) = watch::channel(false);
        let (finished_tx, finished) = watch::channel(false);
        // Capacity is nonzero; enqueue before spawn so Opened is always first.
        event_tx
            .try_send(SessionEvent::Opened { url: self.url })
            .expect("empty event channel");
        let (terminal_tx, terminal) = oneshot::channel();
        tokio::spawn(run_session(
            connection,
            command_rx,
            event_tx,
            close_rx,
            Runtime {
                finished: finished_tx,
                terminal: terminal_tx,
                initial: self.initial,
                initial_presentation: self.initial_presentation,
                disclosure: self.disclosure,
            },
        ));
        Ok(Session {
            sender: SessionSender {
                commands,
                close,
                finished,
            },
            events,
            terminal: Some(terminal),
            tail: VecDeque::new(),
        })
    }
}

/// A bounded command/event session. send waits for command capacity; Sent confirms
/// the write. Consumers should drain events concurrently with large send batches.
/// Backpressure pauses network reads rather than accumulating unbounded payloads.
/// close uses a separate signal so it remains responsive with either queue full.
/// No transport types, terminal I/O or UI entities cross this boundary.
pub struct Session {
    sender: SessionSender,
    events: mpsc::Receiver<SessionEvent>,
    terminal: Option<oneshot::Receiver<VecDeque<SessionEvent>>>,
    tail: VecDeque<SessionEvent>,
}
impl fmt::Debug for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session").finish_non_exhaustive()
    }
}
/// Cloneable bounded command handle, so consumers can send while another task
/// drains events. Dropping the owning Session shuts down even with live handles.
#[derive(Clone)]
pub struct SessionSender {
    commands: mpsc::Sender<SessionData>,
    close: watch::Sender<bool>,
    finished: watch::Receiver<bool>,
}
impl fmt::Debug for SessionSender {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionSender").finish_non_exhaustive()
    }
}
impl SessionSender {
    /// Queue literal data. Sent confirms writing; close may cancel queued writes.
    pub async fn send(&self, data: SessionData) -> Result<(), SessionError> {
        let mut closing = self.close.subscribe();
        if *closing.borrow() || *self.finished.borrow() {
            return Err(SessionError::Closed);
        }
        tokio::select! {
            biased;
            _ = closing.changed() => Err(SessionError::Closed),
            result = self.commands.send(data) => result.map_err(|_| SessionError::Closed),
        }
    }
    /// Request normal close, idempotently. Continue reading to observe Closed.
    pub fn close(&self) {
        self.close.send_replace(true);
    }
}
impl Session {
    /// Get a command handle for sending concurrently with event consumption.
    #[must_use]
    pub fn sender(&self) -> SessionSender {
        self.sender.clone()
    }
    /// Queue literal data; original request secrets are redacted in the Sent event.
    pub async fn send(&self, data: SessionData) -> Result<(), SessionError> {
        self.sender.send(data).await
    }
    pub fn close(&self) {
        self.sender.close();
    }
    pub async fn next_event(&mut self) -> Option<SessionEvent> {
        if let Some(event) = self.events.recv().await {
            return Some(event);
        }
        if let Some(terminal) = &mut self.terminal {
            // Retain the receiver if next_event is cancelled while awaiting it.
            self.tail = terminal.await.unwrap_or_default();
            self.terminal = None;
        }
        self.tail.pop_front()
    }
    /// Wait for socket/task cleanup. Does not require draining the event queue;
    /// terminal events may still be waiting for delivery after cleanup finishes.
    pub async fn wait_closed(&mut self) {
        while !*self.sender.finished.borrow_and_update() {
            if self.sender.finished.changed().await.is_err() {
                break;
            }
        }
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        self.sender.close();
    }
}

struct End {
    origin: CloseOrigin,
    code: Option<u16>,
    reason: String,
}

struct Runtime {
    finished: watch::Sender<bool>,
    terminal: oneshot::Sender<VecDeque<SessionEvent>>,
    initial: Option<String>,
    initial_presentation: Option<String>,
    disclosure: SecretDisclosure,
}

async fn run_session(
    mut connection: WebSocketConnection,
    mut commands: mpsc::Receiver<SessionData>,
    events: mpsc::Sender<SessionEvent>,
    mut close: watch::Receiver<bool>,
    runtime: Runtime,
) {
    let Runtime {
        finished,
        terminal,
        initial,
        initial_presentation,
        disclosure,
    } = runtime;
    let result = tokio::select! {
        biased;
        _ = close.changed() => Ok(End { origin: CloseOrigin::Local, code: None, reason: String::new() }),
        _ = events.closed() => Ok(End { origin: CloseOrigin::Local, code: None, reason: String::new() }),
        result = drive_session(&mut connection, &mut commands, &events, initial, initial_presentation, &disclosure) => result,
    };
    // Flush the remote acknowledgement or initiate local close before reporting
    // terminal events. No await follows parsing Close inside receive itself.
    let shutdown = connection.close().await;
    // Terminal delivery has a separate fixed-size slot (at most error + close).
    // Thus a full data queue cannot retain a closed task or secret material.
    drop(connection);
    drop(commands);
    let mut tail = VecDeque::with_capacity(2);
    if let Err(error) = result.as_ref() {
        tail.push_back(SessionEvent::Error(*error));
    }
    if let Err(error) = shutdown
        && result.is_ok()
    {
        tail.push_back(SessionEvent::Error(error.into()));
    }
    let end = result.unwrap_or_else(|_| End {
        origin: CloseOrigin::Error,
        code: None,
        reason: String::new(),
    });
    tail.push_back(SessionEvent::Closed {
        origin: end.origin,
        code: end.code,
        reason: disclosure.redact_text(&end.reason),
    });
    let _ = terminal.send(tail);
    drop(disclosure);
    drop(events);
    finished.send_replace(true);
}

async fn drive_session(
    connection: &mut WebSocketConnection,
    commands: &mut mpsc::Receiver<SessionData>,
    events: &mpsc::Sender<SessionEvent>,
    initial: Option<String>,
    initial_presentation: Option<String>,
    disclosure: &SecretDisclosure,
) -> Result<End, SessionError> {
    if let Some(initial) = initial {
        let presentation = initial_presentation.ok_or(SessionError::InvalidMessageSelection)?;
        connection.send(WebSocketData::Text(initial)).await?;
        events
            .send(SessionEvent::Sent(SessionData::Text(presentation)))
            .await
            .map_err(|_| SessionError::Closed)?;
    }
    loop {
        tokio::select! {
            // Each write and its Sent event complete before another read.
            command = commands.recv() => {
                let Some(data) = command else { return Ok(End { origin: CloseOrigin::Local, code: None, reason: String::new() }); };
                let presentation = disclosure.redact_data(&data);
                let data = match data { SessionData::Text(text) => WebSocketData::Text(text), SessionData::Binary(bytes) => WebSocketData::Binary(bytes) };
                connection.send(data).await?;
                events.send(SessionEvent::Sent(presentation)).await.map_err(|_| SessionError::Closed)?;
            }
            incoming = connection.receive() => match incoming? {
                WebSocketEvent::Data(data) => {
                    let data = match data { WebSocketData::Text(text) => SessionData::Text(text), WebSocketData::Binary(bytes) => SessionData::Binary(bytes) };
                    events.send(SessionEvent::Received(disclosure.redact_data(&data))).await.map_err(|_| SessionError::Closed)?;
                }
                WebSocketEvent::Closed { code, reason } => return Ok(End { origin: CloseOrigin::Remote, code, reason }),
            }
        }
    }
}
impl SecretDisclosure {
    fn redact_text(&self, text: &str) -> String {
        match &self.secrets {
            Some(secrets) => secrets.redact_secrets(text),
            None => text.to_owned(),
        }
    }
    fn redact_data(&self, data: &SessionData) -> SessionData {
        match data {
            SessionData::Text(text) => SessionData::Text(self.redact_text(text)),
            SessionData::Binary(bytes) => SessionData::Binary(match &self.secrets {
                Some(secrets) => secrets.redact_secret_bytes(bytes),
                None => bytes.clone(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::{SinkExt, StreamExt};
    use probe_core::{Request, RequestKind};
    use tokio::net::TcpListener;
    use tokio_tungstenite::{accept_async, tungstenite::Message};

    #[test]
    fn transport_errors_preserve_safe_session_categories() {
        for (transport, expected) in [
            (WebSocketError::NotWebSocket, SessionError::NotWebSocket),
            (WebSocketError::Timeout, SessionError::Timeout),
            (WebSocketError::CloseTimeout, SessionError::CloseTimeout),
            (
                WebSocketError::KeepAliveTimeout,
                SessionError::KeepAliveTimeout,
            ),
            (WebSocketError::Connection, SessionError::Connection),
            (WebSocketError::Tls, SessionError::Tls),
            (WebSocketError::Handshake, SessionError::Handshake),
            (WebSocketError::Protocol, SessionError::Protocol),
            (WebSocketError::Capacity, SessionError::Capacity),
            (WebSocketError::Closed, SessionError::Closed),
            (WebSocketError::InvalidHeader, SessionError::Configuration),
        ] {
            let error = SessionError::from(transport);
            assert_eq!(error, expected);
            assert!(!error.to_string().is_empty());
            assert!(std::error::Error::source(&error).is_none());
        }
    }

    fn controlled_session() -> (
        Session,
        watch::Sender<bool>,
        watch::Receiver<bool>,
        mpsc::Receiver<SessionData>,
        mpsc::Sender<SessionEvent>,
    ) {
        let (commands, command_rx) = mpsc::channel(1);
        let (close, closing) = watch::channel(false);
        let (finished_tx, finished) = watch::channel(false);
        let (event_tx, events) = mpsc::channel(1);
        (
            Session {
                sender: SessionSender {
                    commands,
                    close,
                    finished,
                },
                events,
                terminal: None,
                tail: VecDeque::new(),
            },
            finished_tx,
            closing,
            command_rx,
            event_tx,
        )
    }

    #[tokio::test]
    async fn close_and_drop_reject_sends_before_runtime_cleanup() {
        for drop_owner in [false, true] {
            let (session, finished, closing, _commands, _events) = controlled_session();
            let sender = session.sender();
            assert!(!*finished.borrow());
            if drop_owner {
                drop(session);
            } else {
                session.close();
            }
            assert!(
                *closing.borrow(),
                "close must signal synchronously, including on drop"
            );
            assert_eq!(
                sender.send(SessionData::Text("too late".into())).await,
                Err(SessionError::Closed)
            );
        }
    }

    #[tokio::test]
    async fn wait_closed_waits_for_runtime_cleanup_and_is_idempotent() {
        use futures_util::FutureExt;
        let (mut session, finished, _closing, _commands, _events) = controlled_session();
        assert!(session.wait_closed().now_or_never().is_none());
        finished.send_replace(true);
        tokio::time::timeout(std::time::Duration::from_secs(1), session.wait_closed())
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(1), session.wait_closed())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn missing_initial_presentation_is_typed_and_sends_no_message() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let request = Request {
            url: Some(format!("ws://{}", listener.local_addr().unwrap())),
            kind: RequestKind::WebSocket { message: None },
            ..Request::default()
        };
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            assert!(matches!(
                socket.next().await.unwrap().unwrap(),
                Message::Close(_)
            ));
            socket.flush().await.unwrap();
        });
        let mut connection = WebSocketRequest::new(&request)
            .unwrap()
            .connect()
            .await
            .unwrap();
        let (_commands, mut command_rx) = mpsc::channel(1);
        let (events, mut event_rx) = mpsc::channel(1);
        let result = drive_session(
            &mut connection,
            &mut command_rx,
            &events,
            Some("private execution message".into()),
            None,
            &SecretDisclosure::default(),
        )
        .await;
        assert!(matches!(result, Err(SessionError::InvalidMessageSelection)));
        assert!(event_rx.try_recv().is_err());
        connection.close().await.unwrap();
        server.await.unwrap();
    }
}
