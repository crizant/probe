#!/bin/sh
set -eu

usage() {
  echo "Usage: scripts/run-mutants.sh [--in-place] <diff-file>" >&2
}

mode=scratch
if [ "${1:-}" = "--in-place" ]; then
  mode=in-place
  shift
fi

if [ "$#" -ne 1 ] || [ -z "$1" ]; then
  usage
  exit 2
fi

diff_file=$1
mutant_jobs=2
if [ "$mode" = "in-place" ]; then
  mutant_jobs=1
  # Reuse incremental artifacts in a quiet local checkout.
  CARGO_INCREMENTAL=${CARGO_INCREMENTAL:-1}
  export CARGO_INCREMENTAL
fi

if [ -n "${MUTANTS_JOBSERVER_TASKS:-}" ]; then
  case "$MUTANTS_JOBSERVER_TASKS" in
    *[!0-9]* | '')
      echo "error: MUTANTS_JOBSERVER_TASKS must be a positive integer" >&2
      exit 2
      ;;
  esac
  tasks=$MUTANTS_JOBSERVER_TASKS
  if [ "$tasks" -lt 1 ]; then
    echo "error: MUTANTS_JOBSERVER_TASKS must be at least 1" >&2
    exit 2
  fi
else
  cpus=
  if command -v nproc >/dev/null 2>&1; then
    cpus=$(nproc 2>/dev/null || true)
  fi
  if [ -z "$cpus" ] && command -v getconf >/dev/null 2>&1; then
    cpus=$(getconf _NPROCESSORS_ONLN 2>/dev/null || true)
  fi
  if [ -z "$cpus" ] && command -v sysctl >/dev/null 2>&1; then
    cpus=$(sysctl -n hw.logicalcpu 2>/dev/null || true)
  fi
  case "$cpus" in
    '' | *[!0-9]*)
      echo "error: could not detect the number of logical CPUs (set MUTANTS_JOBSERVER_TASKS to override)" >&2
      exit 1
      ;;
  esac
  if [ "$cpus" -lt 1 ]; then
    echo "error: CPU detection returned an invalid count: $cpus" >&2
    exit 1
  fi
  tasks=$((cpus / 2))
  if [ "$tasks" -lt 1 ]; then
    tasks=1
  fi
fi

if [ -n "${MUTANTS_TEST_THREADS:-}" ]; then
  case "$MUTANTS_TEST_THREADS" in
    *[!0-9]* | '')
      echo "error: MUTANTS_TEST_THREADS must be a positive integer" >&2
      exit 2
      ;;
  esac
  test_threads=$MUTANTS_TEST_THREADS
  if [ "$test_threads" -lt 1 ]; then
    echo "error: MUTANTS_TEST_THREADS must be at least 1" >&2
    exit 2
  fi
else
  # Keep the existing test-thread budget in both modes.
  test_threads=$((tasks / 2))
  if [ "$test_threads" -lt 1 ]; then
    test_threads=1
  fi
fi

echo "Mutation resources: mode=$mode jobs=$mutant_jobs jobserver_tasks=$tasks test_threads=$test_threads"
started=$(date +%s)
report_duration() {
  status=$?
  echo "Mutation gate duration: $(($(date +%s) - started))s (exit $status)"
  exit "$status"
}
trap report_duration 0

if [ "$mode" = "in-place" ]; then
  # cargo-mutants 27.1.0 rejects --in-place combined with any -j value.
  set -- --in-place
else
  set -- -j "$mutant_jobs"
fi

cargo mutants --workspace --in-diff "$diff_file" "$@" \
  --jobserver-tasks "$tasks" --caught -- -- --test-threads "$test_threads"
