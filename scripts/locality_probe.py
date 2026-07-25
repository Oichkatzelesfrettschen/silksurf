#!/usr/bin/env python3
"""Measure one command's locality-related runtime evidence.

The probe records wall time and GNU time process statistics on every host. When
Linux perf counters are available, it also records generic hardware and
scheduler counters. Results are descriptive evidence, not portable pass/fail
gates: cache topology, counter availability, kernel policy, and competing load
are part of every record.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import math
import os
import platform
import re
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Any, Iterable

SCHEMA_VERSION = 1
DEFAULT_EVENTS = (
    "cycles",
    "instructions",
    "cache-references",
    "cache-misses",
    "branches",
    "branch-misses",
    "context-switches",
    "cpu-migrations",
    "page-faults",
)
CACHE_SIZE_RE = re.compile(r"^([0-9]+)([KMG])$", re.IGNORECASE)
GNU_TIME_FIELDS = {
    "Maximum resident set size (kbytes)": "max_rss_kb",
    "Minor (reclaiming a frame) page faults": "minor_page_faults",
    "Major (requiring I/O) page faults": "major_page_faults",
    "Voluntary context switches": "voluntary_context_switches",
    "Involuntary context switches": "involuntary_context_switches",
}
UNAVAILABLE_COUNTER_VALUES = {"<not counted>", "<not supported>", "not counted", "not supported"}


def parse_size_bytes(value: str) -> int:
    """Converts a sysfs cache size such as ``32K`` or ``96M`` to bytes."""

    match = CACHE_SIZE_RE.fullmatch(value.strip())
    if match is None:
        raise ValueError(f"unsupported cache size: {value!r}")
    quantity = int(match.group(1))
    multiplier = {"K": 1024, "M": 1024**2, "G": 1024**3}[match.group(2).upper()]
    return quantity * multiplier


def read_text(path: Path) -> str | None:
    try:
        return path.read_text(encoding="utf-8").strip()
    except OSError:
        return None


def read_cache_topology(root: Path = Path("/sys/devices/system/cpu/cpu0/cache")) -> list[dict[str, Any]]:
    """Reads CPU 0 cache descriptors from Linux sysfs when available."""

    descriptors: list[dict[str, Any]] = []
    if not root.is_dir():
        return descriptors
    for index in sorted(root.glob("index*"), key=lambda path: path.name):
        level_text = read_text(index / "level")
        type_text = read_text(index / "type")
        size_text = read_text(index / "size")
        if level_text is None or type_text is None or size_text is None:
            continue
        try:
            level = int(level_text)
            size_bytes = parse_size_bytes(size_text)
        except ValueError:
            continue
        descriptors.append(
            {
                "index": index.name,
                "level": level,
                "type": type_text,
                "size_bytes": size_bytes,
                "shared_cpu_list": read_text(index / "shared_cpu_list"),
                "ways_of_associativity": parse_optional_int(read_text(index / "ways_of_associativity")),
                "coherency_line_size": parse_optional_int(read_text(index / "coherency_line_size")),
            }
        )
    return descriptors


def parse_optional_int(value: str | None) -> int | None:
    if value is None:
        return None
    try:
        return int(value)
    except ValueError:
        return None


def last_level_cache_bytes(descriptors: Iterable[dict[str, Any]]) -> int | None:
    eligible = [entry for entry in descriptors if entry.get("type") in {"Unified", "Data"}]
    if not eligible:
        return None
    highest_level = max(int(entry["level"]) for entry in eligible)
    sizes = [int(entry["size_bytes"]) for entry in eligible if int(entry["level"]) == highest_level]
    return max(sizes) if sizes else None


def parse_gnu_time(text: str) -> dict[str, int]:
    parsed: dict[str, int] = {}
    for line in text.splitlines():
        stripped = line.strip()
        for source, target in GNU_TIME_FIELDS.items():
            prefix = f"{source}:"
            if stripped.startswith(prefix):
                raw = stripped[len(prefix) :].strip()
                try:
                    parsed[target] = int(raw)
                except ValueError:
                    pass
                break
    return parsed


def parse_perf_number(value: str) -> int | float | None:
    stripped = value.strip()
    if stripped.lower() in UNAVAILABLE_COUNTER_VALUES or not stripped:
        return None
    normalized = stripped.replace(" ", "").replace(",", "")
    try:
        if any(marker in normalized.lower() for marker in (".", "e")):
            return float(normalized)
        return int(normalized)
    except ValueError:
        return None


def parse_perf_stat(text: str) -> dict[str, int | float | None]:
    """Parses ``perf stat -x '\t'`` output into event-name values."""

    counters: dict[str, int | float | None] = {}
    for line in text.splitlines():
        fields = line.split("\t")
        if len(fields) < 3:
            continue
        value = parse_perf_number(fields[0])
        event = fields[2].strip()
        if not event:
            continue
        counters[event] = value
    return counters


def nearest_rank(values: Iterable[float | int], percentile: float) -> float | None:
    ordered = sorted(float(value) for value in values)
    if not ordered:
        return None
    if not 0.0 <= percentile <= 100.0:
        raise ValueError("percentile must be between 0 and 100")
    rank = max(1, math.ceil(percentile / 100.0 * len(ordered)))
    return ordered[rank - 1]


def summarize_numeric(values: Iterable[float | int]) -> dict[str, float] | None:
    materialized = [float(value) for value in values]
    if not materialized:
        return None
    return {
        "min": min(materialized),
        "median": statistics.median(materialized),
        "p95": nearest_rank(materialized, 95.0),
        "p99": nearest_rank(materialized, 99.0),
        "max": max(materialized),
        "mean": statistics.fmean(materialized),
    }


def probe_perf(events: tuple[str, ...]) -> tuple[bool, str | None]:
    executable = shutil.which("perf")
    if executable is None:
        return False, "perf not found in PATH"
    result = subprocess.run(
        [executable, "stat", "--no-big-num", "-x", "\t", "-e", events[0], "--", "true"],
        check=False,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
        text=True,
    )
    if result.returncode == 0:
        return True, None
    reason = result.stderr.strip().splitlines()
    return False, reason[-1] if reason else f"perf probe exited {result.returncode}"


def command_environment() -> dict[str, str]:
    keys = (
        "RUSTFLAGS",
        "CARGO_PROFILE",
        "WAYLAND_DISPLAY",
        "DISPLAY",
        "XDG_SESSION_TYPE",
        "SILKSURF_TRACE_APP_FRAME",
    )
    return {key: os.environ[key] for key in keys if key in os.environ}


def run_sample(
    command: list[str],
    events: tuple[str, ...],
    use_perf: bool,
    sample_index: int,
) -> dict[str, Any]:
    time_executable = Path("/usr/bin/time")
    if not time_executable.exists():
        raise RuntimeError("/usr/bin/time is required for RSS and fault measurements")

    with tempfile.TemporaryDirectory(prefix="silksurf-locality-") as directory:
        directory_path = Path(directory)
        time_path = directory_path / "time.txt"
        perf_path = directory_path / "perf.tsv"
        timed_command = [str(time_executable), "-v", "-o", str(time_path), *command]
        executed_command = timed_command
        if use_perf:
            perf_executable = shutil.which("perf")
            if perf_executable is None:
                raise RuntimeError("perf disappeared after capability probing")
            executed_command = [
                perf_executable,
                "stat",
                "--no-big-num",
                "-x",
                "\t",
                "-o",
                str(perf_path),
                "-e",
                ",".join(events),
                "--",
                *timed_command,
            ]

        start_ns = time.monotonic_ns()
        result = subprocess.run(executed_command, check=False)
        elapsed_ns = time.monotonic_ns() - start_ns

        time_text = time_path.read_text(encoding="utf-8") if time_path.exists() else ""
        perf_text = perf_path.read_text(encoding="utf-8") if perf_path.exists() else ""
        counters = parse_perf_stat(perf_text) if use_perf else {}
        return {
            "sample": sample_index,
            "return_code": result.returncode,
            "elapsed_ns": elapsed_ns,
            "process": parse_gnu_time(time_text),
            "counters": counters,
        }


def ratio(numerator: int | float | None, denominator: int | float | None) -> float | None:
    if numerator is None or denominator in (None, 0):
        return None
    return float(numerator) / float(denominator)


def summarize_samples(samples: list[dict[str, Any]]) -> dict[str, Any]:
    summary: dict[str, Any] = {
        "elapsed_ns": summarize_numeric(sample["elapsed_ns"] for sample in samples),
        "max_rss_kb": summarize_numeric(
            sample["process"]["max_rss_kb"]
            for sample in samples
            if "max_rss_kb" in sample["process"]
        ),
    }
    counter_names = sorted({name for sample in samples for name in sample["counters"]})
    counter_summary: dict[str, Any] = {}
    for name in counter_names:
        values = [sample["counters"].get(name) for sample in samples]
        materialized = [value for value in values if value is not None]
        counter_summary[name] = summarize_numeric(materialized)
    summary["counters"] = counter_summary

    miss_ratios = [
        ratio(sample["counters"].get("cache-misses"), sample["counters"].get("cache-references"))
        for sample in samples
    ]
    summary["cache_miss_ratio"] = summarize_numeric(
        value for value in miss_ratios if value is not None
    )
    ipc_values = [
        ratio(sample["counters"].get("instructions"), sample["counters"].get("cycles"))
        for sample in samples
    ]
    summary["instructions_per_cycle"] = summarize_numeric(
        value for value in ipc_values if value is not None
    )
    return summary


def load_budget(path: Path | None) -> dict[str, Any] | None:
    if path is None:
        return None
    return json.loads(path.read_text(encoding="utf-8"))


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Record cache-locality evidence for one command without making host-portable claims."
    )
    parser.add_argument("--name", required=True, help="Stable workload name")
    parser.add_argument("--repeat", type=int, default=5, help="Number of complete process runs")
    parser.add_argument("--output", type=Path, help="Write JSON here instead of stdout")
    parser.add_argument(
        "--budget",
        type=Path,
        default=Path("perf/locality-budget.json"),
        help="Hypothesis/budget metadata to embed when the file exists",
    )
    parser.add_argument(
        "--perf",
        choices=("auto", "on", "off"),
        default="auto",
        help="Hardware counter policy",
    )
    parser.add_argument(
        "--events",
        default=",".join(DEFAULT_EVENTS),
        help="Comma-separated perf events",
    )
    parser.add_argument("command", nargs=argparse.REMAINDER, help="Command after --")
    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    command = list(args.command)
    if command and command[0] == "--":
        command.pop(0)
    if not command:
        parser.error("a command is required after --")
    if args.repeat <= 0:
        parser.error("--repeat must be positive")

    events = tuple(event.strip() for event in args.events.split(",") if event.strip())
    if not events:
        parser.error("--events must name at least one event")

    perf_available, perf_reason = probe_perf(events)
    if args.perf == "on" and not perf_available:
        parser.error(f"perf counters requested but unavailable: {perf_reason}")
    use_perf = args.perf != "off" and perf_available

    topology = read_cache_topology()
    samples = [run_sample(command, events, use_perf, index + 1) for index in range(args.repeat)]
    failed_samples = [sample for sample in samples if sample["return_code"] != 0]

    budget_path = args.budget if args.budget.exists() else None
    record = {
        "schema_version": SCHEMA_VERSION,
        "name": args.name,
        "timestamp_utc": dt.datetime.now(dt.timezone.utc).isoformat(),
        "command": command,
        "environment": command_environment(),
        "host": {
            "platform": platform.platform(),
            "machine": platform.machine(),
            "logical_cpus": os.cpu_count(),
            "cache_topology": topology,
            "last_level_cache_bytes": last_level_cache_bytes(topology),
        },
        "perf": {
            "policy": args.perf,
            "available": perf_available,
            "used": use_perf,
            "unavailable_reason": perf_reason,
            "events": list(events),
        },
        "budget": load_budget(budget_path),
        "samples": samples,
        "summary": summarize_samples(samples),
    }

    serialized = json.dumps(record, indent=2, sort_keys=True) + "\n"
    if args.output is None:
        sys.stdout.write(serialized)
    else:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(serialized, encoding="utf-8")
        print(f"wrote {args.output}")

    if failed_samples:
        print(
            f"{len(failed_samples)} of {len(samples)} workload runs exited nonzero",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
