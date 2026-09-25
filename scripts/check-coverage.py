#!/usr/bin/env python3
"""Check production line coverage floors from LLVM JSON and LCOV reports."""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path


# Baseline measured on 2026-09-26 with inline test modules removed from LCOV.
# Desktop rendering is reported separately because a line floor rewards UI glue tests.
LINE_FLOORS = {
    "cli": 89.0,
    "core": 90.0,
    "http": 81.0,
    "opencollection": 84.0,
    "postman": 72.0,
    "yaak": 67.0,
}


def crate_for(path: Path) -> str | None:
    parts = path.parts
    if "crates" not in parts:
        return None
    position = parts.index("crates")
    if len(parts) <= position + 3 or parts[position + 2] != "src":
        return None
    if "tests" in parts[position + 3 :] or path.name == "tests.rs" or path.name.endswith("_tests.rs"):
        return None
    return parts[position + 1]


def inline_test_lines(path: Path) -> set[int]:
    """Find top-level cfg(test) modules; reject unfamiliar forms rather than inflate coverage."""
    source = path.read_text().splitlines()
    excluded: set[int] = set()
    index = 0
    while index < len(source):
        if source[index].strip() != "#[cfg(test)]":
            index += 1
            continue
        if source[index] != "#[cfg(test)]" or index + 1 == len(source):
            raise ValueError(f"unsupported cfg(test) item in {path}:{index + 1}")
        declaration = source[index + 1]
        if re.fullmatch(r"mod [A-Za-z_][A-Za-z_0-9]*;", declaration):
            end = index + 1
        elif re.fullmatch(r"mod [A-Za-z_][A-Za-z_0-9]* \{", declaration):
            end = next((line for line in range(index + 2, len(source)) if source[line] == "}"), None)
            if end is None:
                raise ValueError(f"unclosed cfg(test) module in {path}:{index + 1}")
        else:
            raise ValueError(f"unsupported cfg(test) item in {path}:{index + 1}")
        excluded.update(range(index + 1, end + 2))
        index = end + 1
    return excluded


def lcov_lines(path: Path) -> dict[Path, dict[int, int]]:
    records: dict[Path, dict[int, int]] = {}
    current: Path | None = None
    for line in path.read_text().splitlines():
        if line.startswith("SF:"):
            current = Path(line[3:])
            records.setdefault(current, {})
        elif line.startswith("DA:") and current is not None:
            number, count, *_ = line[3:].split(",")
            number, count = int(number), int(count)
            counts = records[current]
            counts[number] = max(count, counts.get(number, 0))
    return records


def main() -> int:
    if len(sys.argv) != 3:
        print("usage: check-coverage.py llvm-cov.json llvm-cov.lcov", file=sys.stderr)
        return 2
    data = json.loads(Path(sys.argv[1]).read_text())
    files = data["data"][0]["files"]
    report_lines = lcov_lines(Path(sys.argv[2]))
    expected = {
        Path(item["filename"])
        for item in files
        if crate_for(Path(item["filename"])) in LINE_FLOORS
    }
    missing = expected - report_lines.keys()
    if missing:
        print(f"LCOV is missing {len(missing)} behavior source file(s)", file=sys.stderr)
        return 1
    counts = {name: [0, 0, 0, 0] for name in (*LINE_FLOORS, "desktop")}
    for item in files:
        crate = crate_for(Path(item["filename"]))
        if crate not in counts:
            continue
        current = counts[crate]
        regions = item["summary"]["regions"]
        current[2] += regions["covered"]
        current[3] += regions["count"]

    for path, lines in report_lines.items():
        crate = crate_for(path)
        if crate not in counts:
            continue
        excluded = inline_test_lines(path) if crate in LINE_FLOORS else set()
        production = [count for number, count in lines.items() if number not in excluded]
        counts[crate][0] += sum(count > 0 for count in production)
        counts[crate][1] += len(production)

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
            f"raw regions {region_pct:.2f}% ({regions}/{total_regions})"
            + (f", line floor {floor:.0f}%" if floor is not None else "")
        )
        if floor is not None and line_pct < floor:
            failed = True
    return int(failed)


if __name__ == "__main__":
    raise SystemExit(main())
