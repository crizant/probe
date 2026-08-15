# Architecture

## Overview

The application uses a layered architecture with multiple interfaces
over a shared core.

                         Interfaces

                ┌────────────┼────────────┐
                │            │            │
               CLI          GPUI         MCP
                │         Desktop       Future
                │            │            │
                └────────────┼────────────┘
                             │
                    ┌────────▼────────┐
                    │   Application   │
                    │                │
                    │ Workspace      │
                    │ Request Exec   │
                    │ Environment    │
                    │ Persistence    │
                    └────────┬────────┘
                             │
                    ┌────────▼────────┐
                    │     Domain      │
                    │                │
                    │ Workspace      │
                    │ Request        │
                    │ Environment    │
                    │ Response       │
                    └────────▲────────┘
                             │
          ┌──────────────────┼──────────────────┐
          │                  │                  │
   OpenCollection          HTTP              History
    Repository             Engine              Store
          │                  │                  │
          ▼                  ▼                  ▼
        YAML              Network           Files/DB


## Fundamental Rule

CLI and GPUI are interfaces.

They do not own business logic.

If behavior differs between CLI and GPUI, determine whether the
difference belongs to presentation or represents an architectural bug.

For example:

CLI:
request run
    ↓
Application
    ↓
HTTP Engine

GPUI:
Send button
    ↓
Application
    ↓
same HTTP Engine


## Dependency Rule

Dependencies point inward.

Interfaces may depend on Application and Domain.

Application may depend on Domain abstractions.

Infrastructure implements capabilities required by the application.

Domain must not depend on:

- GPUI
- gpui-base
- CLI frameworks
- YAML
- reqwest
- filesystem implementation


## CLI

The CLI is a first-class interface optimized for:

- AI agents
- automation
- CI
- shell usage
- debugging
- headless environments

Its responsibilities are limited to:

- argument parsing
- invoking application operations
- human-readable presentation
- structured presentation
- exit-code mapping
- stdin/stdout integration

It must not implement domain behavior.

### Structured output

Programmatic commands should support JSON.

Streaming protocols should support JSONL where appropriate.

Structured stdout must contain only structured command output.

Logs and diagnostics belong on stderr.


## Desktop Presentation

GPUI owns:

- windows
- rendering
- entities
- events
- application lifecycle

gpui-base provides reusable unstyled/headless component behavior.

The application provides:

- visual identity
- themes
- colors
- typography
- spacing
- component composition

Preferred composition:

gpui-base primitive
        ↓
application styled component
        ↓
feature UI


## Application Layer

The application layer coordinates use cases.

Examples:

- load workspace
- list requests
- get request
- execute request
- resolve environment
- save request
- validate collection

These operations should be usable from both CLI and GPUI without
knowledge of either frontend.


## Workspace

Opening a workspace:

OpenCollection files
        ↓
OpenCollectionRepository
        ↓
Domain Workspace
        ↓
Application layer
        ↓
CLI or GPUI

For the desktop application, the resulting workspace remains in memory
for fast navigation.


## Desktop Request Selection

Fast path:

User click
    ↓
RequestKey (session-only)
    ↓
in-memory lookup
    ↓
selected request state
    ↓
GPUI notification/render

It must not include:

- filesystem reads
- YAML parsing
- database queries
- network operations

### Runtime Identity and Persistence Locators

OpenCollection does not define durable IDs for requests or folders. The active
workspace therefore assigns compact generational `RequestKey` and `FolderKey` values
while it is loaded. These keys index in-memory storage directly, are never serialized,
and are rebuilt the next time the workspace opens. When a deleted slot is reused, its
generation changes so stale selections and asynchronous results cannot resolve to the
replacement item.

Repository adapters separately own persistence locators. An unbundled collection can
use a workspace-relative file path; a bundled collection may use a structural item
path. CLI selectors are derived from repository locators rather than runtime keys.
Request names are presentation data and must not be treated as identity.


## CLI Request Execution

Typical path:

CLI arguments
     ↓
Application operation
     ↓
WorkspaceRepository
     ↓
Request + Environment
     ↓
EnvironmentResolver
     ↓
HTTP Engine
     ↓
Response
     ↓
CLI formatter
     ↓
human text / JSON

## Environment Resolution

Environment selection and interpolation live in `probe-core`, so CLI, desktop, and
future interfaces share exactly the same behavior. Resolution operates on the loaded
in-memory workspace: parent environments are applied before children, child variables
override by name, and variable values may reference other variables. Cyclic
inheritance, cyclic interpolation, missing variables, and invalid variant selection
produce typed errors.

The resolver returns a cloned, resolved request and leaves the canonical parsed model
unchanged. It currently interpolates method, URL, headers, query parameters, supported
body fields, file references, and authentication string/number values. OpenCollection
secret declarations contain no value, so references fail until a separate secure
runtime value provider is introduced. Loading `dotEnvFilePath` is also outside Phase 4;
the domain resolver remains independent of filesystem APIs.

## HTTP Execution

`probe-http` owns the single asynchronous HTTP implementation. It converts resolved
domain requests into network requests, applies enabled headers and query parameters,
selects body/file variants, implements Basic and Bearer authentication, and enforces
OpenCollection timeout and redirect settings. Neither CLI nor desktop constructs HTTP
requests independently.

The engine accepts a caller-provided cancellation future. Completion of that future—or
dropping the execution future—cancels the request without coupling the engine to
terminal signals or a GUI framework. The CLI adapts Ctrl-C to this boundary; desktop
can later adapt task or view cancellation to the same API.

Completed responses contain raw bytes plus status, reason, final URL, duration, size,
and deterministically sorted headers. The CLI only inlines UTF-8 bodies up to 1 MiB and
supports `--output` for large or binary data. Response retention and history policies
remain outside the frontend and can evolve without changing request construction.


## Persistence

Desktop editing:

User edit
    ↓
in-memory update
    ↓
immediate render
    ↓
persistence operation
    ↓
OpenCollectionRepository
    ↓
atomic filesystem write

CLI mutation:

CLI command
    ↓
application operation
    ↓
domain update
    ↓
same repository
    ↓
atomic filesystem write


## Repository Abstraction

The application should not know that the workspace is represented by
YAML.

Conceptually:

                  WorkspaceRepository
                         ▲
              ┌──────────┴──────────┐
              │                     │
 OpenCollectionRepository      future adapters
                                      │
                                   Yaak etc.

Only OpenCollection is required initially.


## HTTP Engine

HTTP execution is shared.

                         HTTP Engine
                         ▲         ▲
                         │         │
                       CLI       GPUI

The engine accepts domain/application request representations rather
than CLI arguments or GPUI state.

It returns structured response data.


## Streaming Protocols

Long-lived protocols require a session/event abstraction.

Conceptually:

WebSocket / SSE / gRPC
          ↓
    Protocol Session
          ↓
       Event Stream
       ┌──────────┐
       ▼          ▼
      CLI        GPUI
       │          │
 JSONL/stdin     visual
 terminal        session UI

Terminal interaction must not leak into the protocol implementation.


## Responses

Responses are separate from request definitions.

Request metadata
→ generally resident in desktop memory

Response metadata
→ cache/history

Response bodies
→ bounded memory and/or filesystem

Large response bodies must not dictate workspace memory consumption.


## Concurrency

Application/UI-facing state has clear ownership.

Slow operations execute outside the GPUI render/event path:

- HTTP
- filesystem
- YAML parsing
- JSON processing
- Git
- streaming network work

Completed operations return structured results/events.

Avoid shared mutable global state.


## Future MCP Interface

MCP should be another adapter over the application layer.

It should not wrap CLI commands unless there is a compelling reason.

Prefer:

MCP
 ↓
Application/Core

rather than:

MCP
 ↓
shell
 ↓
CLI
 ↓
Application/Core

This keeps tool semantics structured and avoids unnecessary process and
text-parsing boundaries.
