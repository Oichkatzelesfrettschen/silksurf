#!/usr/bin/env python3
"""Capture the host, toolchain, and revision facts a measurement depends on.

`docs/design/CACHE-LOCALITY-CONTRACT.md` requires every retained record to name
the commit, build profile, cache topology, governor, affinity, competing load,
and command. `scripts/locality_probe.py` was the only instrument that honored
it; conformance scorecards carried a corpus revision alone and
`perf/history.ndjson` carried four fields. This module is the single
implementation both now embed, so one record shape describes every measurement
the repository publishes.

`perf/measurement-environment.schema.json` is the normative definition of what
`capture()` returns.

Standalone use:

    python3 scripts/measurement_environment.py
    python3 scripts/measurement_environment.py --output perf/results/env.json
    python3 scripts/measurement_environment.py --inject docs/conformance/x.json
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import platform
import re
import subprocess
import sys
from pathlib import Path
from typing import Any, Iterable

SCHEMA_VERSION = 1

CACHE_SIZE_RE = re.compile(r"^([0-9]+)([KMG])$", re.IGNORECASE)

# Environment variables that change what a run measures rather than where it
# runs. RUSTFLAGS selects codegen, the display variables select the presentation
# backend, and the trace flag adds instrumentation to the hot path.
RECORDED_ENVIRONMENT_KEYS = (
    "RUSTFLAGS",
    "CARGO_PROFILE",
    "WAYLAND_DISPLAY",
    "DISPLAY",
    "XDG_SESSION_TYPE",
    "SILKSURF_TRACE_APP_FRAME",
)


def parse_size_bytes(value: str) -> int:
    """Converts a sysfs cache size such as ``32K`` or ``96M`` to bytes."""

    match = CACHE_SIZE_RE.match(value.strip())
    if match is None:
        raise ValueError(f"unrecognized cache size: {value!r}")
    magnitude = int(match.group(1))
    unit = match.group(2).upper()
    return magnitude * {"K": 1024, "M": 1024**2, "G": 1024**3}[unit]


def read_text(path: Path) -> str | None:
    try:
        return path.read_text(encoding="utf-8").strip()
    except OSError:
        return None


def parse_optional_int(value: str | None) -> int | None:
    if value is None:
        return None
    try:
        return int(value)
    except ValueError:
        return None


def read_cache_topology(
    root: Path = Path("/sys/devices/system/cpu/cpu0/cache"),
) -> list[dict[str, Any]]:
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
                "ways_of_associativity": parse_optional_int(
                    read_text(index / "ways_of_associativity")
                ),
                "coherency_line_size": parse_optional_int(
                    read_text(index / "coherency_line_size")
                ),
            }
        )
    return descriptors


def last_level_cache_bytes(descriptors: Iterable[dict[str, Any]]) -> int | None:
    """Reports the largest cache at the deepest data-carrying level.

    sysfs reports the nominal capacity of one instance. Affinity, cache
    sharing, explicit partitions, virtualization, and competing load all reduce
    the share a workload actually receives, so this is an upper bound rather
    than an allocation.
    """

    eligible = [entry for entry in descriptors if entry.get("type") in {"Unified", "Data"}]
    if not eligible:
        return None
    highest_level = max(int(entry["level"]) for entry in eligible)
    sizes = [int(entry["size_bytes"]) for entry in eligible if int(entry["level"]) == highest_level]
    return max(sizes) if sizes else None


def command_output(command: list[str]) -> str | None:
    try:
        result = subprocess.run(
            command,
            check=False,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
        )
    except OSError:
        return None
    if result.returncode != 0:
        return None
    return result.stdout.strip()


def git_metadata() -> dict[str, Any] | None:
    root = command_output(["git", "rev-parse", "--show-toplevel"])
    if root is None:
        return None
    commit = command_output(["git", "-C", root, "rev-parse", "HEAD"])
    branch = command_output(["git", "-C", root, "branch", "--show-current"])
    status = command_output(["git", "-C", root, "status", "--porcelain"])
    # The checkout path is host-local, so the record carries the commit identity
    # that reproduces the tree instead.
    return {
        "commit": commit,
        "branch": branch,
        "dirty": bool(status),
    }


def toolchain_metadata() -> dict[str, Any]:
    """Records the compiler the measured artifact was built with.

    `rustc --version` reports the resolved toolchain, which rust-toolchain.toml
    pins. A record naming a rate without naming the compiler cannot separate a
    engine change from a codegen change.
    """

    version_text = command_output(["rustc", "--version"])
    version = None
    if version_text:
        fields = version_text.split()
        if len(fields) >= 2:
            version = fields[1]
    return {
        "rustc": version,
        "rustc_verbose": version_text,
    }


def cpu_model() -> str | None:
    cpuinfo = read_text(Path("/proc/cpuinfo"))
    if cpuinfo is None:
        return None
    for line in cpuinfo.splitlines():
        if line.startswith("model name") and ":" in line:
            return line.split(":", 1)[1].strip()
    return None


def cpu_governors() -> list[str]:
    governors = {
        value
        for path in Path("/sys/devices/system/cpu").glob("cpu[0-9]*/cpufreq/scaling_governor")
        if (value := read_text(path)) is not None
    }
    return sorted(governors)


def process_affinity() -> list[int] | None:
    getter = getattr(os, "sched_getaffinity", None)
    if getter is None:
        return None
    try:
        return sorted(getter(0))
    except OSError:
        return None


def command_environment() -> dict[str, str]:
    return {key: os.environ[key] for key in RECORDED_ENVIRONMENT_KEYS if key in os.environ}


def host_metadata() -> dict[str, Any]:
    topology = read_cache_topology()
    return {
        "platform": platform.platform(),
        "machine": platform.machine(),
        "cpu_model": cpu_model(),
        "logical_cpus": os.cpu_count(),
        "process_affinity": process_affinity(),
        "load_average": list(os.getloadavg()) if hasattr(os, "getloadavg") else None,
        "cpu_governors": cpu_governors(),
        "perf_event_paranoid": read_text(Path("/proc/sys/kernel/perf_event_paranoid")),
        "cache_topology": topology,
        "last_level_cache_bytes": last_level_cache_bytes(topology),
    }


def utc_timestamp() -> str:
    return dt.datetime.now(dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def capture() -> dict[str, Any]:
    """Returns the provenance envelope every measurement artifact embeds."""

    return {
        "schema_version": SCHEMA_VERSION,
        "timestamp_utc": utc_timestamp(),
        "git": git_metadata(),
        "toolchain": toolchain_metadata(),
        "host": host_metadata(),
        "environment": command_environment(),
    }


def inject(
    path: Path,
    key: str = "measurement_environment",
    envelope: dict[str, Any] | None = None,
) -> None:
    """Adds the envelope to an existing JSON artifact under ``key``.

    Harnesses that write their scorecard from Rust gain the envelope this way
    rather than reimplementing sysfs reads per language. Passing an envelope
    captured before the run keeps `git.dirty` describing the tree the
    measurement ran against; capturing after the harness has written its
    scorecard would report the artifact's own write as an uncommitted change.
    """

    document = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(document, dict):
        raise ValueError(f"{path}: expected a JSON object, found {type(document).__name__}")
    document[key] = capture() if envelope is None else envelope
    path.write_text(json.dumps(document, indent=2) + "\n", encoding="utf-8")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--output", type=Path, help="write the envelope to this path")
    parser.add_argument(
        "--inject",
        type=Path,
        action="append",
        default=[],
        metavar="ARTIFACT",
        help="add the envelope to this JSON artifact; repeatable",
    )
    parser.add_argument(
        "--key",
        default="measurement_environment",
        help="object key --inject writes under (default: measurement_environment)",
    )
    parser.add_argument(
        "--from",
        dest="source",
        type=Path,
        metavar="ENVELOPE",
        help="inject this previously captured envelope instead of capturing now",
    )
    args = parser.parse_args(argv)

    envelope: dict[str, Any] | None = None
    if args.source is not None:
        envelope = json.loads(args.source.read_text(encoding="utf-8"))

    for artifact in args.inject:
        if not artifact.is_file():
            print(f"measurement_environment: no such artifact: {artifact}", file=sys.stderr)
            return 1
        inject(artifact, args.key, envelope)
        print(f"measurement_environment: embedded envelope in {artifact}")

    if args.output is not None:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(capture(), indent=2) + "\n", encoding="utf-8")
        print(f"measurement_environment: wrote {args.output}")
        return 0

    if not args.inject:
        json.dump(capture(), sys.stdout, indent=2)
        sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
