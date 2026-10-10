# Development Practices

Read this document for implementation workflow, dependency changes, GPUI work, or
test design. Project-wide architectural invariants remain in `AGENTS.md`.

## Rust Toolchains

The workspace declares Rust 1.95 as its minimum supported Rust version (MSRV).
`rust-toolchain.toml` selects 1.95.0 for local development, and CI checks formatting,
Clippy, and tests on 1.95.0 across Linux, macOS, and Windows. The other CI checks
also use 1.95.0. Release validation runs the same formatting, Clippy, and test checks
on 1.95.0 before building artifacts.

Production CLI and desktop release artifacts use explicitly pinned Rust 1.99.0.
The release workflow selects that compiler and each platform target in one setup
step per build job. Each build job sets `RUSTUP_TOOLCHAIN` to override the
repository's MSRV toolchain file for Cargo and its subprocesses, and verifies the
active compiler before caching or building. This build compiler is independent of
the MSRV; using it does not raise the minimum compiler required to build Probe from
source. Keep both versions explicit rather than selecting floating `stable`, and
keep GPUI and dependency upgrades separate from toolchain changes.

## macOS Packaging Tools

Native app icon packaging requires full Xcode 26 or later (`actool` and
`assetutil`), Python 3.9 or later, and cargo-bundle 0.11.0. Compile the icon after
bundling and before signing; see the [app-icon workflow](../crates/desktop/assets/app-icon/README.md).
CairoSVG, Pillow, and system Cairo are needed only when regenerating artwork.
Windows and Linux packaging continue to use the existing generated icons.

## Linux RPM Packages

Tagged releases publish upstream binary RPMs next to the Linux `.tar.gz`
archives. Packaging reuses the x86_64 binaries already built for those archives.
`scripts/build-rpm.sh` reads the version from `Cargo.toml`. A prerelease such
as `0.11.0-beta.1` uses `~` in the RPM `Version` field (`0.11.0~beta.1`).
This is not a mock or COPR source build: Probe's GPUI dependencies are git
crates, and the spec does not compile them. `scripts/test-rpm.sh` checks
packaging with stand-in binaries.
`scripts/build-rpm.sh` runs `scripts/verify-rpm.sh` before copying packages
into the release output.

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

Before writing a new test, inspect the existing tests for the affected behavior.
Prefer extending an existing test with focused assertions or additional cases when
it already exercises the relevant setup and contract. Add a separate test when the
behavior needs distinct setup or extending an existing test would obscure its purpose.

When tests inspect or compare text files, account for platform line endings. Windows
may check out fixtures with CRLF (`\r\n`), so avoid assuming LF (`\n`) unless the
test explicitly normalizes line endings or the format requires them.

Do not automate visual constants such as spacing, radii, typography sizes, palette
values, or contrast ratios. Review those visually against [DESIGN.md](DESIGN.md).
Desktop tests may cover behavior such as appearance selection, pane constraints,
focus, and highlight ranges. Extend an existing desktop test when it already builds
the same surface and the new assertion is a follow-on interaction.

Test state machines and pure rules at the type that owns them, and keep desktop tests
for wiring, focus, dialogs, and cross-component contracts. Share window, workspace,
and fake-store setup through test helpers instead of repeating it. Assert semantic
actions, dynamic values, and safety-critical wording in UI copy rather than whole
paragraphs. Similar tests at different boundaries, such as core, repository, and CLI
integration tests, may protect different contracts.

## Completion Checks

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

## Final Quality Checks

Probe uses [Rust-TOPS 1.0.0](https://github.com/Hedronite/rust-tops/blob/v0.1.3/RUST_TOPS.md)
as a risk-review protocol. `rust-tops.yaml` describes this workspace; the
commands below and CI execute its Probe-specific gates. Run these after changes
to Rust production behavior. Use the project `target/` directory and unset any
inherited `CARGO_TARGET_DIR`, especially for macOS GPUI/Metal builds.

```bash
unset CARGO_TARGET_DIR
cargo llvm-cov --workspace --all-features --json --output-path target/coverage.json
cargo llvm-cov report --lcov --output-path target/coverage.lcov
python3 scripts/check-coverage.py target/coverage.json target/coverage.lcov
cargo crap --workspace --lcov target/coverage.lcov --format json --output target/crap.json
test -s target/crap.json
```

The coverage job is CI-blocking. It checks each behavior crate's production
line coverage against a floor set below its measured baseline. Inline
`#[cfg(test)]` modules are excluded from these floors. It reports raw region
coverage and desktop coverage without setting floors for them. This keeps UI
rendering and visual constants from driving test design. Inspect the
CRAP report for newly added or changed functions and investigate scores above
30. The report must be nonempty in CI; the score is a review signal, not an
automatic reason to split code or add a tautological test. Existing high scores
are not a blanket failure. The largest risks are untested error/adapter paths
and a few complex desktop interaction handlers.

For a change with production lines in `crates/{application,cli,core,http,websocket,opencollection,postman,yaak}/src`,
run diff-scoped mutation testing. The diff file must include the changed
production lines. For uncommitted local changes, first run
`git add -N path/to/new.rs` for each new production file so it appears in the
diff. Then:

```bash
git diff --unified=0 HEAD > target/mutants.diff
scripts/run-mutants.sh target/mutants.diff
```

By default, the wrapper gives cargo-mutants' jobserver half of the detected logical
CPUs for Cargo/rustc builds, then divides that budget across its two mutant
jobs for Rust test-framework threads (both counts have a minimum of one).
Set `MUTANTS_JOBSERVER_TASKS` or `MUTANTS_TEST_THREADS` to override either
count. These conservative concurrency limits reduce sustained CPU saturation,
heat, and fan noise during local mutation runs; they are not a hard CPU cap.

For small local diffs in a quiet checkout with an already-warm `target/`, use:

```bash
scripts/run-mutants.sh --in-place target/mutants.diff
```

This keeps baseline verification, mutant selection, and the same compiler and
test-thread budgets. It enables incremental compilation by default while honoring
an explicit `CARGO_INCREMENTAL` value. cargo-mutants 27.1.0 requires serial mutant
execution in this mode, so the wrapper omits `-j`. Source files are temporarily
mutated: do not edit or run watchers or other Cargo commands in that checkout
during the gate. Prefer a dedicated checkout with its own persistent target, and
check source restoration after an interrupted run. The restored source may need
a rebuild after the last mutant. Use the default scratch mode when the checkout
must remain editable or when parallel mutants benefit a larger diff.

Reuse a target already warmed by ordinary development. Building a cache solely
for one gate does not provide the same benefit.

CI uses the same wrapper and resource limits, with incremental compilation
explicitly enabled for repeated builds. The wrapper reports resource counts,
caught-mutant timings, and total gate duration. CI uploads `mutants.out/` and
PR diff inputs even on failure. Superseded CI runs are cancelled only within
the same PR; main runs are independent. Cargo downloads remain cached, while
mutant builds retain separate temporary target directories.

For a PR branch, use `git diff --unified=0 origin/main...HEAD` to include
committed changes. CI runs this gate only on PRs that change Rust source in the
eight behavior crates. A survivor needs investigation: it may expose a missing
behavior assertion, an equivalent change, platform glue, or a design problem. Add
a test when a documented behavior or regression has no assertion, and record the
reason for an equivalent or irrelevant mutant in the change review. Desktop mutation
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

On macOS with a Homebrew Rust toolchain, `rustup` and `llvm-tools-preview` may
be unavailable. If `cargo llvm-cov` reports `failed to find llvm-tools-preview`,
check that Xcode's `llvm-cov` and `llvm-profdata` are compatible with the LLVM
version used by `rustc`:

```bash
rustc -vV
"$(xcrun --find llvm-cov)" --version
"$(xcrun --find llvm-profdata)" --version
```

Then use the Xcode tools for the coverage commands:

```bash
unset CARGO_TARGET_DIR
export LLVM_COV="$(xcrun --find llvm-cov)"
export LLVM_PROFDATA="$(xcrun --find llvm-profdata)"
cargo llvm-cov --workspace --all-features --json --output-path target/coverage.json
cargo llvm-cov report --lcov --output-path target/coverage.lcov
python3 scripts/check-coverage.py target/coverage.json target/coverage.lcov
cargo crap --workspace --lcov target/coverage.lcov --format json --output target/crap.json
test -s target/crap.json
```

These commands passed with Homebrew `rustc` using LLVM 22.1.8 and Xcode 26.6.0's
Apple LLVM 21.0.0 tools. Version numbers need not match exactly, but the tools
must be able to read the generated coverage data. Run the commands outside the
sandbox when tests need to bind local mock HTTP servers.

## Working Style

Scope and worktree rules are in `AGENTS.md`. When reporting a change, summarize
architectural decisions and remaining limitations.
