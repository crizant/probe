# CLI

The `probe` CLI is non-interactive and separates command output on stdout from human
diagnostics on stderr. Add `--json` to commands that return data or structured errors.

## Commands

```text
probe collection validate <path> [--json]
probe request list <path> [--json]
probe request get <path> <selector> [--environment <name>] [--json]
probe request run <path> <selector> [--environment <name>] [--json]
```

`<path>` may be a bundled OpenCollection YAML file or an unbundled collection
directory containing `opencollection.yml` or `opencollection.yaml`.

For `request get` and `request run`, `--environment <name>` selects an environment,
applies parent environments from `extends`, and interpolates variables in supported
request fields. Without it, `request get` returns the request as stored, including
unresolved `{{variable}}` expressions.

`request run` validates the workspace, selector, and selected environment but returns
`execution_unavailable` until the shared HTTP engine is implemented in Phase 5.

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

## Exit Codes

| Code | Category |
| ---: | --- |
| 0 | Success |
| 2 | Invalid arguments |
| 3 | Invalid workspace or parse failure |
| 4 | Request not found |
| 5 | Configuration or environment error |
| 6 | Execution unavailable or network/execution error |
