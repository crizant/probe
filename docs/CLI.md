# CLI

The `probe` CLI is non-interactive and separates command output on stdout from human
diagnostics on stderr. Add `--json` to commands that return data or structured errors.

## Commands

```text
probe collection validate <path> [--json]
probe request list <path> [--json]
probe request get <path> <selector> [--environment <name>] [--json]
probe request run <path> <selector> [--environment <name>] [--output <file>] [--json]
```

`<path>` may be a bundled OpenCollection YAML file or an unbundled collection
directory containing `opencollection.yml` or `opencollection.yaml`.

For `request get` and `request run`, `--environment <name>` selects an environment,
applies parent environments from `extends`, and interpolates variables in supported
request fields. Without it, `request get` returns the request as stored, including
unresolved `{{variable}}` expressions.

`request run` resolves the request and executes it through the shared asynchronous HTTP
engine. Pressing Ctrl-C cancels the active execution. `--output <file>` writes the raw
response body to the specified path; response metadata remains on stdout.

Use `-` instead of `<path>` to read a bundled OpenCollection YAML document from
stdin. Stdin does not represent an unbundled directory, and requests loaded this way
use bundled structural selectors.

`--quiet` (or `-q`) suppresses stdout for successful commands, which is useful when
only the exit status matters. Failure diagnostics remain on stderr. `--quiet` and
`--json` are mutually exclusive because structured mode always emits a result.

## Request Selectors

Selectors are repository locators, not session-only `RequestKey` values:

- Unbundled collection: workspace-relative YAML path, such as
  `users/list-users.yml`.
- Bundled collection: structural source path, such as `items/0/items/2`.

Use `request list` to discover valid selectors. Request names are never treated as
identity.

## JSON Output

`collection validate --json` returns:

```json
{
  "schemaVersion": 1,
  "collection": {
    "name": "Example",
    "summary": null,
    "version": null
  },
  "counts": {
    "environments": 0,
    "folders": 1,
    "requests": 2
  },
  "valid": true
}
```

Every JSON success and error document has top-level `schemaVersion: 1`. Fields may be
added compatibly within schema version 1, but documented fields will not be removed or
change type without incrementing the version.

`request list --json` returns a `requests` array. Each entry has nullable `method`,
`name`, and `url` fields plus a string `selector`.

`request get --json` returns `authentication`, `body`, `environment`, `headers`,
`method`, `name`, `queryParameters`, `selector`, and `url`. `environment` is the
selected name or JSON `null`. Missing optional values are JSON `null`. Headers and
query parameters contain stable `disabled`, `name`, and `value` fields.

`request run --json` returns:

```json
{
  "schemaVersion": 1,
  "request": {
    "method": "GET",
    "url": "https://api.example.com/users"
  },
  "response": {
    "body": {
      "content": "{\"users\":[]}",
      "encoding": "utf8",
      "omissionReason": null,
      "omitted": false,
      "outputPath": null
    },
    "durationMs": 128,
    "headers": [
      { "name": "content-type", "value": "application/json" }
    ],
    "reason": "OK",
    "sizeBytes": 12,
    "status": 200,
    "url": "https://api.example.com/users"
  }
}
```

UTF-8 bodies up to 1 MiB are included directly. Larger and binary bodies are omitted
from stdout with `omitted: true`; rerun with `--output <file>` to retain them. When an
output file is used, `outputPath` identifies it and `content` remains `null`.

Structured errors use:

```json
{
  "schemaVersion": 1,
  "error": {
    "category": "request_not_found",
    "exitCode": 4,
    "message": "request selector not found: missing.yml"
  }
}
```

`error.category` and `error.exitCode` are the stable programmatic failure contract.
`error.message` is a human diagnostic and may include platform-specific paths or I/O
text, so automation must not parse it. JSON stdout never contains progress output,
terminal escape sequences, or logs.

Environment failures use exit code 5 and stable categories including
`environment_not_found`, `missing_variable`, `secret_variable_unavailable`, and
`environment_resolution`. Secret variables declared by OpenCollection do not contain
their values; until a secure runtime provider is added, referencing one reports
`secret_variable_unavailable` rather than silently substituting an empty value.

HTTP request configuration errors use exit code 5 and category
`request_configuration`. Timeout, cancellation, connection, protocol, and response
body failures use exit code 6 with categories such as `request_timeout`,
`request_cancelled`, and `network_execution`. Output-file failures use `output_error`.
Failure to read a stdin workspace uses `stdin_error` and exit code 3; invalid YAML read
from stdin uses `invalid_workspace` and exit code 3.

## Exit Codes

| Code | Category |
| ---: | --- |
| 0 | Success |
| 2 | Invalid arguments |
| 3 | Invalid workspace or parse failure |
| 4 | Request not found |
| 5 | Configuration or environment error |
| 6 | Network, cancellation, execution, or response-output error |
