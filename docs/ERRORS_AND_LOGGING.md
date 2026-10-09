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

Desktop user configuration is separate from those library errors. `probe-desktop`
defines the failure for a missing home or config directory variable, a file that
cannot be read, and TOML that cannot be parsed. The read happens off the UI
thread. A missing file is the default configuration, not an error. Read and
parse failures include the config path and are shown in a persistent desktop
error toast. Location rules are in [Architecture](ARCHITECTURE.md#user-configuration).
