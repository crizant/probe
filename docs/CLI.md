# CLI

The `probe` CLI is non-interactive and separates command output on stdout from human
diagnostics on stderr. Add `--json` to commands that return data or structured errors.

## Commands

```text
probe collection validate <path> [--json]
probe request list <path> [--json]
probe request get <path> <selector> [--json]
probe request run <path> <selector> [--json]
```

`<path>` may be a bundled OpenCollection YAML file or an unbundled collection
directory containing `opencollection.yml` or `opencollection.yaml`.

`request run` validates the workspace and selector but returns
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

`request get --json` returns `authentication`, `body`, `headers`, `method`, `name`,
`queryParameters`, `selector`, and `url`. Missing optional values are JSON `null`.
Headers and query parameters contain stable `disabled`, `name`, and `value` fields.

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

## Exit Codes

| Code | Category |
| ---: | --- |
| 0 | Success |
| 2 | Invalid arguments |
| 3 | Invalid workspace or parse failure |
| 4 | Request not found |
| 5 | Configuration or environment error (reserved for Phase 4) |
| 6 | Execution unavailable or network/execution error |

