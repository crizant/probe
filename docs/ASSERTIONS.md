# Shared assertion evaluator

`probe_application::evaluate_assertions(&[probe_core::Assertion], &HttpResponse,
&RequestResolution, &dyn SecretProvider) -> Vec<AssertionResult>` is a standalone
operation. HTTP and GraphQL use the same completed transport response; GraphQL's
`data` and `errors` remain ordinary JSON properties. CLI and desktop sends invoke the shared
evaluator through the application execution boundary before response redaction.
Collection persistence and the YAML format are unchanged. See [CLI](CLI.md) for
`--assert`, `--no-assertions`, structured results, exit codes, and the breaking
migration from Probe v1.1.1's removed `--expect` flag.

Each input declaration produces one result in source order, with its zero-based
index and `Passed`, `Failed`, `Disabled`, or `Error(AssertionError)` outcome.
Errors have structured categories and value-free `Display` diagnostics. Disabled
assertions do not parse expressions, resolve values, or inspect the response.
Failures and errors do not stop subsequent assertions. Input data is immutable.

## Upstream contract and Probe's subset

Reviewed OpenCollection 1.0.0 at upstream commit
[`535a5c5803da7a3fe1d575fff23350b4a7c43c6a`](https://github.com/opencollection-dev/opencollection/tree/535a5c5803da7a3fe1d575fff23350b4a7c43c6a).
The [assertion documentation](https://github.com/opencollection-dev/opencollection/blob/535a5c5803da7a3fe1d575fff23350b4a7c43c6a/packages/oc-spec-site/src/components/sections/Assertions.jsx)
lists the eight operators below and demonstrates `response.status` and
`response.body.users.length`. The
[types](https://github.com/opencollection-dev/opencollection/blob/535a5c5803da7a3fe1d575fff23350b4a7c43c6a/packages/oc-types/src/common/assertions.ts)
and [schema](https://github.com/opencollection-dev/opencollection/blob/535a5c5803da7a3fe1d575fff23350b4a7c43c6a/packages/oc-schema/src/opencollection.schema.json)
declare string expressions, operators, and optional string values. They do not
specify an expression grammar, numeric precision, coercion, missing-property
behavior, string-length units, literal quoting, or interpolation semantics.
The rules below are Probe's supported evaluation subset, not additional upstream
requirements or YAML extensions.

Supported expressions:

- Exactly `response.status`, yielding a JSON number.
- `response.body`, yielding the JSON body, followed by zero or more `.property`
  or `[index]` segments. Properties use ASCII identifiers
  `[A-Za-z_$][A-Za-z0-9_$]*`. Indexes are nonnegative decimal integers without
  leading zeroes, bounded by the platform's `usize`.
- Terminal `.length` on arrays yields the element count; on strings it yields
  the Unicode scalar count. On objects, `length` is an ordinary property name.

Whitespace around expressions, quoted bracket properties, negative indexes,
wildcards, filters, functions, arithmetic, response headers, and other roots are
unsupported and return `UnsupportedExpression`. There is no JavaScript or
general-purpose expression engine. Expression and operator strings are never
interpolated. Unknown operators always return `UnsupportedOperator`.

Absent properties and out-of-range indexes return `MissingProperty`, including
for `isNull`, `isNotNull`, and negative comparisons. An existing JSON null is a
value, not an absent property. Traversing incompatible types returns
`IncompatibleOperands`.

## Values and operators

Expected values that contain `{{` are interpolated before literal parsing.
Values with no interpolation are used as written and do not resolve an
environment or request secrets. Each assertion resolves its own reachable references; provider lookups are cached
for the evaluation batch.

- JSON numeric literals become numbers; `true`, `false`, and `null` become their
  respective JSON types. Surrounding whitespace on these literals is ignored.
- Unquoted text that only begins with a digit or sign, and is not a JSON number,
  stays a string. `2024-01-01`, `1.0.0`, `200 OK`, `+200`, `01`, `1.`, and `-`
  are strings. A leading dot, as in `.5`, is not a JSON number either.
  `+200` is the string `"+200"` and is not equal to the number 200. Text that
  matches JSON number grammar but does not fit serde_json's finite
  representation, such as `1e400` or `1e9999`, is still an error.
- Double-quoted JSON string literals retain string semantics and decode JSON
  escapes, so `"200"` compares as a string. Malformed quoted strings error.
  YAML quoting alone is removed by YAML parsing: use `value: '"200"'` to store
  an explicitly quoted string literal.
- Other text, including the empty string, remains a string with its whitespace
  preserved. Single-quoted expression literals and JSON array/object literals
  are not supported. Nonfinite numbers and unsupported literals error.

| Operator | Supported operands and behavior |
| --- | --- |
| `equals`, `notEquals` | Typed JSON equality or its negation; different JSON types are unequal, with no coercion. Numbers compare numerically, including integer versus floating representations. |
| `contains`, `notContains` | Case-sensitive substring match when the actual value is a string. The needle is the expected value's source text, so `"2024-01-01"` contains `2024` and `1e3` matches those characters rather than `1000`. Quoted strings contribute their decoded text. Non-string actuals, including numbers, are `IncompatibleOperands`; `200` contains `20` errors. |
| `greaterThan`, `lessThan` | Strict numeric ordering. Both operands must be numbers. |
| `isNull`, `isNotNull` | Test an existing value for JSON null or its negation. No expected value is required; a supplied value is ignored without interpolation. |

Binary operators require a value; omitted and empty values are distinct. Invalid
operand types for substring or ordering operators return `IncompatibleOperands`.
JSON numbers use `serde_json`'s signed/unsigned 64-bit integer and finite f64
representation. Decimal comparison preserves integer precision without casting
integers to floats; floating literals retain the parser's f64 precision.

Body assertions require valid JSON and `HttpResponse.body_complete == true`.
An empty or invalid body returns `InvalidJsonBody`; an incomplete preview returns
`BodyUnavailable`, even if the preview happens to be valid JSON. This operation
does not read spool files or perform filesystem I/O. Status-only assertions work
regardless of body availability. Future adapters must supply an unredacted
completed response for secret comparisons; evaluating an already redacted response
cannot recover its original values. `PreparedRequest::execute_with_assertions`
evaluates original bytes internally and returns only safe outcomes and redacted
response data. HTTP failures do not evaluate assertions. CLI explicit output files
retain the existing original-byte behavior. Desktop sends with assertions defer
file saving until after evaluation and redaction. If the complete body is unavailable,
the desktop keeps the response and results visible and reports the save failure.

## Variables and disclosure

The evaluator reuses `resolve_environment_for_templates_with_provider` and
`ResolvedEnvironment::interpolate_strict` when an expected value contains `{{`.
The `RequestResolution` input supplies the same selected environment, inheritance,
invocation overrides, workspace identity, and provider context used for requests.
Disabled declarations shadow inherited values according to the existing resolver.
Only secrets reachable from an active binary assertion value that contains
interpolation are requested, including transitive references.
The standalone evaluator caches provider values and failures across its batch.
Integrated execution reuses the existing preparation `CachedSecrets`, sharing lookups
with request and certificate resolution. Request, certificate, and assertion secrets
are combined and deduplicated before a single longest-first redaction of the original
response; raw response data and resolved values never cross the application boundary. Runs with assertions bypass response spool caching so those
files cannot retain assertion-only secrets. Disabled and unary declarations never
resolve ignored values.

Missing, disabled, malformed, unavailable, or provider-failed references return
`VariableResolution`. Assertions always require available values, including when
`strict_variables` is false or no environment was selected: an unresolved template
cannot establish a pass. This assertion-specific rule does not alter the existing
strict/non-strict request-sending behavior.

Results contain no expressions, descriptions, operand values, variable names,
provider diagnostics, parser errors, response content, or secret-bearing execution
state. `Debug`, `Display`, and error sources cannot expose those inputs, including
secrets that a server echoes or transforms. Interfaces can associate results with
declarations by index; any future display of declarations needs its own disclosure
review. Providers may block, and JSON parsing can be expensive; desktop callers
must run the evaluator away from the UI thread.

## Desktop

HTTP and GraphQL requests have an **Assertions** editor tab. Add, enable or disable,
edit, delete, and move declarations up or down. The operator menu offers the eight
supported operators and retains an existing unknown operator until explicitly changed.
Descriptions are optional; editing other properties preserves structured descriptions,
explicit nulls, and omitted fields. Unary operators keep any unused expected value.
`{{variables}}` remain ordinary text in the editor. Changes use the normal draft,
dirty indicator, Save, reload, and reconciliation workflows. WebSocket requests do
not offer this editor.

Every HTTP/GraphQL send evaluates the request's current declaration snapshot. The
response **Assertions** tab appears when there are results, even for a truncated
body. It shows a summary and numbered Passed, Failed, Error, or Disabled outcomes.
Errors use shared value-free diagnostics. Expressions, descriptions, and expected
or actual values are never copied into result diagnostics; inspect declarations in
the Request Editor. Failures and assertion errors keep all response tabs available.
Status assertions work on truncated responses; body assertions report body unavailable.

Results belong to the completed execution, so later edits do not change them. A new
send replaces them; cancellation, closing or deleting the request, and workspace
replacement discard them. Reloads preserve valid response/result pairs through the
existing request-key remapping and execution-generation checks.
