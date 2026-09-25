# Development Practices

Read this document for implementation workflow, dependency changes, GPUI work, or
test design. Project-wide architectural invariants remain in `AGENTS.md`.

## Rust and Async Work

- Prefer normal ownership, GPUI entity ownership, message passing, task results, and
  immutable shared data before `Arc`, `Mutex`, `RwLock`, `RefCell`, or global mutable
  state.
- Keep shared networking and application APIs asynchronous where the desktop needs
  them. Do not create duplicate synchronous business logic for the CLI.
- Run filesystem I/O, HTTP, large YAML or JSON parsing, Git work, and expensive
  highlighting away from the GPUI thread.
- Never use `unsafe` solely to bypass ownership problems. Any unsafe code requires
  explicit justification.

## Dependencies

Before adding a crate, check the standard library and existing dependencies. Prefer
actively maintained, cross-platform crates and avoid large dependencies for trivial
work. Do not replace dependencies without a task-specific reason.

Probe uses Longbridge `gpui-base` (`gpui-base` / `gpui_base`, from
`longbridge/gpui-component`'s `crates/base`). Never add the separate, pre-styled
`gpui-component` crate or copy its APIs. Initialize the theme once through
`probe_desktop::theme::Theme::init(cx)`.

Keep GPUI and `gpui-base` on compatible pinned sources. If their types conflict,
inspect the pinned `gpui-base` lockfile and correct the GPUI pin rather than mixing
revisions. Do not upgrade either dependency during unrelated work. Inspect the exact
pinned source and examples before using unfamiliar APIs.

Do not introduce Electron, Tauri, WebView, React, Flutter, or another GUI framework
without explicit approval.

## Tests

Keep shared fixtures under `tests/fixtures/`. CLI integration tests cover command
behavior, JSON output, and exit codes.

Do not automate visual constants such as spacing, radii, typography sizes, palette
values, or contrast ratios. Review those visually against [DESIGN.md](DESIGN.md).
Desktop tests may cover behavior such as appearance selection, pane constraints,
focus, and highlight ranges. Extend an existing desktop test when it already builds
the same surface and the new assertion is a follow-on interaction.

Before completing a code change, run:

```bash
unset CARGO_TARGET_DIR
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo deny check advisories
```

Run those cargo commands outside the Cursor sandbox (`required_permissions: ["all"]`)
and do not inherit the sandbox `CARGO_TARGET_DIR`. That cache cannot compile
`gpui_macos` (Metal toolchain) or `aws-lc-sys`. Use the project `target/` directory.

Do not report completion while any required check fails.

## Final quality checks

Probe uses [Rust-TOPS 1.0.0](https://github.com/Hedronite/rust-tops/blob/v0.1.3/RUST_TOPS.md)
as a risk-review protocol. `rust-tops.yaml` describes this workspace; the
commands below and CI execute its Probe-specific gates. Run these after changes
to Rust production behavior. Use the project `target/` directory and unset any
inherited `CARGO_TARGET_DIR`, especially for macOS GPUI/Metal builds.

```bash
unset CARGO_TARGET_DIR
cargo llvm-cov --workspace --all-features --json --output-path target/coverage.json
python3 scripts/check-coverage.py target/coverage.json
cargo llvm-cov report --lcov --output-path target/coverage.lcov
cargo crap --workspace --lcov target/coverage.lcov --format json --output target/crap.json
test -s target/crap.json
```

The coverage job is CI-blocking. It checks each behavior crate's line coverage
against a floor set roughly two to three points below its measured baseline.
It reports region coverage and desktop coverage without setting floors for them. This
keeps UI rendering and visual constants from driving test design. Inspect the
CRAP report for newly added or changed functions and investigate scores above
30. The report must be nonempty in CI; the score is a review signal, not an
automatic reason to split code or add a tautological test. Existing high scores
are not a blanket failure. The largest risks are untested error/adapter paths
and a few complex desktop interaction handlers.

For a change with production lines in `crates/{cli,core,http,opencollection,postman,yaak}/src`,
run diff-scoped mutation testing. The diff file must include the changed
production lines. For uncommitted local changes:

```bash
git diff --unified=0 HEAD > target/mutants.diff
cargo mutants --workspace --in-diff target/mutants.diff -j 2
```

For a PR branch, use `git diff --unified=0 origin/main...HEAD` to include
committed changes. CI runs this gate only on PRs that change Rust source in the
six behavior crates. A survivor needs investigation; add a test when a
documented behavior or regression has no assertion, and record the reason for
an equivalent or irrelevant mutant in the change review. Desktop mutation
testing is local and selective because GPUI compilation and render glue make a
workspace-wide PR mutation run costly. Property tests fit pure resolution and
round-trip contracts; fuzzing fits untrusted OpenCollection, Postman, and Yaak
parsers. Add either only when a concrete input-space risk calls for it.

Install the CI tool versions when needed: `cargo-llvm-cov 0.9.1`, `cargo-crap
0.5.0`, `cargo-mutants 27.1.0`, and `cargo-deny 0.20.2`. Use `llvm-tools-preview`
with the selected Rust toolchain for LLVM coverage. `cargo tops check` can
check the protocol file's basic typed fields, but `cargo tops gate` 0.1.3
hardcodes nextest, `origin/main`, global `target/` paths, and runs duplicate
checks; it also does not enforce coverage or CRAP thresholds from the YAML.
Use the commands above and Probe's CI instead.

## Working Style

- Inspect relevant architecture and pinned dependency source instead of guessing.
- Preserve unrelated user changes in a dirty worktree.
- Make the smallest coherent change and add or update tests.
- Summarize architectural decisions and remaining limitations.
