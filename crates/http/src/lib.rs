//! Shared asynchronous HTTP construction and execution for Probe.

#![forbid(unsafe_code)]

mod cache;
mod engine;
mod error;
mod identity;
mod pkcs12;
mod request;
mod response;

pub use cache::{ResponseBodyFile, ResponseCache};
pub use engine::{HttpEngine, HttpProgress, StreamedHttpResponse};
pub use error::{HttpError, Pkcs12Error};
pub use identity::{ClientIdentity, PemClientIdentity};
pub use response::{HttpResponse, ResponseHeader};

use std::path::PathBuf;

/// Maximum response body retained by the default in-memory execution methods.
pub const MAX_IN_MEMORY_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

/// Filesystem context used while constructing request bodies.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ExecutionOptions {
    /// Base directory for relative request-body file paths.
    pub base_directory: Option<PathBuf>,
    /// Selected PEM identity; each authenticated execution gets its own client.
    pub client_identity: Option<ClientIdentity>,
    /// Shared cache used for automatically managed large-response spool files.
    ///
    /// When absent, the complete large body is drained without being retained.
    pub response_cache: Option<ResponseCache>,
}
