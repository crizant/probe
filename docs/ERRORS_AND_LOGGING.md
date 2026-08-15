# Errors and Logging

Probe keeps errors structured until they reach an interface boundary.

- Library crates define typed errors for their own operations when those operations
  are introduced.
- The core/application layer coordinates errors without depending on CLI wording or
  desktop presentation types.
- The CLI maps error categories to stable exit codes. Human diagnostics go to
  stderr; command output, including future JSON output, goes to stdout.
- The desktop adapter presents the same structured errors without reimplementing
  their meaning.
- Libraries do not initialize global logging. Interfaces will configure logging and
  send diagnostics to stderr or an appropriate desktop sink.

Phase 0 does not add an error or logging dependency because no fallible domain,
repository, network, or persistence operations exist yet. The first phase that needs
these capabilities should select lightweight, maintained crates at the boundary where
they are required.

