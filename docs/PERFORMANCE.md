# Performance Baseline

The repeatable performance suite covers bundled workspaces containing 100, 1,000,
and 10,000 requests. Each
deterministic fixture contains folders of 100 requests and representative headers,
query parameters, and JSON bodies.

## Time benchmarks

Run the release-mode Criterion suite:

```bash
cargo bench -p probe-cli --bench performance
```

The benchmark groups measure separate boundaries:

- `opencollection_parsing`: YAML decoding, validation, retained document creation,
  and projection into domain models.
- `workspace_construction`: construction of generational request and folder arenas
  from an already parsed domain collection. Fixture cloning is setup work and is not
  timed.
- `request_lookup`: in-memory lookup through a session-only `RequestKey`, cycling
  across the loaded workspace.
- `cli_startup/help`: operating-system process creation and Probe startup through
  rendering `probe --help`.
- `environment_resolution`: resolving a three-level `extends` chain of 10, 100, and
  500 variables. The leaf interpolates a parent value, so nested variable resolution
  is included.
- `environment_variable_status`: three `variable_status` lookups (a present value, a
  secret with no runtime value, and an absent name) on an already resolved
  environment.

The desktop classifies `{{name}}` placeholders with `probe_core::VariableStatus`
while painting a request. `ProbeApp` resolves the selected environment once at the
start of each frame and reuses that context for every variable-bearing field. The
memo is cleared before the frame returns, so event handlers and other calls outside
that render resolve the current selection instead of a previous frame. Secret
highlighting reads in-memory
[credential-presence metadata](ARCHITECTURE.md#presence-metadata) and never queries
the operating-system credential store. Opaque credential identities for the current
workspace, environment, and secret names are derived once, then reused for later
frames until that set changes.

Criterion stores machine-local reports under `target/criterion`. Compare results on
the same machine and build profile; absolute timings from different machines are not
directly comparable.

Criterion 0.7 remains pinned with the original baseline. The workspace minimum is
Rust 1.95 to match the exact GPUI revision. Production release artifacts use Rust
1.99.0; record the compiler version when comparing benchmarks, since it can affect
results. Handle dependency upgrades separately from performance measurement.

## Representative fixture files

Benchmarks generate fixtures in memory to avoid maintaining megabytes of repetitive
YAML. Generate the same deterministic files when an external profiler needs paths:

```bash
cargo run -p probe-cli --example generate_performance_fixtures -- \
  target/performance-fixtures
```

Generated files are named `workspace-100.yml`, `workspace-1000.yml`, and
`workspace-10000.yml`.

## Peak memory

Build Probe and generate the fixtures first:

```bash
cargo build --release -p probe-cli
cargo run -p probe-cli --example generate_performance_fixtures -- \
  target/performance-fixtures
```

On macOS, record peak resident memory while loading the 10,000-request workspace:

```bash
/usr/bin/time -l target/release/probe collection validate \
  target/performance-fixtures/workspace-10000.yml --quiet
```

Use the `maximum resident set size` line. On Linux, run the equivalent command with
`/usr/bin/time -v` and read `Maximum resident set size`. These measurements include
the process, YAML source retention, parsed document, locator index, and domain
workspace, matching the memory paid by a real workspace load.

## Baseline policy

Record the date, commit, operating system, CPU, Rust version, and Criterion estimates
when evaluating a change. Performance thresholds should only follow stable data
from representative machines.

## Initial reference run

The initial baseline was recorded on 2026-08-15 using an Apple M4 MacBook Pro
(10 cores, 16 GB), macOS 26.6.1, and rustc 1.97.1. Criterion point estimates from a
release build were:

| Measurement | 100 | 1,000 | 10,000 |
| --- | ---: | ---: | ---: |
| OpenCollection parsing | 1.97 ms | 22.90 ms | 219.30 ms |
| Workspace construction | 2.73 µs | 20.46 µs | 364.21 µs |
| Request lookup | 1.23 ns | 1.44 ns | 1.83 ns |

`probe --help` process startup measured 2.58 ms. Loading and validating the generated
10,000-request fixture peaked at 222,199,808 bytes resident memory (about 212 MiB).
These values are a local reference, not regression thresholds.

## LoadedWorkspace bookkeeping measurements (2026-10-09)

The 10k comparison uses baseline production revision
`eec40fe634a037277b46d6efd17e7ce4c320c1d6` and the optimization in this workspace,
with the same added measurement harness on both implementations. Hardware: Apple
M4 (10 logical cores, 16 GiB), macOS 26.7.1; compiler: Homebrew rustc 1.97.1
(`8bab26f4f`, LLVM 22.1.8), default Cargo release/bench profiles. These are local
measurements using the installed compiler, not the production release compiler.

### Repeatable timing commands

Run benchmarks serially, without concurrent compilation, tests, or profilers:

```bash
unset CARGO_TARGET_DIR
# Before the production change, with the new harness present:
cargo bench -p probe-cli --bench performance -- \
  '10000|repository_lookup' --save-baseline workspace-before
# After the production change:
cargo bench -p probe-cli --bench performance -- \
  '10000|repository_lookup' --baseline workspace-before

# Run on both implementations:
cargo test --release -p probe-opencollection loaded_workspace_construction_10k -- \
  --ignored --nocapture --test-threads=1
cargo test --release -p probe-desktop workspace_reconcile_10k -- \
  --ignored --nocapture --test-threads=1
```

`repository_lookup` cycles across all 10,000 requests and 100 folders, measuring
selector → key and key → selector separately. The original `request_lookup`
continues to measure domain arena lookup. `structural_preparation/10000` prepares
moving the last root folder to index zero on a file-backed workspace, including
capture of the original selectors and shared source baseline; it excludes execution
and filesystem writes. The temporary fixture is generated and loaded outside timing.

The ignored construction measurements avoid adding a public API solely for a
benchmark. They start from the parsed collection and prebuilt locator tree.
`loaded_workspace_construction/10000` includes domain arena construction and
repository indexing; `locator_index_construction/10000` starts from an already
constructed domain workspace. Collection/workspace cloning and destruction of the
result are outside timing. Locator projection from YAML is outside these measurements;
YAML parsing remains covered by Criterion.

The ignored reconciliation measurement runs the actual desktop reconciliation code
with every request clean and present. Normal reload uses the same fixture; shifted
reload inserts an empty folder at the root before the original folders, changing
every request's structural selector. Fresh loading happens outside timing. Each
ignored measurement has three warmups and 31 timed samples and prints minimum,
median, and p90; report medians. These are exploratory measurements, not CI timing
thresholds. Ignore runs with competing builds; one such reconciliation run was
excluded from this comparison.

### Timing results

Criterion rows report point estimates; ignored-test rows report medians.

| 10k fixture measurement | Before | After |
| --- | ---: | ---: |
| OpenCollection parsing | 234.71 ms | 224.98 ms |
| Domain workspace construction (existing Criterion group) | 1.4482 ms | 1.3870 ms |
| LoadedWorkspace construction | 1.754709 ms | 1.608292 ms |
| Locator index construction only | 648.709 µs | 498.083 µs |
| Domain request arena lookup | 2.4760 ns | 2.3508 ns |
| Request selector → key | 90.200 ns | 84.652 ns |
| Request key → selector | 36.286 ns | 2.3148 ns |
| Folder selector → key | 25.800 ns | 24.972 ns |
| Folder key → selector | 10.829 ns | 1.5113 ns |
| Normal reconcile | 10.992416 ms | 10.913083 ms |
| Shifted reconcile | 13.796667 ms | 14.035333 ms |
| Structural edit preparation | 207.30 µs | 208.41 µs |

LoadedWorkspace construction improves 8.3%, locator indexing 23.2%, and request
key → selector lookup about 15.7×. Parsing and domain construction code are unchanged;
their timing differences should not be attributed to this optimization. Criterion
reported no significant change in domain construction or structural preparation, and
arena lookup changed within its noise threshold. Shifted reconciliation's 1.7%
median increase is small compared with its sample spread: before min/median/p90
13.386/13.797/14.304 ms, after 13.445/14.035/14.867 ms. Normal reconciliation was
10.644/10.992/11.377 ms before and 10.671/10.913/11.128 ms after. No material
reconciliation or structural-preparation regression was observed.

### Peak process RSS results

Use the existing macOS command on an explicitly rebuilt production binary for each
implementation, after all compilation finishes:

```bash
unset CARGO_TARGET_DIR
cargo build --release -p probe-cli
cargo run -p probe-cli --example generate_performance_fixtures -- \
  target/performance-fixtures
for run in 1 2 3 4 5; do
  /usr/bin/time -l target/release/probe collection validate \
    target/performance-fixtures/workspace-10000.yml --quiet
done
```

Record `maximum resident set size` in **bytes**, and report the median of five fresh
processes. Fixture generation and compilation are not part of the measured process.
The original and optimized implementations were rebuilt serially with the same
`cargo build --release -p probe-cli` command for the final comparison.

| Maximum resident set size | Before | After |
| --- | ---: | ---: |
| Median bytes | 222,281,728 | 222,248,960 |
| Median MiB | 211.98 | 211.95 |
| Minimum bytes | 222,232,576 | 220,512,256 |
| Maximum bytes | 222,560,256 | 224,509,952 |

Raw before samples: 222232576, 222560256, 222298112, 222281728, 222248960.
Raw after samples: 224509952, 224444416, 220512256, 222248960, 220577792.
Peak RSS is effectively unchanged: the 32 KiB median difference is much smaller
than the run-to-run variation. This change reduces retained locator bookkeeping
rather than the earlier parsing peak; do not claim a peak-RSS improvement. Preliminary
runs using the benchmark-produced binary also fluctuated by several MiB and were
superseded by the explicitly rebuilt comparison above.

### Retained heap methodology

```bash
cargo run --release -p probe-cli --example workspace_memory
```

This example alone installs the dev-only `dhat` allocator. It generates the fixture
before profiling, constructs and drops a domain `Workspace`, then loads the same
source into a `LoadedWorkspace`. It records live requested allocation bytes after
each completed construction, excluding the shared input source. Their difference
isolates retained repository locator/index bookkeeping, not parsing temporaries or
request payloads. Both loads are in memory, so file persistence locators and retained
source bytes are excluded. This is a focused ownership measurement, not malloc size
classes, process RSS, or a file-backed workspace's complete memory cost. DHAT's
backtrace instrumentation makes it much slower than a normal load; do not use this
example for timing or RSS. No production allocator or profiling dependency changes.

| Retained allocation bytes | Before | After |
| --- | ---: | ---: |
| Domain workspace | 19,050,875 | 19,050,875 |
| Loaded workspace | 21,666,463 | 21,156,523 |
| Locator/index bookkeeping (difference) | 2,615,588 | 2,105,648 |

Bookkeeping saves 509,940 bytes (498 KiB, 19.5%); total retained in-memory workspace
allocations save 2.35%. Measurements use requested allocation sizes from the same
allocator and compiler; they should be deterministic on repeated runs with those
inputs.

### Design and trade-offs

Each selector previously had separate owned `String` allocations in its located
record and its `BTreeMap` entry. These now share one `Arc<str>` allocation. Ordered
selector indexes remain; there is no global interner or hash-table substitution.
Sharing adds atomic reference counting and an Arc allocation header, but removes
duplicated string ownership and reduces the size of each selector handle.

The two key → index trees repeated full keys already retained in the located
records. Fresh arena insertion and locator traversal have identical order, so key
lookup now indexes the located vector by slot and compares the **entire** stored key,
including workspace and slot generations. Loader assertions enforce that ordering.
Repository structural edits reload these indexes; detached drafts occupy subsequent
arena slots and never enter the located vectors. Foreign keys, detached keys, and
reused detached slots do not acquire a repository selector. Tests cover both older
and newer foreign workspace generations, nested and unbundled traversal, selector
round trips, draft slot reuse, and shared selector allocation ownership.

Temporary locator trees are released after construction. Persistence item paths,
source baselines, and domain ancestor information remain because they support writes,
external-change detection, and hierarchy semantics rather than equivalent redundant
lookups. Parsing, response retention, and desktop editor state are unchanged.

Validation: formatting, Clippy with warnings denied, all-target/all-feature workspace
tests, and dependency advisory checks passed. Coverage floors passed (OpenCollection
87.94% production line coverage); the changed functions' CRAP scores were below 7,
with request/folder key → selector lookup at 1 and 100% line coverage. Diff-scoped
mutation testing completed with 17 caught mutations, 3 unviable, and no survivors.
