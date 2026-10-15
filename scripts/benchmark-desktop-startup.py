#!/usr/bin/env python3
"""Measure real GPUI startup with an opt-in release binary; requires a GUI session."""
import argparse
import json
import hashlib
import math
import os
from pathlib import Path
import platform
import queue
import statistics
import sys
import subprocess
import tempfile
import threading
import time

PREFIX = "PROBE_STARTUP "


def sample(binary, workspace, timeout):
    with tempfile.TemporaryDirectory(prefix="probe-startup-") as temporary:
        directory = Path(temporary)
        state = {"schema_version": 2}
        if workspace:
            path = str(workspace)
            selector = "items/0/items/0"
            state.update(active_collection=path, recent_collections=[path], workspaces={
                path: {"open_tabs": [selector], "active_tab": selector}
            })
        (directory / "desktop-session.json").write_text(json.dumps(state))
        events = queue.Queue()
        with tempfile.TemporaryFile(mode="w+", encoding="utf-8", errors="replace") as stderr:
            started = time.perf_counter()
            process = subprocess.Popen(
                [str(binary)], stdout=subprocess.PIPE, stderr=stderr, text=True,
                encoding="utf-8", errors="replace",
                env={**os.environ, "PROBE_STARTUP_BENCHMARK_DIR": temporary},
            )

            def read():
                for line in process.stdout:
                    if line.startswith(PREFIX):
                        events.put((time.perf_counter(), line[len(PREFIX):]))
                events.put((time.perf_counter(), None))

            reader = threading.Thread(target=read, daemon=True)
            reader.start()
            result = {}
            try:
                deadline = started + timeout
                while len(result) < 2:
                    received, raw = events.get(timeout=max(0, deadline - time.perf_counter()))
                    if raw is None:
                        raise RuntimeError("desktop exited before both startup milestones")
                    event = json.loads(raw)
                    if "error" in event:
                        raise RuntimeError(event["error"])
                    milestone = event["milestone"]
                    if milestone not in ("first_frame", "session_ready") or milestone in result:
                        raise RuntimeError(f"invalid startup milestone: {milestone}")
                    elapsed = event["main_ms"]
                    if not isinstance(elapsed, (int, float)) or not math.isfinite(elapsed) or elapsed < 0:
                        raise RuntimeError("invalid startup duration")
                    result[milestone] = {
                        "main_ms": elapsed,
                        "launch_observed_ms": (received - started) * 1000,
                    }
                process.wait(timeout=max(0.001, deadline - time.perf_counter()))
                if process.returncode:
                    raise RuntimeError(f"desktop exited with status {process.returncode}")
                return result
            except (queue.Empty, subprocess.TimeoutExpired) as error:
                raise RuntimeError(f"startup timed out after {timeout}s") from error
            finally:
                if process.poll() is None:
                    process.kill()
                    process.wait()
                reader.join(timeout=1)
                process.stdout.close()
                stderr.seek(0)
                diagnostics = stderr.read()
                if diagnostics and len(result) < 2:
                    print(diagnostics, file=sys.stderr)


def summarize(samples):
    summary = {}
    for milestone, metrics in samples[0].items():
        summary[milestone] = {}
        for metric in metrics:
            values = sorted(sample[milestone][metric] for sample in samples)
            summary[milestone][metric] = {
                "median_ms": statistics.median(values),
                "p90_ms": values[math.ceil(len(values) * 0.9) - 1],
            }
    return summary


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path("target/release") /
                        ("probe-desktop.exe" if os.name == "nt" else "probe-desktop"))
    parser.add_argument("--workspace", type=Path, action="append", default=[],
                        help="generated bundled fixture; repeat for multiple sizes")
    parser.add_argument("--runs", type=int, default=30)
    parser.add_argument("--warmups", type=int, default=3)
    parser.add_argument("--timeout", type=float, default=30)
    parser.add_argument("--output", type=Path, default=Path("target/desktop-startup.json"))
    args = parser.parse_args()
    if args.runs < 1 or args.warmups < 0 or not math.isfinite(args.timeout) or args.timeout <= 0:
        parser.error("runs must be positive, warmups nonnegative, and timeout finite and positive")
    binary = args.binary.resolve(strict=True)
    workspaces = [path.resolve(strict=True) for path in args.workspace]
    report = {"schema_version": 1, "platform": platform.platform(),
              "machine": platform.machine(), "binary": str(binary),
              "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
              "warmups": args.warmups, "runs": args.runs, "scenarios": []}
    for workspace in [None, *workspaces]:
        name = str(workspace) if workspace else "empty"
        print(f"Measuring {name}", file=sys.stderr)
        for _ in range(args.warmups):
            sample(binary, workspace, args.timeout)
        samples = [sample(binary, workspace, args.timeout) for _ in range(args.runs)]
        report["scenarios"].append({"workspace": name, "summary": summarize(samples), "samples": samples})
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
