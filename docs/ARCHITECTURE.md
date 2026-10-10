# Architecture

## Overview

The application uses two interfaces over shared application and domain layers.

                         Interfaces

                       ┌─────┴─────┐
                       │           │
                      CLI         GPUI
                                Desktop
                       │           │
                       └─────┬─────┘
                             │
                    ┌────────▼────────┐
                    │   Application   │
                    │                │
                    │ Request Exec   │
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
                       ┌─────┴─────┐
                       │           │
                OpenCollection   HTTP
                 Repository      Engine
                       │           │
                       ▼           ▼
                     YAML       Network

Portable import formats are inbound adapters. The Postman adapter reads official
Collection v2.0/v2.1 JSON, while the Yaak adapter reads official export JSON or
directory-sync models. Both produce the same domain `Collection` used by every
interface. OpenCollection remains the only canonical persistence representation:

    Postman JSON        Yaak export / sync directory
          ↓                         ↓
    Postman adapter             Yaak adapter
          └────────────┬────────────┘
                       ↓
              Domain Collection
                       ↓
    OpenCollection repository
                       ↓
       Bundled YAML file

The adapters do not own CLI prompts, GPUI state, or filesystem persistence. Both
frontends invoke them and pass the converted domain value to the shared atomic writer.
Shared import diagnostics live in the core so strict and partial behavior remains
identical across providers and interfaces.
OpenCollection projection diagnostics live in the repository adapter. They identify
unsupported values retained in source YAML and travel with loaded workspaces so CLI
validation and desktop loading can show them without rejecting the collection.


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

The crate graph is acyclic:

    probe-cli ─────┐        ┌──► probe-opencollection ──┐
                   ├────────┤    probe-postman ─────────┤
    probe-desktop ─┘        │    probe-yaak ────────────┤
                            │                           ▼
                            └──► probe-application ──► probe-core
                                        │                ▲
                                        └──► probe-http ─┘

Both interfaces depend on `probe-application`, `probe-core`, the repository and
import adapters, and `probe-http` for engine construction and response types. `probe-application` depends only on `probe-core` and `probe-http`.
`probe-http` depends only on `probe-core`. Only `probe-desktop` depends on GPUI,
gpui-base, and `keyring`.

Domain must not depend on:

- GPUI
- gpui-base (Longbridge)
- CLI frameworks
- YAML
- reqwest
- filesystem implementation


## CLI

The CLI is a first-class automation and headless interface. Its responsibilities
are limited to:

- argument parsing
- invoking application operations
- human-readable presentation
- structured presentation
- exit-code mapping
- stdin/stdout integration

It must not implement domain behavior.

### Structured output

The CLI's JSON documents carry an explicit schema version. Automation should branch
on stable error categories and exit codes, never parse human diagnostic messages.
Bundled collections may be supplied through stdin without moving YAML parsing into
the frontend: the OpenCollection repository projects the in-memory document and
builds the same structural selectors used for bundled files. Quiet mode is a
presentation concern and suppresses only successful command output. The complete
public contract is in [CLI.md](CLI.md).


## Desktop Presentation

GPUI owns:

- windows
- rendering
- entities
- events
- application lifecycle

gpui-base (Longbridge `gpui-base` from `longbridge/gpui-component`) provides
reusable component behavior and default chrome tokens. Do not use
`gpui-component`.

The application provides:

- visual identity
- themes
- colors
- typography
- spacing
- component composition

Preferred composition:

Longbridge gpui-base primitive
        ↓
application styled component
        ↓
feature UI

Desktop components consume semantic design tokens. They must not hard-code theme
colors or parse theme files. Platform presentation may map the same semantic intent
to different macOS, Windows, and Linux conventions.

Built-in themes map Porcelain Honey (light) and Graphite Honey (dark) onto the
semantic token model. [DESIGN.md](DESIGN.md) is the canonical source for platform
behavior, visual testing, themes, and accessibility.


## Application Layer

The application layer coordinates use cases. These operations should be usable from
both CLI and GPUI without knowledge of either frontend.

`probe-application` currently owns request execution:

    prepare_request(request, RequestResolution, &dyn SecretProvider)
        ↓
    PreparedRequest ── presentation() ──► dry runs, summaries
        ↓ into_http()
    HttpExecution
        ↓ execute(engine, options, output, cancellation, progress)
    ExecutedResponse (redacted when a secret was used)

`RequestResolution` carries the environments, selected environment, invocation
overrides, strict flag, and provider workspace identity. Variables are resolved only
when an environment, overrides, or strict resolution is requested. The provider is
consulted only for secrets the request can reach. `PreparedRequest` and
`HttpExecution` keep the secret-bearing execution request and the disclosure context
private. Their `Debug` output shows only the presentation request and whether a secret
was used. `NoSecrets` is the shared provider for invocations without a secret backend.
The process-environment provider stays in the CLI. The native credential provider and
its presence observer stay in the desktop.

Other use cases are not yet collected here. Loading, listing, validation, saving,
structure edits, and import conversion are shared through `probe-core`,
`probe-opencollection`, `probe-postman`, and `probe-yaak` directly.


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

Collection items are `Folder` or `Request`. Every OpenCollection request item, HTTP,
GraphQL, or WebSocket, loads into one native `Request` whose `RequestKind` selects the
protocol and owns that protocol's payload: `Http { body }`, `Graphql { body }`, or
`WebSocket { message }`. A request therefore cannot carry a payload for another
protocol. OpenCollection projection maps `info.type` and the matching
`http`/`graphql`/`websocket` section to and from `RequestKind`, and imports build the
same model directly.

A WebSocket request reuses the common URL, headers, authentication, docs, metadata, and
settings fields. OpenCollection WebSocket details define no method, parameters, or HTTP
body, so those fields stay empty and updates that set them are rejected. The message is
a single `{type, data}` value or titled variants with one selected entry; `type` is
`text`, `json`, `xml`, or `binary`. Settings add the WebSocket `keepAliveInterval`;
`timeout` is the connection timeout. WebSocket requests load, edit, and save natively,
but they are not executable yet: preparing one for the HTTP engine is rejected.


## Desktop Runtime

Opening a collection delegates filesystem traversal and parsing to the
OpenCollection repository on a background executor. The desktop retains the resulting
workspace in memory for the life of the window. Request selection is therefore:

    user selection
        ↓
    session-only RequestKey
        ↓
    O(1) in-memory lookup
        ↓
    render notification

The selection path performs no filesystem, YAML, database, or network work. Folder
expansion, tabs, pane state, and environment selection are presentation state.

Request controls mutate the in-memory domain request. Saving is a separate shared
repository operation and never occurs implicitly on each keystroke. Environment
resolution and mutation use core and repository operations rather than desktop-only
logic. Secret declarations live in OpenCollection; the desktop sets, replaces, and
deletes their values in the native credential store, as described in
[Secrets](#secrets).

Desktop Send uses the same `probe-application` execution operation as the CLI, away
from the UI thread. Resolution runs on a Tokio blocking worker because the native
provider may block. Cancellation reaches the shared engine;
generation checks prevent stale completions from replacing newer results. Response
and execution state remain presentation-only.
The desktop retains one Tokio execution runtime and HTTP engine per window, so
concurrent sends can reuse HTTP connections while progress and completion return
to GPUI through channels.

The response viewer uses virtualized, read-only editing and performs expensive
formatting or highlighting on a background executor. The request tree similarly
virtualizes a flat index of visible in-memory item references.

All create, rename, delete, move, and reorder interactions become repository-owned
StructureOperation values. A successful operation returns a refreshed workspace and
selector remaps. The desktop rebuilds runtime keys and remaps tabs, selection,
collapsed folders, drafts, and session state; dirty request fields are not implicitly
saved. Destructive operations require confirmation when data or drafts would be lost.

### Desktop Session Restoration

Probe stores a small, versioned session document in the platform application-data
directory. It contains presentation metadata such as recent collection paths,
repository selectors, selected environments, pane state, and orientation. It is never
stored in OpenCollection YAML. Request, collection, and folder tabs share one ordered
strip. Sessions retain their mixed order and active tab using typed repository locators;
legacy request-only session fields are used only when unified tab state is absent.
An explicitly empty unified tab list remains empty. Detached requests have no saved
locator and are not restored.

Session I/O is atomic and runs off the UI thread. Restoration reloads the collection,
rebuilds runtime keys, and resolves persisted repository selectors. Missing collections,
items, or environments produce recoverable state rather than preventing startup.
Closing a collection clears active session state without deleting collection files.

### User Configuration

The desktop reads an optional TOML file for user settings. It is not an
OpenCollection document, and it does not store collection variables, secrets, or
desktop session state. The CLI does not load it. Omitted keys use defaults, and
unknown keys are ignored. The desktop does not write the file.

```toml
theme = "system"
```

`theme` selects the desktop appearance:

- `system` is the default. Probe uses Porcelain Honey or Graphite Honey to match
  the OS appearance, and keeps following later OS appearance changes.
- `light` uses Porcelain Honey.
- `dark` uses Graphite Honey.

`light` and `dark` stay on that built-in theme when the OS appearance changes.
On macOS they also set the native window appearance, so the titlebar matches
the theme. `system` clears that override and macOS chrome follows the OS.
Any other value fails config parsing.

- macOS and Linux: `$XDG_CONFIG_HOME/probe/config.toml` when `XDG_CONFIG_HOME`
  is an absolute path. An empty or relative value is ignored, and the file is
  `$HOME/.config/probe/config.toml` when `HOME` is an absolute path. Otherwise
  startup reports that the config path could not be resolved.
- Windows: `%APPDATA%\probe\config.toml` when `APPDATA` is an absolute path.
  Otherwise startup reports that the config path could not be resolved.

A missing file loads the built-in defaults and is not created. A symlink to a
regular file is followed. A dangling symlink is a read error. A directory or
other non-file, and a regular file larger than 64 KiB, are configuration
errors. Invalid TOML and these filesystem failures include the config path.
Startup reads this small file once, before the GPUI event loop and before the
main window is created, so the effective theme is applied as the window is
created. The same result is reused when the process opens another window later.
The view does not parse TOML or touch the filesystem. A failure is shown as a
persistent error toast once the window exists, and the window keeps the defaults.

### Runtime Identity and Persistence Locators

OpenCollection does not define durable request or folder IDs. Each loaded workspace
therefore assigns session-only RequestKey and FolderKey values for fast, stale-safe
in-memory lookup. RequestKey includes a workspace generation as well as the arena
slot generation, so a key from an earlier load cannot resolve in a new workspace.
These keys are never serialized and are rebuilt on reload.

Repository adapters separately own persistence locators: workspace-relative paths for
unbundled collections and structural item paths for bundled collections. CLI selectors
and desktop-session references use these locators. Names are presentation data, not
identity.

## CLI Request Execution

Typical path:

CLI arguments
     ↓
WorkspaceRepository
     ↓
native Request + Environment
     ↓
probe_application::prepare_request   (--dry-run stops here)
     ↓
PreparedRequest::into_http()
     ↓
HttpExecution::execute → probe-http engine → network (reqwest)
     ↓
redacted ExecutedResponse
     ↓
CLI expectations and formatter
     ↓
human text / JSON

## Environment Resolution

Environment selection and interpolation live in `probe-core`, so CLI, desktop, and
future interfaces share exactly the same behavior. Resolution operates on the loaded
in-memory workspace: parent environments are applied before children, child variables
override by name, and variable values may reference other variables. Cyclic
inheritance, cyclic interpolation, missing variables, and invalid variant selection
produce typed errors. The same crate also exposes the effective variable declarations for a
selected environment, together with the environment that currently defines each name,
so desktop presentation does not reimplement inheritance, overrides, or secret
shadowing.

The resolver returns a cloned, resolved request and leaves the canonical parsed model
unchanged. It currently interpolates method, URL, headers, query and path parameters, supported
body fields, file references, and authentication string/number values. OpenCollection
secret declarations contain no value, so references require a runtime secret
provider. The resolver does not load `dotEnvFilePath`;
the domain remains independent of filesystem APIs.

## HTTP Execution

Execution preparation lives in `probe-core`. `Request::into_http()` consumes a resolved
native request and returns a `PreparedHttpRequest`, which can only be constructed there.
Runtime secret values are separate from public resolved environment variables. The
core `SecretProvider` receives a logical variable name, selected environment, and
optional workspace identity; `SecretValue` redacts formatting, and raw access stays
inside `probe-core`. The resolved environment exposes safe redaction operations to
adapters. Providers are consulted only for effective enabled
secret declarations after inheritance and invocation overrides.
`probe_application::prepare_request` produces an execution request and a presentation
request that keeps secret references. Missing secret values and provider failures are
retained until a request references the affected secret, including through a dependent
plain variable; then resolution fails closed. The core does not manage credential
storage; each interface supplies its provider, as described in [Secrets](#secrets).

Both interfaces prepare requests through `resolve_environment_for_request_with_provider`.
It finds the request's variable references and follows transitive plain-variable
dependencies before consulting a provider. It reads each reachable effective secret
once per execution. Unused declarations cause no provider calls and do not make the
request secret-bearing.

When a secret was used, `HttpExecution::execute` applies the same rules for every
interface:

- it bypasses the response cache, so no spool file holds unredacted bytes;
- it redacts exact secret byte sequences in response headers and the body preview,
  including binary previews;
- it reports the presentation URL instead of the final URL;
- it withholds failure diagnostics but keeps the failure kind. Cancellation and
  timeout are kept, output failures keep their path, configuration failures stay
  configuration failures, and all others become transport failures.

Response-to-file policy is left to each interface. The CLI's `--output` deliberately
writes the original bytes to a user-owned file. Direct desktop response-to-file
execution is rejected for secret-bearing requests, because that streaming path would
write response bytes before redaction. The normal viewer can save a complete redacted
in-memory response; oversized previews cannot be saved as complete bodies without
retained storage.

HTTP requests pass through unchanged. GraphQL requests become GraphQL-over-HTTP: `GET`
carries the selected operation in query parameters, and other methods carry a JSON
envelope body. The engine therefore never sees GraphQL.

`probe-http` owns the single asynchronous HTTP implementation. It accepts only
`PreparedHttpRequest` values and converts them into network requests, substitutes enabled `:variableName` path parameters,
applies enabled headers and query parameters,
selects body/file variants, implements Basic, Bearer, and API Key authentication, and enforces
OpenCollection timeout and redirect settings. Neither CLI nor desktop constructs HTTP
requests independently.

The engine accepts a caller-provided cancellation future. Completion of that future—or
dropping the execution future—cancels the request without coupling the engine to
terminal signals or a GUI framework. The CLI adapts Ctrl-C to this boundary; desktop
can later adapt task or view cancellation to the same API.

Completed responses contain status, reason, final URL, duration, size, deterministically
sorted headers, and at most 16 MiB of in-memory body data. Once that bound is crossed, the
engine keeps the leading 16 MiB as the first presentation page and, when requested by the caller,
streams the complete body to an automatically managed spool file. Cloned response handles share
ownership of that file. The final owner queues its deletion on a cache worker, which removes it
under the quota lock. Frontends that need the complete body provide a cache directory;
callers such as the CLI can drain the remainder without retaining it. The desktop reads subsequent
16 MiB pages off the UI thread, searches only the resident page, and renders those pages as
unwrapped Raw text without retaining a duplicate Pretty representation. Pretty is hidden for
file-backed responses because formatting an isolated page would not produce a valid document.
Inspect remains available and scans file-backed JSON and XML through streaming parsers without
constructing a complete document tree. `--output`
remains distinct: it streams chunks to a temporary file and replaces the requested user-owned
destination only after the complete response is written and synced.
Response retention and history policies remain outside the frontend and can evolve without
changing request construction.

The desktop response cache has a 512 MiB global quota. Cache sessions hold filesystem leases so
multiple Probe processes do not recover or delete one another's live responses. Initialization and
subsequent reservations remove session directories whose lease was released by a crash. Quota
accounting includes live response files from every active session. If a response cannot fit in the
remaining quota, Probe deletes its partial spool, continues draining the network response, and
returns the 16 MiB preview with a retention warning; existing retained responses are not evicted.


## Secrets

OpenCollection secret declarations (`secret: true`) contain no value, and Probe never
writes a secret value to collection YAML or the desktop session. Values come only from
a runtime `SecretProvider` supplied by the interface:

- The CLI offers the process-environment provider (`--secret-provider env`); its
  contract is in [CLI.md](CLI.md).
- The desktop uses `NativeSecretProvider` over the Probe-owned `credentials` service,
  which exposes `CredentialId::for_workspace` and `CredentialStore::{set,delete,get}`
  over `keyring` 4. Setting an existing key replaces it; deleting a missing key returns
  `NotFound`. There is no plaintext file or process-environment fallback when the
  native store is unavailable. The adapter maps backend diagnostics to safe Probe
  errors and then to core's diagnostic-free `SecretError`.

`keyring` is a desktop-only dependency, so the CLI dependency graph does not include
native credential backends. It uses Keychain Services on macOS, Windows Credential
Manager, and Secret Service on Linux. The Linux backend needs a user D-Bus session and
a Secret Service implementation; headless or locked sessions return an error. Native
platform builds are CI-gated on macOS, Windows, and Linux.

### Native Credential Identity

Credential identity v1 is a SHA-256 digest of length-prefixed canonical workspace
path, effective environment name, and variable name. The keyring service is
`dev.probe.desktop.credentials.v1`; the account is `v1-` plus the digest. Neither
the raw path nor variable names appear in the native entry key. The canonical path
survives relative paths and symlinks, but a workspace move or rename changes its
identity and leaves the old credential behind. Environment and variable names are the
only durable identities, so renaming either also requires storing the value again.
Inherited declarations are scoped to the selected effective environment. Probe never
migrates or deletes credentials implicitly: deleting a stored value keeps its
declaration, and removing a declaration keeps its stored value.

### Presence Metadata

The desktop session may record opaque identities (`v1-` plus the digest) in a stored
set and a known-missing set. These sets contain no secret values, workspace paths,
environment names, or variable names. They are a rebuildable hint for presentation,
not a source of truth: missing or corrupt session state leaves both sets empty and
does not block execution.

Presentation, including placeholder highlighting and Environment Manager status,
reads only presence metadata. It never reads credential values or queries the
operating-system credential store. A secret whose identity is in neither set is
unknown. Only trusted operations change the sets. Asynchronous credential writes
update presence when they complete, independently of whether the Secret Value dialog
or Environment Manager that initiated them is still open:

- a successful Set records the identity as stored;
- a Delete that succeeds or returns `NotFound` records it as missing;
- execution records the identities the native store returned or did not return.
  Store errors are omitted, so a transient failure does not erase metadata.

Credentials created or deleted outside Probe are not discovered until one of those
operations observes them. This avoids an operating-system credential query every time
a placeholder is painted.

### Execution and Reconciliation

The native credential store is the source of truth when a request runs. Desktop
execution reads reachable secrets through `NativeSecretProvider` on a Tokio blocking
worker and reconciles presence when secret resolution finishes, not when the HTTP
response arrives. Every Set or Delete advances a session-only presence revision. A
send captures that revision when it starts, and a reconciliation from an older
revision is discarded, so an execution that read the store before a newer Set or
Delete cannot overwrite that newer state.

### Secret Material

Probe does not present secret values. Presentation requests, dry runs, and summaries
keep `{{name}}` references; `SecretValue` redacts its formatting; response redaction
follows [HTTP Execution](#http-execution). Desktop value entry is masked, starts
empty, and never reveals a stored value. Secret material exists in ordinary process
memory while it is submitted to the store or used by a request.


## Persistence and Filesystem Synchronization

Interfaces submit domain or repository operations; only the OpenCollection repository
serializes YAML or mutates collection files. Desktop calls prepare work in memory and
execute filesystem operations away from the UI thread.

For request and environment changes, the repository retains the loaded source and
merges supported fields into that document so unknown YAML survives. Under a stable
workspace writer lock it compares the current source with the loaded bytes, writes and
syncs a temporary file, and atomically replaces the destination. Successful writes
refresh the retained baseline. Symlinked workspaces update their canonical target
without replacing the symlink.

Desktop dirty state compares the live request with its last loaded or successfully
saved snapshot. `probe-core` builds the request patch between those snapshots;
the desktop owns save queue and revision tracking. Save completion acknowledges the
captured revision, so edits made while I/O is running remain dirty. Failures and
external-change conflicts retain the
in-memory draft. Closing dirty work requires an explicit save, discard, or cancel
decision.

Structural mutations are also repository-owned. Bundled operations retain unknown
YAML and replace one document atomically. Unbundled multi-document moves and ordering
changes retain rollback data and a recovery manifest; incomplete rollback is reported
as requiring recovery rather than hidden. Every affected retained source is checked
before mutation.

Filesystem notifications are invalidation hints. The desktop debounces them, reloads
through the repository on a background executor, and reconciles baseline, local draft,
and disk state. Non-overlapping field changes merge automatically; overlapping edits,
dirty deletion, and ambiguous rename require an explicit choice. Invalid or partially
written files never replace the last valid workspace.

Accepted reloads rebuild runtime keys and resolve repository selectors for tabs and
presentation state. Confident file renames may remap selectors. Successful Probe writes
refresh the baseline, so their watcher events reconcile as no-ops.

## OpenCollection Validation

Workspace loading requires the OpenCollection `1.0.0` format marker, collection metadata, and
an explicit `bundled` mode matching the source kind. It also validates environment names and
the complete inheritance graph, including duplicate names, missing parents, and cycles. This
validation is shared by `collection validate` and every operation that loads a workspace.


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
