//! Native GPUI presentation adapter for Probe.
//!
//! Presentation and interaction live here. Workspace and request behavior remains in
//! [`probe_core`] so the desktop and CLI continue to share the same application model.

#![forbid(unsafe_code)]

mod app;
pub mod components;
mod execution;
mod request_editor;
mod session;
mod shell;
pub mod theme;

pub use app::{ProbeApp, run};
