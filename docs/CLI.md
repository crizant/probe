# CLI

The `probe` CLI is non-interactive and separates command output on stdout from human
diagnostics on stderr. Add `--json` to commands that return data or structured errors.

## Commands

```text
probe collection create <path> [--name <name>] [--json]
probe collection import postman <source.json> <destination> [--allow-partial] [--json]
probe collection import yaak <source> <destination> [--workspace <id>] [--allow-partial] [--json]
probe collection validate <path> [--json]
probe collection get <path> [--json]
probe collection set <path> [--summary <text>] [--docs <text>] [--docs-json <json>] [--json]
probe collection unset <path> [--summary] [--docs] [--json]
probe request list <path> [--json]
probe request get <path> <selector> [--environment <name>] [--strict-variables] [--json]
probe request variables <path> <selector> [--environment <name>] [--json]
probe request run <path> <selector> [--environment <name>] [--strict-variables] [--var <name=value>]... [--secret-provider env] [--output <file>] [--show-headers] [--dry-run] [--expect <expr>]... [--json]
probe request set <path> <selector> [--name <name>] [--method <method>] [--url <url>] [--description <text>] [--description-json <json>] [--docs <text>] [--headers <json-array-or-null>] [--query-parameters <json-array-or-null>] [--path-parameters <json-array-or-null>] [--body <json-object-or-null>] [--auth <json-or-null>] [--graphql-query <text>] [--graphql-variables <json-object-or-null>] [--graphql-operation-name <json-string-or-null>] [--graphql-extensions <json-object-or-null>] [--json]
probe request unset <path> <selector> [--description] [--docs] [--json]
probe request create <path> --name <name> [--parent <folder>] [--index <index>] [--method <method>] [--url <url>] [--type http|graphql] [--headers <json-array-or-null>] [--query-parameters <json-array-or-null>] [--path-parameters <json-array-or-null>] [--body <json-object-or-null>] [--auth <json-or-null>] [--graphql-query <text>] [--graphql-variables <json-object-or-null>] [--graphql-operation-name <json-string-or-null>] [--graphql-extensions <json-object-or-null>] [--json]
probe request rename <path> <selector> --name <name> [--json]
probe request delete <path> <selector> [--json]
probe request move <path> <selector> [--parent <folder>] [--index <index>] [--json]
probe request reorder <path> <selector> --index <index> [--json]
probe folder list <path> [--json]
probe folder get <path> <selector> [--json]
probe folder set <path> <selector> [--description <text>] [--description-json <json>] [--docs <text>] [--docs-json <json>] [--json]
probe folder unset <path> <selector> [--description] [--docs] [--json]
probe folder create <path> --name <name> [--parent <folder>] [--index <index>] [--json]
probe folder rename <path> <selector> --name <name> [--json]
probe folder delete <path> <selector> [--json]
probe folder move <path> <selector> [--parent <folder>] [--index <index>] [--json]
probe folder reorder <path> <selector> --index <index> [--json]
probe environment create <path> --name <name> [--extends <parent>] [--json]
probe environment list <path> [--json]
probe environment set <path> --environment <name> (--name <var> --value <value> | --description <text> | --description-json <json>) [--json]
probe environment unset <path> --environment <name> (--name <var> | --description) [--json]
probe environment delete <path> --environment <name> [--json]
probe environment rename <path> --environment <name> --name <new> [--json]
```

`<path>` may be a bundled OpenCollection YAML file or an unbundled collection
directory containing `opencollection.yml` or `opencollection.yaml`.
`collection create` always writes a new bundled YAML file and refuses to overwrite
an existing path. A missing `.yml` extension is added. `--name` sets `info.name`;
otherwise the file stem is used. Stdin (`-`) is not accepted.

`collection import yaak` accepts either an official Yaak export JSON file (schemas
1–4) or a Yaak Directory/Git Sync directory. It converts one Yaak workspace through
the shared import adapter and writes a new bundled OpenCollection YAML file atomically.
The destination is never overwritten and stdin is not accepted. If a source contains
multiple workspaces, pass the exact Yaak workspace ID with `--workspace`; JSON errors
include the selectable IDs and names.

`collection import postman` accepts one official Postman Collection v2.0 or v2.1 JSON
file. It does not accept Postman environment exports, data dumps, or v3 YAML.
Collection variables are stored in a `Postman Collection Variables` OpenCollection
environment and the returned JSON names that environment when one was created.
`--workspace` is Yaak-only and is rejected for Postman imports.

Import is strict by default. Unsupported or unknown data returns
`unsupported_import` without creating the destination. `--allow-partial` explicitly
permits those omissions and returns every deterministic compatibility diagnostic in
`warnings`. Authentication kinds that OpenCollection can store are preserved even if
Probe's current HTTP engine cannot execute them; those appear as warning diagnostics,
not silent data loss.

For `request get` and `request run`, `--environment <name>` selects an environment,
applies parent environments from `extends`, and interpolates variables in supported
request fields. Without it, `request get` returns the request as stored, including
unresolved `{{variable}}` expressions. That flag is not a write operation; use
`environment set` and `environment unset` to persist variable values.

By default, variables without an available value remain literal in `request get` and
`request run`, so `{{name}}` can intentionally be sent. Add `--strict-variables` to
either command to reject undefined or disabled variables with the stable
`missing_variable` error instead. Unavailable secret variables always fail.

`request variables` reports the interpolation variables referenced by a request and
their request locations. With `--environment`, it also reports whether each variable
is defined by the effective inherited environment and whether its declaration is a
secret. It never resolves values, reads secrets, accepts runtime `--var` values, or
executes the request.

`request run` resolves the request and executes it through the shared asynchronous HTTP
engine. Pressing Ctrl-C cancels the active execution. `--output <file>` writes the raw
response body to the specified path using bounded streaming; response metadata remains on
stdout. The destination is replaced only after the complete response has been written.
Human output shows the request method and effective URL from the built HTTP request
(after path/query parameters and URL normalization), status, duration, response size, and
response body. `Final URL:` appears only when redirects change the effective URL
initially sent by the HTTP engine. Path/query parameters and URL normalization
alone do not show it. Response headers are hidden by default; add `--show-headers`
to include them. `--json` always includes response headers and is unaffected by this
flag. `--show-headers` does not change dry-run output.

`--dry-run` uses the same selector, environment, `--var`, and `--strict-variables`
resolution as a live run, including GraphQL-over-HTTP preparation, then exits without
opening a network connection. Text output is the resolved method and URL only; it does
not print headers, bodies, or secret values. `--json` returns the same resolved
`request` object as a live run, plus `"dryRun": true`, and omits `response`.
Unavailable secret variables still fail closed with `secret_variable_unavailable`.
`--dry-run` cannot be combined with `--output` or `--expect`.

`--expect <expr>` asserts a completed live response. v1 accepts `status=<code>` or
one or more HTTP statuses separated by `|` (100–599; `|` is OR). The flag may be
repeated; every expression must pass. Assertions run only after a successful
transport, so timeouts, cancellation, and connection failures keep their existing
execution categories and exit code 6. A missed status uses category
`expectation_failed` and exit code 9. `--json` includes
`expectations: [{expr, ok, actual}]` on success and the same array under
`error.details.expectations` on failure, with no top-level `response`. `--expect`
cannot be combined with `--dry-run`.

Probe supports OpenCollection-native GraphQL items (`info.type: graphql` with a `graphql` section).
Their protocol identity and body stay native when saved. At execution time Probe resolves the same
URL, headers, authentication, environment, and runtime variables used by HTTP requests, then prepares
a GraphQL-over-HTTP request for the shared HTTP engine. `POST` uses a JSON envelope and `GET` uses
the `query`, `variables`, `operationName`, and `extensions` URL parameters. An HTTP `200` response
containing both `data` and `errors` remains a successful transport response; GraphQL application
errors are response data.

`request set` updates native GraphQL fields independently with `--graphql-query`,
`--graphql-variables`, `--graphql-operation-name`, and `--graphql-extensions`; the query does not
need to be supplied again. Pass JSON `null` to clear variables, extensions, or the operation name;
for operation name, pass a JSON string (e.g. `"Viewer"`) to set it or JSON `null` to clear it.
`request create --type graphql` writes a native GraphQL item (`info.type: graphql`). GraphQL body
flags on create also imply that protocol. `request set` cannot convert between HTTP and GraphQL
protocols; recreate the request instead. OpenCollection 1.0.0
formally defines `query` and JSON-string `variables` in the
native GraphQL body. Probe also preserves `operationName` and `extensions` there as forward-compatible
GraphQL-over-HTTP fields; strict OpenCollection 1.0.0 schema validators may reject those two fields.

`request set` and `request create` also replace headers, query parameters, path
parameters, the HTTP body, and authentication. Pass JSON `null` to clear any of
those fields. An empty JSON array also clears a header or parameter list. Query
and path parameters are replaced independently, so changing one leaves the other
in place. Header and parameter objects require string `name` and `value`;
`disabled` is an optional boolean and defaults to false.

An HTTP body is a JSON object with `type` and `data`. The types are `json`,
`text`, `xml`, `sparql`, `form-urlencoded`, `multipart-form`, and `file`. Raw
types store `data` as a string. Form bodies store an array of `{name, value,
disabled}` fields. Multipart parts use `type` `text` or `file`, a string or
string-array `value`, and an optional `contentType`. Each file-body entry
requires `filePath`, `contentType`, and boolean `selected`. When the saved body
is already a variant list, `--body` updates only the selected variant's body and
keeps every variant's title and selected flag. `--body null` removes the whole
body, including a variant list. The CLI does not add, remove, rename, or
reselect variants.

Authentication writes accept the JSON string `"inherit"`, or a JSON object for
`basic` (`username`, `password`), `bearer` (`token`), or `apikey` (`key`,
`value`, and `placement` of `header` or `query`). Other schemes, including
OAuth and AWS Signature, are rejected. Files that already store those schemes
still load, and edits that do not set `--auth` leave them unchanged. HTTP body
flags are rejected for a native GraphQL request.

Repeatable `--var <name=value>` arguments provide invocation-only variables for `request run`.
They override selected and inherited environment values before dependent variables are
interpolated, and also work without `--environment`. If a name is repeated, the last value
wins. Runtime variables are never written to the environment or collection source.

An enabled OpenCollection variable declared with `secret: true` has no value in the
collection file. For `request run --environment <name> --secret-provider env`, Probe
reads its effective logical name from the process environment. For example, a
declaration named `apiToken` reads the process variable `apiToken`; the request
continues to use `{{apiToken}}`. Inherited declarations and overrides follow the
normal environment rules. Enable this provider only for trusted collections: a
collection controls both the process-variable names it reads and the outbound
request destination. No `.env` file is loaded automatically. If a referenced
secret is missing, the run fails with `secret_variable_unavailable` before HTTP
execution. A backend failure is reported without its diagnostic content only if
the request uses that secret, directly or through another variable. Unused secret
declarations do not prevent an unrelated request from running.

The outbound request receives the runtime value. Request summaries, dry runs, and
JSON retain the `{{name}}` reference for secret fields. Exact occurrences of secret
values in the response reason, headers, and body are redacted, including byte sequences
in non-UTF-8 bodies. Failure messages for such a run withhold HTTP diagnostics; the error
category and exit code still reflect the failure kind. Encoded or otherwise
transformed echoes may remain visible in CLI output. When a runtime secret is used,
the reported final URL is the presentation request URL because redirects can encode
or transform secret text. The initial URL uses that safe presentation URL only when
URL, path, or query fields are secret-derived (including GraphQL GET parameters and
query API-key authentication), or a secret-derived native GraphQL method controls
whether fields enter the URL. A secret-derived ordinary HTTP method does not hide
the initial URL. Secrets confined to headers, bodies, or header
authentication do not hide the effective initial URL. `--output <file>` intentionally saves the original server
response bytes; that file can contain echoed secrets and must be handled as sensitive.
A `--var name=value` override of a declared secret also stays secret, but process
environment injection is preferred: command-line
arguments may appear in shell history or process inspection. The CLI does not read
values stored in the operating-system credential store by the desktop application.

Use `-` instead of `<path>` to read a bundled OpenCollection YAML document from
stdin. Stdin does not represent an unbundled directory, and requests loaded this way
use bundled structural selectors.

`request set` is a deliberately small, non-interactive persistence command. At
least one supported request field is required. It updates the in-memory
request first, merges only those fields into the retained YAML document, and then
atomically replaces the source file. It is unavailable for stdin workspaces.

`collection get` reads collection `info.summary` and root `docs`. `collection set`
writes those fields only. OpenCollection collections have no `description` field,
and `--description` and `--description-json` are rejected. `folder get` reads one folder's `info.description`
and `docs`. `folder set` writes those fields only. Folder and collection
documentation accepts a plain string via `--description` or `--docs`, or a JSON
string, `null`, or `{"content","type"}` object via `--description-json` or
`--docs-json`. An object is stored as `{content, type}` and is not flattened to
its content string. Explicit JSON `null` is stored as YAML null. `null` does not
remove the field.

`collection unset`, `folder unset`, `request unset`, and `environment unset --description`
remove fields from the file. At least one field flag is required.
`--summary` and `--docs` on collection unset, `--description` and `--docs` on
folder and request unset, and `--description` on environment unset, take no value
and omit that key. They do not write YAML null. `set` remains set-only.

Request `info.description` uses the same documentation value. Request `docs` is a
plain string: `--docs <text>` writes that string, and `--docs-json` is rejected
because an object or null is not valid request docs. `request list` and
`folder list` omit description and docs. `request get`, `folder get`, and
`collection get` include them. Documentation writes are not applied by rename,
move, create, or other field updates unless that command is given the
documentation flag.

`environment set` and `environment unset` persist OpenCollection environment variables
and the environment `description` through the same repository path. `--environment`
names the environment to mutate; it does not resolve a request. One command writes
either a variable or a description. `set --name <var> --value <value>` writes a plain
variable on that environment, updating it when present or adding an override when the
value currently comes from a parent. `unset --name <var>` removes the variable entry
from that environment only, so a parent value can show through. Both variable commands
reject secrets, empty names, and stdin workspaces.

`set --description <text>` writes a plain string at that environment's `description`.
`set --description-json <json>` writes a JSON string, YAML null, or a `{content, type}`
object, using the same documentation value as folder and request descriptions. An
object is stored as `{content, type}` and is not flattened to its content string.
Explicit JSON `null` is stored as YAML null and does not remove the field.
`unset --description` takes no value and deletes the `description` key. It does not
write YAML null. `--description` and `--description-json` cannot be combined with
`--name` or `--value`. `unset --description-json` is rejected.

`environment delete` and `environment rename` use the same `--environment` flag for the
existing environment. `--name` on rename is the new identity, matching `environment create`.
Parent environments cannot be deleted or renamed; that failure is `environment_in_use`.
Stdin workspaces cannot be persisted.

Before committing, Probe compares the source file with the exact bytes that were
loaded. If another process changed it, the command fails with `workspace_modified`
instead of overwriting the external edit. Unknown YAML fields are retained, although
comments and original formatting may change when the YAML document is serialized.

Structural commands are non-interactive and use the same repository operation for bundled and
unbundled workspaces. Omit `--parent` for the collection root and omit `--index` to append.
`reorder` keeps an item in its current parent and requires its new zero-based `--index`.
Bundled selectors are structural and may change when siblings move. Unbundled creation and rename
derive lowercase hyphenated paths from names (requests use `.yml`); an existing destination is
never overwritten.

Bundled edits atomically replace the single collection document. Unbundled ordering is persisted
as `info.seq` in each affected sibling document. Multi-file ordering writes retain rollback
snapshots, and file/directory moves are rolled back if metadata persistence fails. Every retained
source is compared byte-for-byte before a structural write, so external changes fail safely.
Durable recovery directories with manifests are retained if a multi-document rollback cannot
complete.

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
  "valid": true,
  "warnings": []
}
```

Validation succeeds when a valid collection contains future item, body, or parameter
types or unsupported authentication fields. `warnings` identifies each retained YAML
location with a structural `path`, a stable `code`, and the unsupported `value`.
Authentication warnings can describe values retained in the projected request but
ignored by Probe's HTTP execution engine.
The human output lists up to 20 warnings and the total count. Unknown source values
remain in the YAML when supported fields are edited and saved.

Every JSON success and error document has top-level `schemaVersion: 1`. Fields may be
added compatibly within schema version 1, but documented fields will not be removed or
change type without incrementing the version.

`collection create --json` returns:

```json
{
  "schemaVersion": 1,
  "collection": {
    "name": "pets"
  },
  "counts": {
    "environments": 0,
    "folders": 0,
    "requests": 0
  },
  "created": true,
  "path": "/tmp/pets.yml"
}
```

`collection import yaak --json` returns:

```json
{
  "schemaVersion": 1,
  "counts": { "environments": 1, "folders": 1, "requests": 1 },
  "defaultEnvironment": "Global Variables",
  "imported": true,
  "partial": false,
  "path": "/tmp/imported.yml",
  "sourceFormat": "yaak_export",
  "projectionWarnings": [],
  "warnings": [],
  "workspace": { "id": "wk_1", "name": "Pets" }
}
```

`collection import postman --json` returns:

```json
{
  "schemaVersion": 1,
  "collection": { "id": "8dcb...", "name": "Pets" },
  "collectionVariablesEnvironment": "Postman Collection Variables",
  "counts": { "environments": 1, "folders": 1, "requests": 2 },
  "imported": true,
  "partial": false,
  "path": "/tmp/imported.yml",
  "sourceFormat": "postman_collection_v2_1",
  "projectionWarnings": [],
  "warnings": []
}
```

`collection.id`, `collection.name`, `collectionVariablesEnvironment`, and `defaultEnvironment` are nullable.
Postman v2.0 uses `postman_collection_v2_0` as `sourceFormat`.
`projectionWarnings` uses the same `{path, code, value}` shape as validation
warnings for values preserved in the created OpenCollection file but unsupported
by Probe's runtime.

`request list --json` returns a `requests` array. Each entry has nullable `method`,
`name`, and `url` fields plus a string `selector` and a `type` field (`http` or `graphql`).
List entries omit `description` and `docs`.

`folder list --json` returns a `folders` array in deterministic collection order.
Each entry has nullable `name` and `parent` fields plus a string `selector`.
List entries omit `description` and `docs`.

`collection get --json` returns `collection.name`, `collection.summary`, and
`collection.docs`. `folder get --json` returns `name`, `selector`, `description`,
and `docs`. Documentation JSON is a string, `null`, or an object
`{"content","type"}`. `null` covers both an omitted field and explicit null.
`collection set --json` and `folder set --json` return the same objects after the
write, with `"updated": true`. `collection unset --json` returns `operation` and
`fields`. `folder unset --json` and `request unset --json` also return
`selector`. `fields` lists the removed names in command order: `summary`,
`description`, then `docs`.

`request get --json` returns `authentication`, `body`, `description`, `docs`, `environment`, `graphql`, `headers`,
`method`, `name`, `pathParameters`, `queryParameters`, `selector`, and `url`. `description`
is a documentation value. `docs` is a string or `null`. `environment` is the
selected name or JSON `null`. Missing optional values are JSON `null`. Headers and
query and path parameters contain stable `disabled`, `name`, and `value` fields. Path
parameters are referenced from URLs with `:variableName` segments.

The additional `type` field is `http` or `graphql`. `graphql` is JSON `null` for HTTP requests.
For native GraphQL requests it contains nullable `query`, `variables`, `operationName`, and
`extensions` fields for the selected body. GraphQL body variants must have exactly one selected
entry for inspection, editing, or execution.

`request set --json` returns the same request shape after the persisted update,
with `environment` set to JSON `null`.

`request variables --json` returns variables sorted by name, with deduplicated usages in
request-field order:

```json
{
  "schemaVersion": 1,
  "variables": [
    {
      "name": "token",
      "defined": true,
      "secret": true,
      "usages": [
        { "location": "header", "name": "Authorization" },
        { "location": "authentication", "name": "token" }
      ]
    }
  ]
}
```

Without `--environment`, `defined` and `secret` are false because no effective
environment was selected. Usage locations are `method`, `url`, `header`,
`query_parameter`, `path_parameter`, `body`, `graphql_query`, `graphql_variables`,
`graphql_operation_name`, `graphql_extensions`, `form_urlencoded`, `multipart`, `file`,
or `authentication`. Named request fields also include `name`.

`environment list` prints `NAME` and `EXTENDS` as tab-separated text.
`environment list --json` returns an `environments` array. Each entry has `name`,
nullable `extends`, and `description` when the environment has one. `description` is
a string, `null` for an explicit YAML null, or a `{"content","type"}` object. An
environment with no description omits `description` instead of returning JSON null.

`environment set --json` and `environment unset --json` for a variable return
`environment`, `name`, and `operation`. `set` also returns `value`. A description
`set` returns `environment`, `operation`, and `description`. `unset --description`
returns `environment`, `operation`, and `fields` containing `description`.

`environment create --json` returns `environment` and `operation`, plus `extends` when a
parent was supplied. `environment delete --json` returns `environment` and `operation`.
`environment rename --json` returns `environment` (the new name), `previousEnvironment`,
and `operation`.

Structural commands return stable fields `operation`, `itemType`, `previousSelector`, `selector`,
`parent`, `index`, and `selectorRemaps`. The remap object contains every surviving known
repository selector, including siblings whose bundled structural selector shifted. `selector`
and `index` are `null` after deletion; `previousSelector` is `null` after creation.

`request run --json` returns:

```json
{
  "schemaVersion": 1,
  "request": {
    "type": "http",
    "graphql": null,
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

UTF-8 bodies up to 16 MiB are included directly. Larger and binary bodies are omitted
from stdout with `omitted: true`; rerun with `--output <file>` to retain them. When an
output file is used, `outputPath` identifies it and `content` remains `null`.

`request run --dry-run --json` returns the resolved request without executing it:

```json
{
  "schemaVersion": 1,
  "dryRun": true,
  "request": {
    "type": "http",
    "graphql": null,
    "method": "GET",
    "url": "https://api.example.com/users"
  }
}
```

The `request` object matches a live `request run --json` document. There is no
`response` field because no HTTP request is sent.

When `--expect` is present and every assertion passes, `request run --json` also
includes an `expectations` array:

```json
{
  "schemaVersion": 1,
  "request": {
    "type": "http",
    "graphql": null,
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
  },
  "expectations": [
    { "expr": "status=200", "ok": true, "actual": 200 }
  ]
}
```

`expr` is the supplied argument. `actual` is the HTTP status from the completed
exchange. Body, header, and JMESPath assertions are out of scope for this version.

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
`environment_not_found`, `duplicate_environment`, `environment_in_use`,
`missing_variable`, `variable_not_found`,
`secret_variable_unavailable`, and
`environment_resolution`. Secret variables declared by OpenCollection do not contain
their values; a referenced secret with no invocation override or selected runtime
provider value reports `secret_variable_unavailable` rather than silently substituting
an empty value.

HTTP request configuration errors use exit code 5 and category
`request_configuration`. Timeout, cancellation, connection, protocol, and response
body failures use exit code 6 with categories such as `request_timeout`,
`request_cancelled`, and `network_execution`. Output-file failures use `output_error`.
Failure to read a stdin workspace uses `stdin_error` and exit code 3; invalid YAML read
from stdin uses `invalid_workspace` and exit code 3.

Persistence failures use exit code 7. Stable categories are `workspace_modified` for
external-modification conflicts, `persistence_read_only` for stdin sources,
`recovery_required` when a multi-file rollback could not be completed, and
`committed_refresh_failed` when persistence succeeded but the workspace could not be refreshed.
`committed_cleanup_failed` means deletion committed but an out-of-workspace tombstone requires
manual cleanup. Callers must not retry operations reported as committed failures without
reloading the workspace. Other serialization or filesystem failures use `persistence_error`.

Structural validation uses stable categories `folder_not_found`, `destination_not_found`,
`duplicate_destination`, `invalid_destination`, `invalid_name`, and `invalid_index`.
Missing request/folder selectors use exit code 4; invalid destinations, names, duplicates, and
indices use exit code 2.

Postman and Yaak compatibility failures use exit code 8 and category
`unsupported_import`. Malformed or unsupported import schemas use `invalid_import` and
exit code 3; a missing or ambiguous Yaak workspace selection, invalid provider-specific
arguments, and an existing destination use exit code 2. Other destination write
failures use the existing persistence categories and exit code 7.

A completed `request run` that misses `--expect` uses exit code 9 and category
`expectation_failed`. Invalid `--expect` expressions and combining `--expect` with
`--dry-run` use `invalid_arguments` and exit code 2.

`collection validate` requires the OpenCollection `1.0.0` marker, explicit collection
metadata, and a `bundled` flag matching whether the source is a bundled file/stdin document or
an unbundled directory. Duplicate environments and invalid inheritance graphs are rejected.

## Exit Codes

| Code | Category |
| ---: | --- |
| 0 | Success |
| 2 | Invalid arguments |
| 3 | Invalid workspace or parse failure |
| 4 | Request or folder not found |
| 5 | Configuration or environment error |
| 6 | Network, cancellation, execution, or response-output error |
| 7 | Persistence failure or external-modification conflict |
| 8 | Import compatibility failure |
| 9 | Expectation failure |
