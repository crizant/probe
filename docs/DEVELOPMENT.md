# Development Practices

Read this for Rust, GPUI, dependencies, tests, and validation. Project invariants
live in [AGENTS.md](../AGENTS.md); release packaging is in [PACKAGING.md](PACKAGING.md).

## Rust Toolchains

`rust-toolchain.toml` selects Rust 1.95.0, the workspace MSRV. CI and release
validation run formatting, Clippy, and tests on that version across supported
platforms. Production artifacts use Rust 1.99.0, pinned by `RUSTUP_TOOLCHAIN` in
[release.yml](../.github/workflows/release.yml); this does not raise the MSRV.
Keep versions explicit and toolchain changes separate from GPUI/dependency upgrades.

## Rust and Async Work

- Prefer normal ownership, GPUI entities, message passing, task results, and immutable
  shared data before `Arc`, `Mutex`, `RwLock`, `RefCell`, or global mutable state.
- Keep shared networking and application APIs asynchronous where the desktop needs
  them; avoid duplicate synchronous CLI logic.
- Keep filesystem I/O, HTTP, expensive parsing, Git work, and highlighting off the
  GPUI thread. Any unsafe code requires explicit justification.

## Dependencies

Check the standard library and existing dependencies before adding a crate. Prefer
maintained, cross-platform crates; avoid large dependencies for trivial work and
replacements unrelated to the task.

Use `gpui-kit` with `default-features = false`, importing through `gpui_kit` and
its unstyled `gpui_kit::base` layer. Do not enable the pre-styled `component` layer
or copy its APIs unless explicitly scoped. Initialize the theme once through
`crate::theme::Theme::init(cx)` in `probe-desktop`.

Kit coordinates GPUI and base versions. Keep its resolved version in `Cargo.lock`;
do not add independent `gpui`, `gpui-base`, `gpui-component`, or platform dependencies,
or upgrade Kit during unrelated work. Inspect the resolved source and examples for
unfamiliar APIs. Introducing another GUI framework requires explicit approval.

## Tests

- Keep shared fixtures in `tests/fixtures/`. CLI integration tests cover commands,
  JSON, and exit codes. Linux terminal-width tests require `script` and `stty`.
- Inspect existing tests first. Extend them when setup and behavior match; add a
  separate test when distinct setup or purpose makes it clearer.
- Account for CRLF in text fixtures unless the format requires or normalizes LF.
- Test state machines and pure rules at their owner. Use desktop tests for wiring,
  focus, dialogs, pane constraints, and cross-component behavior; share setup helpers.
- Assert semantic actions, dynamic values, and safety-critical wording rather than
  whole paragraphs. Tests at different boundaries may protect different contracts.
- Review visual constants in light and dark against [DESIGN.md](DESIGN.md), rather
  than automating assertions about spacing, typography, radii, palettes, or contrast.
- Add property tests or fuzzing only for a concrete input-space risk, such as
  resolution, round trips, or untrusted import parsers.

## Completion Checks

Run `cargo fmt --check` for all Rust changes, then select local checks by scope:

| Change | Required local validation |
| --- | --- |
| Isolated Rust change | Formatting, affected-crate Clippy, regression and related tests; visual review for UI changes |
| Shared behavior, persistence, cross-crate APIs, or uncertain impact | Full workspace tests; Clippy for affected crates and consumers |
| Dependencies or `Cargo.lock` | Full workspace tests, affected-consumer Clippy, and advisory check |
| Documentation only | Review diff, links, and command examples; no Cargo builds/tests |
| Scripts or CI | Checks relevant to the changed files |

For an isolated desktop change, replace `components::` with the relevant test or
module; expand to additional filters or the affected crate's full suite as needed:

```bash
unset CARGO_TARGET_DIR
cargo fmt --check
cargo clippy -p probe-desktop --all-targets --all-features -- -D warnings
cargo test -p probe-desktop --all-features 'components::'
```

CI runs full workspace formatting, Clippy, tests, advisories, and the quality gates
below. Isolated changes may leave full gates to CI. To run the broader checks locally:

```bash
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo deny check advisories
```

Use the project `target/` with `CARGO_TARGET_DIR` unset. Run outside the Cursor
sandbox: its cache cannot compile `gpui_macos` (Metal) or `aws-lc-sys`, and tests
may need local server sockets. Run Cargo builds/tests sequentially per checkout.
Reuse successful checks unless relevant changes or failures justify rerunning them.
Report local results and gates left to CI; resolve required failures and wait for
required CI checks before merging. Stop when the requested work and checks pass.

## Final Quality Checks

[Rust-TOPS](https://github.com/Hedronite/rust-tops/blob/v0.1.3/RUST_TOPS.md) is the
review protocol; [rust-tops.yaml](../rust-tops.yaml) describes Probe's gates.
[CI](../.github/workflows/ci.yml) owns full coverage, CRAP, advisories, and applicable
mutation checks. Local quality reruns are for failures, coverage gaps, or higher-risk
changes; coverage also runs workspace tests. Use CI's pinned tool versions when
installing tools. Use these commands and scripts rather than `cargo tops gate`,
which duplicates checks and does not enforce Probe's configured thresholds.

### Coverage and CRAP

With `CARGO_TARGET_DIR` unset and `llvm-tools-preview` installed:

```bash
cargo llvm-cov --workspace --all-features --json --output-path target/coverage.json
cargo llvm-cov report --lcov --output-path target/coverage.lcov
python3 scripts/check-coverage.py target/coverage.json target/coverage.lcov
cargo crap --workspace --lcov target/coverage.lcov --format json --output target/crap.json
test -s target/crap.json
```

CI enforces production line-coverage floors for behavior crates, excluding inline
`#[cfg(test)]` modules. Desktop and raw region coverage are reported without floors.
Inspect changed functions' CRAP scores above 30; the score signals review, not a
reason to split functions or add tautological tests. Existing high scores alone
are not failures; CI requires a nonempty report.

If Homebrew Rust cannot find `llvm-tools-preview`, set the following before running
the same coverage commands. Xcode tools must be able to read the compiler's data;
compare `rustc -vV` and the tools' `--version` output if reporting fails.

```bash
export LLVM_COV="$(xcrun --find llvm-cov)"
export LLVM_PROFDATA="$(xcrun --find llvm-profdata)"
```

### Mutation Review

CI runs diff-scoped mutation testing on PRs changing Rust source in
`crates/{application,cli,core,http,websocket,opencollection,postman,yaak}/src`.
For a local investigation, include all changed production lines in the diff. Use
`git add -N path/to/new.rs` for new files, then:

```bash
git diff --unified=0 HEAD > target/mutants.diff
scripts/run-mutants.sh target/mutants.diff
```

For committed PR changes, use `git diff --unified=0 origin/main...HEAD` instead.
The wrapper limits build/test concurrency; `MUTANTS_JOBSERVER_TASKS` and
`MUTANTS_TEST_THREADS` override its budgets. Reuse a warm target when available.

For a small diff in a quiet checkout, use:

```bash
scripts/run-mutants.sh --in-place target/mutants.diff
```

This reuses incremental artifacts and runs mutants serially.
It temporarily changes source: do not edit, run watchers, or run other Cargo
commands there during the check. Prefer a dedicated checkout, verify restoration
after interruption, and rebuild if needed. Default scratch mode keeps the checkout
editable. Desktop mutation testing remains local and selective.

Investigate survivors. Add tests for missing behavior assertions; document equivalent
or irrelevant mutants in the review. CI uploads quality and mutation reports for
investigation. Describe architectural decisions and remaining limitations at handoff.
