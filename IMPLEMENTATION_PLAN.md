# Implementation Plan

## Phase 0 — Rust Workspace Foundation

Create the Rust workspace.

Suggested structure:

crates/
├── core/
├── opencollection/
├── http/
├── cli/
└── desktop/

Required:

- Rust workspace
- formatting
- Clippy
- test infrastructure
- fixture directory
- CI
- logging/error strategy

GPUI/gpui-base may be added now or when desktop development begins,
but desktop implementation must not begin yet.

Exit criteria:

- cargo fmt --check passes
- cargo clippy passes
- cargo test passes
- CLI binary starts


## Phase 1 — OpenCollection Reader

Implement OpenCollection YAML parsing independently of every frontend.

Support:

- Collection
- Folder
- HTTP request
- Headers
- Query parameters
- Request body
- Authentication
- Environments

Use specification-based fixtures.

Preserve unknown fields whenever practical.

Exit criteria:

- OpenCollection fixtures load successfully
- models contain expected values
- round-trip serialization works
- unsupported fields are not silently destroyed


## Phase 2 — Workspace Domain Model

Implement the in-memory workspace.

Conceptually:

Workspace
├── requests_by_id
├── folders_by_id
├── environments
└── metadata

Requirements:

- O(1) request lookup where appropriate
- domain has no dependency on CLI or GPUI
- domain has no dependency on YAML representation
- simple Rust ownership

Avoid unnecessary:

- Arc
- Mutex
- RwLock
- global mutable state

Exit criteria:

- 1,000+ request fixture loads
- workspace operations have unit tests
- domain can be used without UI or CLI-specific code


## Phase 3 — Agent-Friendly CLI Foundation

Build the first real frontend over the core.

Initial commands:

<app> collection validate <path>
<app> request list <path>
<app> request get <path> <request-id>
<app> request run <path> <request-id>

Support:

--json
--help

Human output should be readable.

JSON output should be stable and machine-readable.

Requirements:

- deterministic output
- meaningful exit codes
- no unnecessary prompts
- errors available as structured JSON when --json is used

Exit criteria:

- agent can discover requests
- agent can inspect a request
- agent can validate a workspace


## Phase 4 — Environment Resolution

Implement:

- environments
- variable lookup
- variable interpolation
- environment selection
- missing-variable errors

Expose through the CLI.

Example:

<app> request run ./api req_users \
  --environment development

Tests must cover resolution behavior independently of CLI parsing.


## Phase 5 — HTTP Execution

Implement the HTTP engine.

Support:

- GET
- POST
- PUT
- PATCH
- DELETE
- headers
- query parameters
- JSON body
- text body
- form body
- multipart
- Basic auth
- Bearer auth
- redirects
- timeout
- cancellation

Response metadata:

- status
- duration
- size
- headers

Expose through:

<app> request run

Human example:

GET https://api.example.com/users

200 OK
128 ms
4.2 KB

{ ... }

Machine output:

<app> request run ./api req_users --json

Large bodies must not create unreasonable stdout behavior.

Support explicit body output to a file when appropriate.

Exit criteria:

- real OpenCollection HTTP requests execute correctly
- HTTP engine is completely independent of CLI
- CLI only adapts arguments/output to the shared engine


## Phase 6 — CLI Automation Hardening

Make the CLI suitable for AI agents and CI.

Add:

- stable JSON schemas
- documented exit codes
- stdin support where useful
- explicit output-file support
- quiet mode where useful
- deterministic error representation

Avoid output that depends on terminal animation or cursor control when
structured mode is enabled.

Add integration tests exercising the binary.

Exit criteria:

An external program can reliably:

1. discover requests
2. inspect requests
3. execute requests
4. interpret success/failure

without parsing human-readable text.


## Phase 7 — OpenCollection Persistence

Implement safe writes.

Required:

- serialization
- atomic writes
- unknown-field preservation
- safe error handling

Add CLI operations only where useful.

Do not turn the CLI into a full interactive request editor.

Exit criteria:

load
→ modify
→ save
→ reload

preserves expected OpenCollection semantics.


## Phase 8 — Performance Baseline

Before building the UI, establish core benchmarks.

Workspace sizes:

- 100 requests
- 1,000 requests
- 10,000 requests

Measure:

- parsing
- workspace construction
- request lookup
- CLI startup
- memory consumption where practical

Create representative benchmark fixtures.

Do not optimize without measurements.


## Phase 9 — GPUI + gpui-base Foundation

Begin desktop implementation only after the core and CLI architecture
is established.

Add/pin:

- GPUI
- gpui-base

Before using unfamiliar APIs:

- inspect exact pinned source
- inspect examples/tests
- do not guess APIs

Build:

- application window
- theme foundation
- basic gpui-base primitives

Exit criteria:

- desktop application launches
- gpui-base works
- shared core is accessible from desktop crate


## Phase 10 — Desktop Shell

Implement:

- workspace sidebar
- folder/request tree
- tabs
- request editor area
- response panel
- resizable panes

No duplicate OpenCollection parsing logic is allowed.

The desktop application consumes the same core used by the CLI.

Exit criteria:

- open collection
- browse requests
- switch requests instantly
- open/close tabs


## Phase 11 — Desktop Request Editor

Implement:

- method
- URL
- query parameters
- headers
- body
- authentication
- environment selection

Changes update the in-memory model immediately.

Persistence happens separately.


## Phase 12 — Desktop HTTP Execution

Connect the existing shared HTTP engine to GPUI.

Do not implement a second HTTP stack.

Required:

- Send
- Cancel
- status
- duration
- size
- headers
- response body


## Phase 13 — Response Viewer

Implement:

- Pretty
- Raw
- Headers
- Search

Benchmark gpui-base/GPUI text components before assuming suitability
for large responses.

For large responses consider:

- virtualization
- chunked rendering
- lazy loading
- background parsing
- specialized read-only rendering

Exit criteria:

- normal responses render immediately
- large responses do not freeze navigation
- request switching remains responsive


## Phase 14 — Filesystem Synchronization

Implement:

- file watching
- external modifications
- file creation/deletion
- rename handling
- conflict detection

Never silently overwrite externally modified data.


## Phase 15 — Streaming Protocol Architecture

Design a shared event/session abstraction suitable for:

- WebSocket
- SSE
- gRPC streaming

Do not design this specifically around terminal interaction.

Conceptually:

Protocol Engine
      ↓
Session/Event Stream
      ↓
 ┌────┴────┐
 CLI      GPUI


## Phase 16 — WebSocket

Implement shared WebSocket support.

CLI should support agent-friendly non-interactive operation, including
structured streaming output such as JSONL.

Interactive terminal mode may be added but is not the architectural
foundation.


## Phase 17 — gRPC

Implement:

- unary
- server streaming
- client streaming
- bidirectional streaming

The shared protocol implementation must work for both CLI and GPUI.


## Phase 18 — Git Integration

Filesystem remains the primary Git integration boundary.

Optional built-in functionality:

- status
- changed files
- current branch
- diff
- commit
- pull
- push

Keep basic Git behavior provider-independent.


## Phase 19 — MCP

Consider exposing the shared application layer through an MCP server.

Do not reimplement business logic.

Conceptually:

                 Application/Core
                ┌──────┼──────┐
                │      │      │
               CLI    GPUI    MCP