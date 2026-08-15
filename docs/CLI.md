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

`request list --json` returns a `requests` array. Each entry has `method`, `name`,
`selector`, and `url` fields.

`request get --json` returns `authentication`, `body`, `environment`, `headers`,
`method`, `name`, `queryParameters`, `selector`, and `url`. `environment` is the
selected name or JSON `null`. Missing optional values are JSON `null`. Headers and
query parameters contain stable `disabled`, `name`, and `value` fields.

`request run --json` returns:

```json
{
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
  "error": {
    "category": "request_not_found",
    "message": "request selector not found: missing.yml"
  }
}
```

JSON stdout never contains progress output, terminal escape sequences, or logs.

Environment failures use exit code 5 and stable categories including
`environment_not_found`, `missing_variable`, `secret_variable_unavailable`, and
`environment_resolution`. Secret variables declared by OpenCollection do not contain
their values; until a secure runtime provider is added, referencing one reports
`secret_variable_unavailable` rather than silently substituting an empty value.

HTTP request configuration errors use exit code 5 and category
`request_configuration`. Timeout, cancellation, connection, protocol, and response
body failures use exit code 6 with categories such as `request_timeout`,
`request_cancelled`, and `network_execution`. Output-file failures use `output_error`.

## Exit Codes

| Code | Category |
| ---: | --- |
| 0 | Success |
| 2 | Invalid arguments |
| 3 | Invalid workspace or parse failure |
| 4 | Request not found |
| 5 | Configuration or environment error |
| 6 | Network, cancellation, execution, or response-output error |
