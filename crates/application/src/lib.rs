//! Interface-neutral use cases shared by the Probe CLI and desktop.
//!
//! This crate coordinates core and HTTP operations. It must not depend on GPUI,
//! CLI parsing, native credential storage, YAML, or presentation formatting.

#![forbid(unsafe_code)]

mod execution;

pub use execution::{
    ExecutedResponse, HttpExecution, NoSecrets, PreparedRequest, RequestResolution,
    SECRET_DIAGNOSTIC_WITHHELD, copy_as_curl, prepare_request,
};
