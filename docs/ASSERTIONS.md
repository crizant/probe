# Shared assertion evaluator

`probe_application::evaluate_assertions(&[probe_core::Assertion], &HttpResponse,
&RequestResolution, &dyn SecretProvider) -> Vec<AssertionResult>` is a standalone
operation. HTTP and GraphQL use the same completed transport response; GraphQL's
`data` and `errors` remain ordinary JSON properties. Request execution, CLI flags,
exit codes, desktop presentation, and collection persistence do not invoke or
change this operation.

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
environment or request secrets. Each assertion still resolves independently.

- JSON numeric literals become numbers; `true`, `false`, and `null` become their
  respective JSON types. Surrounding whitespace on these literals is ignored.
- Unquoted text that only begins with a digit or sign, and is not a JSON number,
  stays a string. `2024-01-01`, `1.0.0`, `200 OK`, and `+200` are strings.
  `+200` is the string `"+200"` and is not equal to the number 200. A JSON
  number that does not fit serde_json's finite representation, such as `1e9999`,
  is still an error.
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
cannot recover its original values. Request execution is intentionally unchanged.

## Variables and disclosure

The evaluator reuses `resolve_environment_for_templates_with_provider` and
`ResolvedEnvironment::interpolate_strict` when an expected value contains `{{`.
The `RequestResolution` input supplies the same selected environment, inheritance,
invocation overrides, workspace identity, and provider context used for requests.
Disabled declarations shadow inherited values according to the existing resolver.
Only secrets reachable from an active binary assertion value that contains
interpolation are requested, including transitive references.
Each assertion resolves independently; provider values are not cached across
assertions, so callers should provide stable values for an evaluation batch.

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
