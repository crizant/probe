# Errors and Logging

Probe keeps errors structured until they reach an interface boundary.

- Library crates define typed errors for their own operations when those operations
  are introduced.
- The core/application layer coordinates errors without depending on CLI wording or
  desktop presentation types.
- The CLI maps error categories to stable exit codes. Human diagnostics go to
  stderr; versioned JSON command output goes to stdout.
- The desktop adapter presents the same structured errors without reimplementing
  their meaning.
- Libraries do not initialize global logging. Interfaces will configure logging and
  send diagnostics to stderr or an appropriate desktop sink.

Repository loading and saving, environment resolution, and HTTP execution expose typed
library errors. Persistence distinguishes stale-source conflicts, read-only in-memory
sources, invalid retained documents, serialization failures, and filesystem failures.
Interfaces map these types without requiring callers to parse diagnostic messages.

Desktop user configuration failures stay in `probe-desktop`. A missing file is
the default configuration. A read or parse failure, including an unrecognized
`theme` value, includes the config path and is shown in a persistent error
toast after the window opens. A path that is not a regular file, and a regular
file larger than 64 KiB, are the same kind of visible configuration error. The
window still uses the default configuration. The location is in
[Architecture](ARCHITECTURE.md#user-configuration).
