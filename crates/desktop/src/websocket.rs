//! Desktop-only, bounded presentation state. Transport ownership stays in Session.
use gpui::{ListAlignment, ListState, Rgba, px};
use probe_application::{
    CloseOrigin, SessionData, SessionError, SessionEvent, SessionSender, close_code_name,
};
use probe_core::RequestKey;
use std::{
    collections::{BTreeMap, VecDeque},
    time::Instant,
};
use tokio::sync::oneshot;

use crate::theme::Theme;

pub(crate) const BRIDGE_CAPACITY: usize = 16;
const MAX_ENTRIES: usize = 1000;
const MAX_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConnectionState {
    Connecting,
    Connected,
    Closing,
    Closed,
    Failed,
}
impl ConnectionState {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Connecting => "Connecting…",
            Self::Connected => "Connected",
            Self::Closing => "Closing…",
            Self::Closed => "Disconnected",
            Self::Failed => "Failed",
        }
    }
    pub(crate) fn button(self) -> &'static str {
        match self {
            Self::Connecting => "Cancel",
            Self::Connected => "Disconnect",
            Self::Closing => "Closing…",
            Self::Closed | Self::Failed => "Connect",
        }
    }
}
pub(crate) enum Update {
    Ready(SessionSender),
    Event(TimelineEntry),
}

/// How a WebSocket close should read in the timeline.
///
/// Error closes are server failures. Local closes are user-initiated and stay
/// neutral. Remote closes follow the close code. Informational is unused.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DisconnectSeverity {
    Success,
    Temporary,
    ClientError,
    ServerError,
    Neutral,
}

pub(crate) fn disconnect_severity(origin: CloseOrigin, code: Option<u16>) -> DisconnectSeverity {
    match origin {
        CloseOrigin::Error => DisconnectSeverity::ServerError,
        CloseOrigin::Local => DisconnectSeverity::Neutral,
        CloseOrigin::Remote => match code {
            Some(1000) => DisconnectSeverity::Success,
            Some(1001 | 1012 | 1013 | 3008) => DisconnectSeverity::Temporary,
            Some(1002 | 1003 | 1007 | 1008 | 1009 | 1010 | 3000 | 3003) => {
                DisconnectSeverity::ClientError
            }
            Some(1006 | 1011 | 1014 | 1015) => DisconnectSeverity::ServerError,
            _ => DisconnectSeverity::Neutral,
        },
    }
}

/// Same response palette as HTTP status text, plus muted text for neutral closes.
pub(crate) fn disconnect_severity_color(theme: Theme, severity: DisconnectSeverity) -> Rgba {
    match severity {
        DisconnectSeverity::Success => theme.colors.responses.success,
        DisconnectSeverity::Temporary => theme.colors.responses.redirect,
        DisconnectSeverity::ClientError => theme.colors.responses.client_error,
        DisconnectSeverity::ServerError => theme.colors.responses.server_error,
        DisconnectSeverity::Neutral => theme.colors.text.muted,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DisconnectLine {
    pub(crate) origin: &'static str,
    pub(crate) status: Option<String>,
    pub(crate) reason: Option<String>,
}

impl DisconnectLine {
    pub(crate) fn from_close(origin: CloseOrigin, code: Option<u16>, reason: &str) -> Self {
        let origin = match origin {
            CloseOrigin::Local => "local",
            CloseOrigin::Remote => "remote",
            CloseOrigin::Error => "error",
        };
        let status = code.map(|code| match close_code_name(code) {
            Some(name) => format!("{code} {name}"),
            None => code.to_string(),
        });
        let reason = if reason.trim().is_empty() {
            None
        } else {
            Some(reason.to_owned())
        };
        Self {
            origin,
            status,
            reason,
        }
    }

    pub(crate) fn display(&self) -> String {
        self.text().0
    }

    /// The disconnect sentence and the byte range of its close-code segment.
    pub(crate) fn text(&self) -> (String, Option<std::ops::Range<usize>>) {
        let mut text = format!("Disconnected · {}", self.origin);
        let status = self.status.as_ref().map(|status| {
            text.push_str(" · ");
            let start = text.len();
            text.push_str(status);
            start..text.len()
        });
        if let Some(reason) = &self.reason {
            text.push_str(" · ");
            text.push_str(reason);
        }
        (text, status)
    }
}

pub(crate) struct TimelineEntry {
    pub(crate) event: SessionEvent,
    pub(crate) time: String,
    pub(crate) display: String,
    pub(crate) original: Option<String>,
}
impl TimelineEntry {
    // Called on the execution worker, never parse/format large JSON on GPUI.
    pub(crate) fn new(event: SessionEvent) -> Self {
        let (display, original) = match &event {
            SessionEvent::Sent(SessionData::Text(text))
            | SessionEvent::Received(SessionData::Text(text)) => (
                serde_json::from_str::<serde_json::Value>(text)
                    .ok()
                    .and_then(|v| serde_json::to_string_pretty(&v).ok())
                    .unwrap_or_else(|| text.clone()),
                Some(text.clone()),
            ),
            SessionEvent::Sent(SessionData::Binary(bytes))
            | SessionEvent::Received(SessionData::Binary(bytes)) => {
                let encoded = crate::response_viewer::encode_base64(bytes);
                (
                    format!("Binary · {} B\n{encoded}", bytes.len()),
                    Some(encoded),
                )
            }
            SessionEvent::Opened { url } => (format!("Connected to {url}"), None),
            SessionEvent::Error(error) => (error.to_string(), None),
            SessionEvent::Closed {
                origin,
                code,
                reason,
            } => (
                DisconnectLine::from_close(*origin, *code, reason).display(),
                None,
            ),
        };
        Self {
            event,
            time: chrono::Local::now().format("%H:%M:%S").to_string(),
            display,
            original,
        }
    }
    fn bytes(&self) -> usize {
        self.display.len()
            + self.original.as_ref().map_or(0, String::len)
            + match &self.event {
                SessionEvent::Sent(SessionData::Text(s))
                | SessionEvent::Received(SessionData::Text(s)) => s.len(),
                SessionEvent::Sent(SessionData::Binary(b))
                | SessionEvent::Received(SessionData::Binary(b)) => b.len(),
                SessionEvent::Opened { url } => url.len(),
                SessionEvent::Closed { reason, .. } => reason.len(),
                _ => 0,
            }
    }
}

pub(crate) struct SessionState {
    pub(crate) generation: u64,
    pub(crate) state: ConnectionState,
    pub(crate) sender: Option<SessionSender>,
    cancellation: Option<oneshot::Sender<()>>,
    pub(crate) timeline: VecDeque<TimelineEntry>,
    bytes: usize,
    pub(crate) evicted: bool,
    pub(crate) sent: usize,
    pub(crate) received: usize,
    pub(crate) started: Option<Instant>,
    pub(crate) close: Option<(CloseOrigin, Option<u16>, String)>,
    pub(crate) error: Option<SessionError>,
    pub(crate) draft: String,
    pub(crate) sending: bool,
    pub(crate) list: ListState,
}
impl SessionState {
    pub(crate) fn clear_timeline(&mut self) {
        self.timeline.clear();
        self.bytes = 0;
        self.evicted = false;
        self.list.reset(0);
    }
    pub(crate) fn disconnect(&mut self) {
        match self.state {
            ConnectionState::Connecting => {
                self.cancellation.take();
                self.state = ConnectionState::Closed;
            }
            ConnectionState::Connected => {
                if let Some(sender) = &self.sender {
                    sender.close();
                }
                self.state = ConnectionState::Closing;
            }
            _ => {}
        }
    }
    fn apply(&mut self, update: Update) {
        match update {
            Update::Ready(sender) => {
                self.sender = Some(sender);
            }
            Update::Event(entry) => {
                match &entry.event {
                    SessionEvent::Opened { .. } => {
                        self.state = ConnectionState::Connected;
                        self.started = Some(Instant::now());
                    }
                    SessionEvent::Sent(_) => self.sent += 1,
                    SessionEvent::Received(_) => self.received += 1,
                    SessionEvent::Error(error) => {
                        if self.error.is_some() {
                            return;
                        }
                        self.error = Some(*error);
                        self.state = ConnectionState::Failed;
                    }
                    SessionEvent::Closed {
                        origin,
                        code,
                        reason,
                    } => {
                        self.close = Some((*origin, *code, reason.clone()));
                        if self.error.is_none() {
                            self.state = ConnectionState::Closed;
                        }
                        self.sender = None;
                        self.cancellation.take();
                    }
                }
                let follow = self.list.is_scrolled_to_end().unwrap_or(true);
                let old_len = self.timeline.len();
                let size = entry.bytes();
                self.bytes += size;
                self.timeline.push_back(entry);
                let mut removed = 0;
                while self.timeline.len() > MAX_ENTRIES || self.bytes > MAX_BYTES {
                    if let Some(old) = self.timeline.pop_front() {
                        self.bytes -= old.bytes();
                        removed += 1;
                        self.evicted = true;
                    } else {
                        break;
                    }
                }
                if removed > 0 {
                    self.list.splice(0..removed.min(old_len), 0);
                }
                if removed <= old_len {
                    let len = self.timeline.len();
                    self.list
                        .splice(len.saturating_sub(1)..len.saturating_sub(1), 1);
                }
                if follow {
                    self.list.scroll_to_end();
                }
            }
        }
    }
}
impl Drop for SessionState {
    fn drop(&mut self) {
        if let Some(sender) = &self.sender {
            sender.close();
        }
        self.cancellation.take();
    }
}
#[derive(Default)]
pub(crate) struct WebSocketState {
    next: u64,
    pub(crate) sessions: BTreeMap<RequestKey, SessionState>,
}
impl WebSocketState {
    pub(crate) fn begin(&mut self, key: RequestKey, cancellation: oneshot::Sender<()>) -> u64 {
        self.next = self.next.wrapping_add(1);
        self.sessions.insert(
            key,
            SessionState {
                generation: self.next,
                state: ConnectionState::Connecting,
                sender: None,
                cancellation: Some(cancellation),
                timeline: VecDeque::new(),
                bytes: 0,
                evicted: false,
                sent: 0,
                received: 0,
                started: None,
                close: None,
                error: None,
                draft: String::new(),
                sending: false,
                list: ListState::new(0, ListAlignment::Bottom, px(300.0)),
            },
        );
        self.next
    }
    pub(crate) fn apply(&mut self, generation: u64, update: Update) {
        if let Some(state) = self
            .sessions
            .values_mut()
            .find(|s| s.generation == generation)
        {
            // Cancelled attempts must not accept already-queued Ready/Opened.
            if state.state == ConnectionState::Closed && state.close.is_none() {
                return;
            }
            state.apply(update);
        }
    }
    pub(crate) fn remap_requests(&mut self, remaps: &BTreeMap<RequestKey, RequestKey>) {
        self.sessions = std::mem::take(&mut self.sessions)
            .into_iter()
            .filter_map(|(key, state)| remaps.get(&key).map(|new| (*new, state)))
            .collect();
    }
    pub(crate) fn clear(&mut self) {
        self.sessions.clear();
    }
    pub(crate) fn remove(&mut self, key: RequestKey) {
        self.sessions.remove(&key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn keys() -> (RequestKey, RequestKey) {
        let workspace = probe_core::Workspace::from_collection(probe_core::Collection {
            items: vec![
                probe_core::CollectionItem::Request(probe_core::Request::default()),
                probe_core::CollectionItem::Request(probe_core::Request::default()),
            ],
            ..Default::default()
        });
        let probe_core::WorkspaceItemRef::Request(a) = workspace.root_items()[0] else {
            panic!();
        };
        let probe_core::WorkspaceItemRef::Request(b) = workspace.root_items()[1] else {
            panic!();
        };
        (a, b)
    }
    fn event(event: SessionEvent) -> Update {
        Update::Event(TimelineEntry::new(event))
    }
    #[test]
    fn generations_remaps_and_removal_preserve_only_owned_sessions() {
        let (a, b) = keys();
        let mut state = WebSocketState::default();
        let (cancel, mut cancellation) = oneshot::channel();
        let first = state.begin(a, cancel);
        state.apply(
            first,
            event(SessionEvent::Opened {
                url: "ws://safe".into(),
            }),
        );
        state.remap_requests(&BTreeMap::from([(a, b)]));
        state.apply(
            first,
            event(SessionEvent::Received(SessionData::Text("one".into()))),
        );
        assert_eq!(state.sessions[&b].received, 1);
        assert!(cancellation.try_recv().is_err());
        let (cancel, _) = oneshot::channel();
        let second = state.begin(b, cancel);
        assert!(matches!(
            cancellation.try_recv(),
            Err(oneshot::error::TryRecvError::Closed)
        ));
        state.apply(first, event(SessionEvent::Error(SessionError::Connection)));
        assert!(state.sessions[&b].timeline.is_empty());
        state.apply(
            second,
            event(SessionEvent::Opened {
                url: "ws://new".into(),
            }),
        );
        state.remove(b);
        state.apply(
            second,
            event(SessionEvent::Received(SessionData::Text("late".into()))),
        );
        assert!(state.sessions.is_empty());
    }
    #[test]
    fn cancellation_and_errors_have_distinct_terminal_states() {
        let (a, _) = keys();
        let mut state = WebSocketState::default();
        let (cancel, mut cancellation) = oneshot::channel();
        let generation = state.begin(a, cancel);
        state.sessions.get_mut(&a).unwrap().disconnect();
        assert!(matches!(
            cancellation.try_recv(),
            Err(oneshot::error::TryRecvError::Closed)
        ));
        state.apply(
            generation,
            event(SessionEvent::Opened { url: "late".into() }),
        );
        assert_eq!(state.sessions[&a].state, ConnectionState::Closed);
        let (cancel, _) = oneshot::channel();
        let generation = state.begin(a, cancel);
        state.apply(
            generation,
            event(SessionEvent::Error(SessionError::Connection)),
        );
        state.apply(
            generation,
            event(SessionEvent::Error(SessionError::Connection)),
        );
        state.apply(
            generation,
            event(SessionEvent::Closed {
                origin: CloseOrigin::Error,
                code: None,
                reason: String::new(),
            }),
        );
        assert_eq!(state.sessions[&a].state, ConnectionState::Failed);
        assert_eq!(state.sessions[&a].timeline.len(), 2);
        assert_eq!(state.sessions[&a].error, Some(SessionError::Connection));
    }
    #[test]
    fn history_bounds_evict_in_order_and_clear_does_not_close() {
        let (a, _) = keys();
        let mut state = WebSocketState::default();
        let (cancel, _) = oneshot::channel();
        let generation = state.begin(a, cancel);
        state.apply(
            generation,
            event(SessionEvent::Opened {
                url: "ws://safe".into(),
            }),
        );
        for index in 0..MAX_ENTRIES + 3 {
            state.apply(
                generation,
                event(SessionEvent::Received(SessionData::Text(index.to_string()))),
            );
        }
        let session = state.sessions.get_mut(&a).unwrap();
        assert_eq!(session.timeline.len(), MAX_ENTRIES);
        assert_eq!(
            session.list.item_count(),
            session.timeline.len(),
            "evicted messages must also leave the virtualized list"
        );
        assert_eq!(
            session.timeline.front().unwrap().original.as_deref(),
            Some("3")
        );
        assert!(session.evicted);
        session.clear_timeline();
        assert!(session.timeline.is_empty());
        assert_eq!(session.list.item_count(), 0);
        assert!(!session.evicted);
        assert_eq!(session.state, ConnectionState::Connected);
        for _ in 0..10 {
            session.apply(event(SessionEvent::Received(SessionData::Text(
                "x".repeat(MAX_BYTES / 8),
            ))));
        }
        assert!(session.bytes <= MAX_BYTES);
        assert!(session.evicted);
        assert!(session.timeline.len() < 10);
    }
    #[test]
    fn oversized_payloads_cannot_bypass_retained_byte_limits() {
        let (key, _) = keys();
        let mut state = WebSocketState::default();
        let (cancel, _) = oneshot::channel();
        let generation = state.begin(key, cancel);
        for event_value in [
            SessionEvent::Received(SessionData::Text("x".repeat(MAX_BYTES / 2))),
            SessionEvent::Received(SessionData::Binary(vec![0; MAX_BYTES * 3 / 8])),
            SessionEvent::Opened {
                url: "x".repeat(MAX_BYTES * 5 / 8),
            },
            SessionEvent::Closed {
                origin: CloseOrigin::Remote,
                code: None,
                reason: "x".repeat(MAX_BYTES * 5 / 8),
            },
        ] {
            state.apply(generation, event(event_value));
            assert!(
                state.sessions[&key].timeline.is_empty(),
                "all retained representations count toward the limit"
            );
            assert!(state.sessions[&key].evicted);
            assert_eq!(
                state.sessions[&key].list.item_count(),
                0,
                "oversized payloads must not leave inaccessible list rows"
            );
        }
    }

    #[test]
    fn payload_presentation_keeps_original_and_binary_is_explicit() {
        let original = "{\"ok\":true}";
        let entry = TimelineEntry::new(SessionEvent::Received(SessionData::Text(original.into())));
        assert!(entry.display.contains('\n'));
        assert_eq!(entry.original.as_deref(), Some(original));
        assert_eq!(
            entry.event,
            SessionEvent::Received(SessionData::Text(original.into()))
        );
        let entry = TimelineEntry::new(SessionEvent::Received(SessionData::Binary(vec![0, 1, 2])));
        assert_eq!(entry.original.as_deref(), Some("AAEC"));
        assert!(entry.display.starts_with("Binary · 3 B"));
    }

    #[test]
    fn close_timeline_keeps_standard_names_and_peer_reasons() {
        fn display(origin: CloseOrigin, code: Option<u16>, reason: &str) -> String {
            TimelineEntry::new(SessionEvent::Closed {
                origin,
                code,
                reason: reason.to_owned(),
            })
            .display
        }
        assert_eq!(
            display(CloseOrigin::Remote, Some(1012), "restarting"),
            "Disconnected · remote · 1012 Service Restart · restarting"
        );
        assert_eq!(
            display(CloseOrigin::Local, Some(1000), ""),
            "Disconnected · local · 1000 Normal Closure"
        );
        assert_eq!(
            display(CloseOrigin::Local, Some(4000), "custom"),
            "Disconnected · local · 4000 · custom"
        );
        assert_eq!(
            display(CloseOrigin::Remote, Some(1004), ""),
            "Disconnected · remote · 1004"
        );
        assert_eq!(
            display(CloseOrigin::Error, None, "reset"),
            "Disconnected · error · reset"
        );
        assert_eq!(
            display(CloseOrigin::Remote, Some(1012), " \t "),
            "Disconnected · remote · 1012 Service Restart"
        );
        assert_eq!(
            display(CloseOrigin::Local, Some(4000), " "),
            "Disconnected · local · 4000"
        );
        assert_eq!(
            display(CloseOrigin::Local, None, ""),
            "Disconnected · local"
        );
    }

    #[test]
    fn disconnect_line_status_slice_is_only_the_close_code() {
        let (text, status) =
            DisconnectLine::from_close(CloseOrigin::Remote, Some(1012), "restarting").text();
        let status = status.expect("named close has a status segment");
        assert_eq!(&text[status.start..status.end], "1012 Service Restart");
        assert_eq!(&text[..status.start], "Disconnected · remote · ");
        assert_eq!(&text[status.end..], " · restarting");

        let (text, status) = DisconnectLine::from_close(CloseOrigin::Error, None, "reset").text();
        assert!(status.is_none());
        assert_eq!(text, "Disconnected · error · reset");
    }

    #[test]
    fn disconnect_severity_classifies_code_and_origin() {
        use DisconnectSeverity::{ClientError, Neutral, ServerError, Success, Temporary};
        for (origin, code, severity) in [
            (CloseOrigin::Remote, Some(1000), Success),
            (CloseOrigin::Remote, Some(1012), Temporary),
            (CloseOrigin::Remote, Some(3008), Temporary),
            (CloseOrigin::Remote, Some(1002), ClientError),
            (CloseOrigin::Remote, Some(3000), ClientError),
            (CloseOrigin::Remote, Some(1006), ServerError),
            (CloseOrigin::Remote, Some(1011), ServerError),
            (CloseOrigin::Remote, Some(1005), Neutral),
            (CloseOrigin::Remote, Some(4000), Neutral),
            (CloseOrigin::Remote, None, Neutral),
            (CloseOrigin::Local, Some(1000), Neutral),
            (CloseOrigin::Error, Some(1000), ServerError),
        ] {
            assert_eq!(
                disconnect_severity(origin, code),
                severity,
                "{origin:?} {code:?}"
            );
        }
    }
}
