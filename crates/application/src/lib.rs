//! Interface-neutral use cases shared by the Probe CLI and desktop.
//!
//! This crate coordinates core and transport operations. It must not depend on GPUI,
//! CLI parsing, native credential storage, YAML, or presentation formatting.

#![forbid(unsafe_code)]

mod assertions;
mod client_certificate;
pub use client_certificate::{inspect_certificate, inspect_certificate_observing};
mod execution;
mod websocket_input;

pub use assertions::{
    AssertionError, AssertionOutcome, AssertionResult, MAX_ASSERTION_ACTUAL_CHARS,
    evaluate_assertions,
};

pub use websocket_input::WebSocketInputFormat;

pub use execution::{
    ExecutedResponse, HttpExecution, HttpExecutionError, NoSecrets, PreparedRequest,
    RequestResolution, SECRET_DIAGNOSTIC_WITHHELD, copy_as_curl, prepare_request,
};

pub use execution::{
    CloseOrigin, Session, SessionData, SessionError, SessionEvent, SessionSender,
    WebSocketExecution,
};
pub use probe_websocket::close_code_name;
