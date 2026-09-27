#!/bin/sh
set -eu

usage() {
  echo "Usage: scripts/run-mutants.sh <diff-file>" >&2
}

if [ "$#" -ne 1 ] || [ -z "$1" ]; then
  usage
  exit 2
fi

diff_file=$1

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

exec cargo mutants --workspace --in-diff "$diff_file" -j 2 --jobserver-tasks "$tasks"
