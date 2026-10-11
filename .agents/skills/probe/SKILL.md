---
name: probe
description: Use Probe CLI to inspect, edit, validate, and execute OpenCollection API collections. Prefer Probe commands over manually editing OpenCollection YAML when Probe supports the operation.
---

# Probe

Use Probe as the primary interface for working with OpenCollection API collections.

Probe is designed to be scriptable and agent-friendly. Prefer structured CLI commands and `--json` output over parsing human-readable output.

## Core workflow

When modifying an existing request:

1. Discover the request with `probe request list <path> --json`.
2. Inspect it with `probe request get <path> <selector> --json`.
3. Modify it using `probe request set`, `unset`, `rename`, `move`, or `reorder`.
4. Run `probe collection validate <path> --json`.
5. When runtime resolution matters, run the request with `--dry-run --json`.
6. Execute the request only when execution is appropriate.
7. Use `--expect` for HTTP/GraphQL response status assertions. For WebSocket sessions, bound execution with `--max-messages` and/or `--timeout`.

Do not manually edit OpenCollection YAML unless Probe cannot perform the required operation.

## CLI reference

Before inventing a command, flag, or behavior, consult the full Probe CLI documentation.

- In the Probe repository: `docs/CLI.md`
- When installed as an agent skill: `references/cli.md`

Treat that documentation as the source of truth for CLI syntax and edge cases.

## Workspace paths

`<path>` may point to:

- a bundled OpenCollection YAML file; or
- an unbundled OpenCollection directory containing `opencollection.yml` or `opencollection.yaml`.

Use the existing workspace format. Do not convert between bundled and unbundled formats unless explicitly requested.

## Discover before changing

Do not guess request or folder selectors.

Use:

```bash
probe request list <path> --json
probe folder list <path> --json
probe environment list <path> --json
```

Inspect existing state before modifying it:

```bash
probe request get <path> <selector> --json
probe folder get <path> <selector> --json
probe collection get <path> --json
```

## Requests

Create requests with:

```bash
probe request create <path> --name <name> ...
```

Modify requests with:

```bash
probe request set <path> <selector> ...
```

Remove optional fields with:

```bash
probe request unset <path> <selector> ...
```

Do not assume `set` and `unset` are interchangeable. Use `unset` when a field should be removed from the OpenCollection document rather than assigned a null-like value.

Structural operations are available through:

```bash
probe request rename
probe request move
probe request reorder
probe request delete
```

Prefer these commands over direct YAML restructuring.

## Variables and environments

Before diagnosing variable-resolution problems, inspect variable usage:

```bash
probe request variables <path> <selector> --json
```

When an environment matters:

```bash
probe request get <path> <selector> \
  --environment <environment> \
  --json
```

Use runtime variables for invocation-specific values:

```bash
probe request run <path> <selector> \
  --var name=value
```

Runtime `--var` values must not be persisted unless the user explicitly asks to update an environment.

Use environment commands to persist environment state:

```bash
probe environment list
probe environment create
probe environment set
probe environment unset
probe environment rename
probe environment delete
```

## Validation

After modifying collection structure or persisted request data, run:

```bash
probe collection validate <path> --json
```

Treat validation failures as errors that should be resolved before finishing the task.

Validation checks the OpenCollection workspace itself. It is not a substitute for request runtime resolution.

## Dry runs

Use a dry run when you need to verify how a request resolves without making a network request:

```bash
probe request run <path> <selector> \
  --environment <environment> \
  --dry-run \
  --json
```

Add `--strict-variables` when unresolved variables should cause failure:

```bash
probe request run <path> <selector> \
  --environment <environment> \
  --strict-variables \
  --dry-run \
  --json
```

Dry runs use Probe's normal request preparation path but do not open a network connection.

Prefer dry runs before live execution when:

- variables or environments were changed;
- the URL or method was changed;
- GraphQL configuration was changed;
- authentication configuration was changed;
- the target environment may be sensitive.

## Executing requests

Execute with:

```bash
probe request run <path> <selector> --json
```

When verifying expected HTTP status:

```bash
probe request run <path> <selector> \
  --expect status=200 \
  --json
```

Multiple `--expect` flags may be used when supported by the requested verification.

Do not execute requests unnecessarily when inspection, validation, or a dry run is sufficient.

Be especially cautious with requests that may mutate or delete data.

## Secrets

Never print, expose, persist, or reconstruct secret values unless explicitly required for the task.

Secret variables declared by OpenCollection do not store their secret value in the collection file.

Do not replace a secret reference such as:

```text
{{apiToken}}
```

with a literal credential.

When CLI secret resolution is required and the process environment is the intended provider, Probe supports:

```bash
--secret-provider env
```

Do not assume values stored by the desktop application's operating-system credential store are available to the CLI.

Prefer environment-based secret injection over passing secret values directly on the command line.

## JSON output

For agent workflows, prefer `--json` whenever the command supports it.

Use structured fields and stable error categories from Probe output. Do not scrape human-readable output when equivalent JSON is available.

## GraphQL

Probe stores GraphQL requests natively in OpenCollection.

Use the GraphQL-specific request fields rather than manually converting GraphQL requests into generic HTTP bodies.

Available request fields include:

```text
--graphql-query
--graphql-variables
--graphql-operation-name
--graphql-extensions
```

Do not convert an existing HTTP request into GraphQL, or GraphQL into HTTP, using `request set` if Probe does not support that conversion. Recreate the request when protocol conversion is required.

## Collection and folder metadata

Use Probe commands to modify collection and folder metadata rather than editing YAML directly:

```bash
probe collection get
probe collection set
probe collection unset

probe folder get
probe folder set
probe folder unset
```

Respect the distinction between setting a value, setting JSON null where supported, and removing a field with `unset`.

## Imports

Probe can import supported external API-client formats.

Use the dedicated import commands instead of manually converting files:

```bash
probe collection import postman <source> <destination>
probe collection import yaak <source> <destination>
```

Imports are strict by default. Only use partial import when data loss or unsupported fields are explicitly acceptable.

## Safety rules

Before executing a request:

- identify the selected environment;
- inspect the effective request when necessary;
- prefer `--dry-run` if execution is not required;
- avoid destructive requests unless the user's task requires them;
- never expose secrets in output;
- do not silently change environments, credentials, or runtime variables.

When uncertain whether the user wants a persisted change or a one-time runtime override, prefer the least destructive operation.

## Preferred agent pattern

For a typical request-editing task:

```text
request list
→ request get
→ request set / unset
→ collection validate
→ request run --dry-run
→ request run --expect
```

Skip unnecessary steps when the task does not require them.

The goal is to use Probe as the authoritative interface for OpenCollection operations, while keeping changes deterministic, inspectable, and safe.
## WebSocket execution

Use `probe request run <path> <selector>` for native WebSocket requests. Human
output streams interactively (`>` actual outbound messages, `<` inbound messages).
For agent execution, prefer `--max-messages 1 --timeout 10 --json`: live WebSocket
`--json` is NDJSON, one versioned SessionEvent per line, and terminal errors retain
nonzero exit status. HTTP/GraphQL and dry-run JSON remain single objects.
Repeatable `--send` sends literal text without interpolation after the configured
message. Stdin lines can send more messages; EOF does not close the session.
`--max-messages` counts inbound data only. `--timeout` is the CLI live-run bound,
separate from persisted connection timeout. `--expect`, `--output`, and
`--show-headers` are HTTP/GraphQL-only. Configured binary messages lack an encoding
contract and cannot execute; received binary frames are supported as base64.
