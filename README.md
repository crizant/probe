# Probe

A fast, native, local-first API client for macOS, Windows, and Linux.

Built with Rust, GPUI, and gpui-base, with OpenCollection YAML as the
primary workspace format.

The project provides two first-class interfaces:

- CLI — for developers, automation, CI, and AI agents
- Desktop — a native GPUI application for interactive API development

Both interfaces use the same Rust application and domain layers.

## Goals

- Fast native desktop experience
- Powerful agent-friendly CLI
- OpenCollection YAML as the primary workspace format
- Filesystem-first and Git-friendly
- No account or cloud service required
- Instant navigation in very large collections
- Compatible with existing OpenCollection collections
- Low memory usage
- Fast startup
- Native GPU-rendered desktop UI without Electron or WebView
- Automation-friendly and suitable for AI coding agents

## Technology

- Rust
- GPUI
- gpui-base
- OpenCollection YAML
- Git-compatible filesystem storage

GPUI provides the native application and rendering framework.

gpui-base provides reusable unstyled/headless UI primitives.

Application-specific visual design and styling belong to this project,
not to the component library.

## Interfaces

### CLI

The CLI is a first-class product interface.

It is intended for:

- AI coding agents
- shell scripts
- CI/CD
- automated testing
- developers
- debugging
- headless environments

The CLI should support both human-readable and structured output.

Example:

```bash
<app> collection validate ./api

<app> request list ./api

<app> request get ./api req_users --json

<app> request run ./api req_users --json
```

## Development

The repository is a Cargo workspace. Phase 0 provides the shared crate boundaries
and a minimal CLI entry point; OpenCollection and HTTP behavior are intentionally
deferred to later phases.

```bash
cargo run -p probe-cli -- --help
cargo fmt --check
cargo clippy --all-targets --all-features
cargo test
```
