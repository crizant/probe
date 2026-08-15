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

The CLI is a first-class product interface intended for:

- AI coding agents
- shell scripts
- CI/CD
- automated testing
- developers
- debugging
- headless environments

It supports human-readable and deterministic JSON output.

Examples:

```bash
probe collection validate ./api

cat collection.yml | probe collection validate - --json

probe request list ./api --json

probe request get ./api users/list-users.yml --json

probe request get ./api users/list-users.yml \
  --environment development --json
```

Unbundled collections use workspace-relative request paths as selectors. Bundled
collections use structural selectors:

```bash
probe request get ./collection.yml items/0/items/0 --json
```

```bash
probe request run ./api users/list-users.yml \
  --environment development --json

probe request run ./api reports/download.yml \
  --environment development --output ./report.pdf --json

probe request set ./api users/list-users.yml \
  --method GET --url https://api.example.com/v2/users --json

probe collection validate ./api --quiet
```

`--environment <name>` selects an OpenCollection environment, applies its `extends`
chain, and interpolates request variables before output or execution preflight.

## Development

The repository is a Cargo workspace. It currently includes OpenCollection parsing,
bundled and unbundled filesystem loading, an indexed in-memory workspace, shared
environment resolution, asynchronous HTTP execution, a versioned automation-safe CLI
contract, and a native GPUI desktop shell with semantic light/dark themes,
OpenCollection-backed browsing, request tabs, resizable editor/response panes, and
a title-bar workspace switcher. The request editor updates method, URL, query
parameters, headers, body, authentication, and environment selection in memory as
the user works. Desktop Send and Cancel actions use the same asynchronous HTTP engine
as the CLI, and completed responses expose status, duration, size, headers, and body.
The editor and response viewer support vertical or horizontal layouts,
and the local desktop session restores the active workspace, tabs, collapsed folders,
pane orientation, and pane sizes automatically.

```bash
cargo run -p probe-cli -- --help
cargo run -p probe-desktop
cargo fmt --check
cargo clippy --all-targets --all-features
cargo test
```

Core performance baselines cover parsing, workspace construction, request lookup,
CLI startup, and practical peak-memory profiling at 100, 1,000, and 10,000 requests.
See [docs/PERFORMANCE.md](docs/PERFORMANCE.md) for reproducible commands and fixture
generation.

Desktop work follows the native, platform-aware design and future plain-text theming
contract in [docs/DESIGN.md](docs/DESIGN.md).

Try the current CLI against the included unbundled fixture:

```bash
cargo run -p probe-cli -- collection validate \
  tests/fixtures/opencollection/unbundled --json

cargo run -p probe-cli -- request list \
  tests/fixtures/opencollection/unbundled --json

cargo run -p probe-cli -- request get \
  tests/fixtures/opencollection/unbundled users/list-users.yml --json

cargo run -p probe-cli -- request get \
  tests/fixtures/opencollection/phase4-environments.yml items/0 \
  --environment development --json
```

See [docs/CLI.md](docs/CLI.md) for selectors, JSON fields, and exit codes.
