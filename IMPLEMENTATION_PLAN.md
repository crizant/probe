# Implementation Status and Roadmap

This document records current scope and future product work. It is not required
reading for ordinary implementation tasks; use the task-specific references in
[AGENTS.md](AGENTS.md) and the [documentation index](docs/README.md).

## Current Scope

The foundation planned for the first desktop release is implemented. Current behavior
is documented in the canonical references listed in the
[documentation index](docs/README.md), not here. Code and tests remain authoritative
when a document falls behind.

## Planned Work

The following capabilities are intentionally deferred. They require explicit task
scope before implementation.

### User-Defined Themes

Add versioned, human-editable theme files after the semantic token model is stable.
Parsing and validation must remain outside components, invalid themes must fall back
safely to built-ins, and theme configuration must remain local presentation data rather
than OpenCollection content. The design contract lives in
[docs/DESIGN.md](docs/DESIGN.md#future-plain-text-themes).

### Streaming Protocols

Design a shared protocol session/event abstraction before adding WebSocket, SSE, or
gRPC. Protocol implementations must be independent of stdin/stdout and GPUI; the CLI
may adapt events to JSONL while the desktop adapts the same events to visual sessions.

### Git Integration

The filesystem remains the primary Git boundary. Optional built-in status, diff,
branch, commit, pull, and push workflows may be added later without coupling core
collection behavior to a hosting provider.

### MCP Interface

An MCP server may eventually become another adapter over the shared application layer.
It must not duplicate business logic or depend on parsing CLI output.

## Planning Rules

- Add a roadmap item only when it expresses product scope not already documented by
  current behavior or tests.
- Move implemented behavior to its canonical product or architecture document instead
  of retaining a completed phase checklist here.
- Do not use historical phase numbers as dependencies. Describe concrete prerequisites
  and affected architectural boundaries.
- Keep speculative provider integrations, cloud services, accounts, telemetry,
  analytics, plugins, and unsupported protocols out of scope until explicitly approved.
