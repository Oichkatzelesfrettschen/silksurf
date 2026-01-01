#!/usr/bin/env python3
from __future__ import annotations

import os
import re
import subprocess
import sys

TOTAL_RE = re.compile(r"^total:\s*[^,]*,\s*per-iter:\s*([0-9.]+)(ns|us|ms|s)")


def to_ns(value: float, unit: str) -> float:
    if unit == "ns":
        return value
    if unit == "us":
        return value * 1_000.0
    if unit == "ms":
        return value * 1_000_000.0
    if unit == "s":
        return value * 1_000_000_000.0
    raise ValueError(f"unknown unit: {unit}")


def parse_per_iter(output: str) -> float:
    per_iter = None
    for line in output.splitlines():
        cleaned = line.replace("\u00b5", "u")
        match = TOTAL_RE.search(cleaned)
        if match:
            per_iter = to_ns(float(match.group(1)), match.group(2))
    if per_iter is None:
        raise RuntimeError("missing per-iter line")
    return per_iter


def run_cmd(cmd: list[str]) -> str:
    result = subprocess.run(cmd, check=True, text=True, capture_output=True)
    return result.stdout


def run_time_cmd(cmd: list[str]) -> tuple[str, str]:
    time_cmd = ["/usr/bin/time", "-v"] + cmd
    result = subprocess.run(time_cmd, check=True, text=True, capture_output=True)
    return result.stdout, result.stderr


def parse_max_rss_kb(stderr: str) -> int | None:
    for line in stderr.splitlines():
        if "Maximum resident set size" in line:
            parts = line.split(":", 1)
            if len(parts) == 2:
                return int(parts[1].strip())
    return None


def read_threshold_ns(ns_name: str, us_name: str, default_us: float) -> float:
    if ns_name in os.environ:
        return float(os.environ[ns_name])
    if us_name in os.environ:
        return to_ns(float(os.environ[us_name]), "us")
    return to_ns(default_us, "us")


def main() -> int:
    pipeline_ns = read_threshold_ns("PIPELINE_NS", "PIPELINE_US", 15.0)
    selectors_ns = read_threshold_ns("SELECTORS_NS", "SELECTORS_US", 0.2)
    cascade_ns = read_threshold_ns("CASCADE_NS", "CASCADE_US", 30.0)

    checks = [
        ("pipeline", ["cargo", "run", "-p", "silksurf-engine", "--bin", "bench_pipeline"], pipeline_ns),
        (
            "selectors",
            ["cargo", "run", "-p", "silksurf-css", "--bin", "bench_selectors", "--", "--guard"],
            selectors_ns,
        ),
        ("cascade", ["cargo", "run", "-p", "silksurf-css", "--bin", "bench_cascade_guard"], cascade_ns),
    ]

    failed = False
    for name, cmd, limit in checks:
        output = run_cmd(cmd)
        per_iter = parse_per_iter(output)
        print(f"{name} per-iter: {per_iter:.1f} ns (limit {limit:.1f} ns)")
        if per_iter > limit:
            print(f"{name} exceeds limit", file=sys.stderr)
            failed = True

    max_rss = os.environ.get("MAX_RSS_KB")
    if max_rss:
        bench_path = "target/debug/bench_pipeline"
        if not os.path.exists(bench_path):
            subprocess.run(
                ["cargo", "build", "-p", "silksurf-engine", "--bin", "bench_pipeline"],
                check=True,
                text=True,
            )
        output, stderr = run_time_cmd([bench_path])
        _ = output
        rss_kb = parse_max_rss_kb(stderr)
        if rss_kb is None:
            print("missing RSS data from /usr/bin/time", file=sys.stderr)
            failed = True
        elif rss_kb > int(max_rss):
            print(f"pipeline RSS {rss_kb} KB exceeds {max_rss} KB", file=sys.stderr)
            failed = True

    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
