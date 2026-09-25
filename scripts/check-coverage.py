#!/usr/bin/env python3
"""Check line coverage floors for Probe's behavior crates from llvm-cov JSON."""

import json
import sys
from pathlib import Path


# Baseline measured on 2026-09-26 with all workspace targets and features.
# Floors leave roughly two to three percentage points of measurement headroom. Desktop
# rendering is reported separately because a line floor rewards UI glue tests.
LINE_FLOORS = {
    "cli": 88.0,
    "core": 90.0,
    "http": 82.0,
    "opencollection": 82.0,
    "postman": 70.0,
    "yaak": 67.0,
}


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: check-coverage.py llvm-cov.json", file=sys.stderr)
        return 2
    data = json.loads(Path(sys.argv[1]).read_text())
    files = data["data"][0]["files"]
    counts = {name: [0, 0, 0, 0] for name in (*LINE_FLOORS, "desktop")}
    for item in files:
        parts = Path(item["filename"]).parts
        if "crates" not in parts:
            continue
        position = parts.index("crates")
        if len(parts) <= position + 2 or parts[position + 2] != "src":
            continue
        crate = parts[position + 1]
        if crate not in counts:
            continue
        summary = item["summary"]
        current = counts[crate]
        for offset, kind in ((0, "lines"), (2, "regions")):
            current[offset] += summary[kind]["covered"]
            current[offset + 1] += summary[kind]["count"]

    failed = False
    for crate, (lines, total_lines, regions, total_regions) in counts.items():
        if total_lines == 0 or total_regions == 0:
            print(f"{crate}: missing production coverage", file=sys.stderr)
            failed = True
            continue
        line_pct = 100 * lines / total_lines
        region_pct = 100 * regions / total_regions
        floor = LINE_FLOORS.get(crate)
        print(
            f"{crate}: lines {line_pct:.2f}% ({lines}/{total_lines}), "
            f"regions {region_pct:.2f}% ({regions}/{total_regions})"
            + (f", line floor {floor:.0f}%" if floor is not None else "")
        )
        if floor is not None and line_pct < floor:
            failed = True
    return int(failed)


if __name__ == "__main__":
    raise SystemExit(main())
