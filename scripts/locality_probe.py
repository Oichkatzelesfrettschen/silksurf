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
import json
import math
import re
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Any, Iterable

# The host, toolchain, and revision facts are the same ones every measurement
# artifact carries, so they come from the one implementation the conformance
# scorecards and perf/history.ndjson also embed. The re-exported names keep
# scripts/test_locality_probe.py addressing this module.
sys.path.insert(0, str(Path(__file__).resolve().parent))
from measurement_environment import (  # noqa: E402
    capture as capture_environment,
    last_level_cache_bytes,
    parse_optional_int,
    parse_size_bytes,
    read_cache_topology,
    read_text,
)

__all__ = [
    "last_level_cache_bytes",
    "parse_optional_int",
    "parse_size_bytes",
    "read_cache_topology",
    "read_text",
]

SCHEMA_VERSION = 3
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
GNU_TIME_FIELDS = {
    "Maximum resident set size (kbytes)": "max_rss_kb",
    "Minor (reclaiming a frame) page faults": "minor_page_faults",
    "Major (requiring I/O) page faults": "major_page_faults",
    "Voluntary context switches": "voluntary_context_switches",
    "Involuntary context switches": "involuntary_context_switches",
}
UNAVAILABLE_COUNTER_VALUES = {"<not counted>", "<not supported>", "not counted", "not supported"}
EVENT_MODIFIER_RE = re.compile(r":[ukhHGISDpPe]+$")
# The kernel charges a context switch and a migration to the scheduler, so a
# user-mode modifier makes both count zero for every workload. GNU time reports
# the same quantities from rusage without a privilege condition.
SCHEDULER_EVENTS = {"context-switches", "cpu-migrations"}
USER_ONLY_MODIFIERS = frozenset("u")


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


def base_event_name(event: str) -> str:
    """Strips the privilege and precision modifiers perf appends to an event name.

    `perf stat` echoes `instructions:u` when `perf_event_paranoid` restricts
    counting to user mode, and `instructions` when it does not. Derivations key
    on the base name so the same record shape holds across both hosts.
    """

    return EVENT_MODIFIER_RE.sub("", event.strip())


def event_modifiers(event: str) -> str:
    match = EVENT_MODIFIER_RE.search(event.strip())
    return match.group(0)[1:] if match else ""


def partition_counters(
    counters: dict[str, int | float | None],
) -> tuple[dict[str, int | float | None], dict[str, str]]:
    """Splits parsed counters into measurements and structurally blind events.

    A scheduler event counted under a user-only modifier reads zero for every
    workload, so it records as unobservable with its cause rather than as a
    measured zero.
    """

    measured: dict[str, int | float | None] = {}
    unobservable: dict[str, str] = {}
    for event, value in counters.items():
        modifiers = set(event_modifiers(event))
        blind = base_event_name(event) in SCHEDULER_EVENTS and modifiers <= USER_ONLY_MODIFIERS
        if blind and modifiers:
            unobservable[event] = (
                "scheduler event counted under a user-only modifier reads zero; "
                "see process.voluntary_context_switches and "
                "process.involuntary_context_switches"
            )
            continue
        measured[event] = value
    return measured, unobservable


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


def probe_perf(events: tuple[str, ...]) -> tuple[tuple[str, ...], dict[str, str]]:
    executable = shutil.which("perf")
    if executable is None:
        return (), {event: "perf not found in PATH" for event in events}

    supported: list[str] = []
    unavailable: dict[str, str] = {}
    for event in events:
        result = subprocess.run(
            [executable, "stat", "--no-big-num", "-x", "\t", "-e", event, "--", "true"],
            check=False,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            text=True,
        )
        if result.returncode == 0:
            supported.append(event)
            continue
        reason = result.stderr.strip().splitlines()
        unavailable[event] = reason[-1] if reason else f"perf probe exited {result.returncode}"
    return tuple(supported), unavailable


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
        parsed = parse_perf_stat(perf_text) if use_perf else {}
        counters, unobservable = partition_counters(parsed)
        return {
            "sample": sample_index,
            "return_code": result.returncode,
            "elapsed_ns": elapsed_ns,
            "process": parse_gnu_time(time_text),
            "counters": counters,
            "unobservable_counters": unobservable,
        }


def ratio(numerator: int | float | None, denominator: int | float | None) -> float | None:
    if numerator is None or denominator in (None, 0):
        return None
    return float(numerator) / float(denominator)


def counter_by_base(
    counters: dict[str, int | float | None], event: str
) -> int | float | None:
    """Looks a counter up by base name, ignoring the modifiers perf appended."""

    for name, value in counters.items():
        if base_event_name(name) == event:
            return value
    return None


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

    unavailable: dict[str, str] = {}
    for derived, numerator, denominator in (
        ("cache_miss_ratio", "cache-misses", "cache-references"),
        ("instructions_per_cycle", "instructions", "cycles"),
    ):
        values = [
            ratio(
                counter_by_base(sample["counters"], numerator),
                counter_by_base(sample["counters"], denominator),
            )
            for sample in samples
        ]
        summary[derived] = summarize_numeric(value for value in values if value is not None)
        if summary[derived] is None:
            missing = [
                event
                for event in (numerator, denominator)
                if all(counter_by_base(sample["counters"], event) is None for sample in samples)
            ]
            unavailable[derived] = (
                f"no sample carries {' and '.join(missing)}"
                if missing
                else "every sample divides by zero"
            )
    summary["unavailable_derivations"] = unavailable
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

    if args.perf == "off":
        supported_events: tuple[str, ...] = ()
        unavailable_events = {event: "disabled by --perf off" for event in events}
    else:
        supported_events, unavailable_events = probe_perf(events)
    if args.perf == "on" and unavailable_events:
        details = "; ".join(f"{event}: {reason}" for event, reason in unavailable_events.items())
        parser.error(f"requested perf events are unavailable: {details}")
    use_perf = args.perf != "off" and bool(supported_events)

    # Capturing before the samples run records the load the workload starts
    # against, which is the competing activity the cache-locality contract
    # names as part of every record.
    environment = capture_environment()
    samples = [
        run_sample(command, supported_events, use_perf, index + 1)
        for index in range(args.repeat)
    ]
    failed_samples = [sample for sample in samples if sample["return_code"] != 0]

    budget_path = args.budget if args.budget.exists() else None
    record = {
        "schema_version": SCHEMA_VERSION,
        "name": args.name,
        # A retained record outlives the exit status of the run that produced it,
        # so the workload outcome rides in the record itself.
        "workload": {
            "status": "failed" if failed_samples else "ok",
            "sample_count": len(samples),
            "failed_sample_count": len(failed_samples),
        },
        "measurement_environment": environment,
        "command": command,
        "perf": {
            "policy": args.perf,
            "available": bool(supported_events),
            "used": use_perf,
            "requested_events": list(events),
            "used_events": list(supported_events) if use_perf else [],
            "unavailable_events": unavailable_events,
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
