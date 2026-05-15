#!/usr/bin/env python3
# perf/append_history.py -- append a single bench-run record to perf/history.ndjson.
#
# WHY: the rolling NDJSON history at perf/history.ndjson is the source of
#      truth for cross-commit perf trend analysis. Each cargo bench run that
#      we want to track must produce exactly one line conforming to
#      perf/schema.json. Doing this by hand is error-prone (timestamps,
#      git SHAs, rust version drift). This script automates the append step
#      so CI and humans produce byte-identical record shapes.
#
# WHAT: reads metrics from perf/baseline.json (the latest captured snapshot),
#      collects git SHA via `git rev-parse HEAD`, rust version via
#      `rustc --version`, builds an NDJSON record, and appends one line to
#      perf/history.ndjson. Prints a short summary to stdout. Exits non-zero
#      on any failure.
#
# HOW:
#      python3 perf/append_history.py
#      python3 perf/append_history.py --baseline perf/baseline.json \
#                                     --history  perf/history.ndjson \
#                                     --profile  release \
#                                     --notes    "post-fused-pipeline tweak"
#
#      The three required microsecond metrics (fused_pipeline_us,
#      css_cache_hit_us, full_render_us) are taken from
#      baseline.json["metrics"] when present. To accommodate older baseline
#      shapes that do not yet carry those keys, you may inject them via
#      environment variables before invoking the script:
#
#          SILKSURF_FUSED_PIPELINE_US=223.0 \
#          SILKSURF_CSS_CACHE_HIT_US=279.0 \
#          SILKSURF_FULL_RENDER_US=512.0 \
#              python3 perf/append_history.py
#
#      Stdlib only -- no third-party packages. Compatible with Python 3.8+.

from __future__ import annotations

import argparse
import datetime as _dt
import json
import os
import re
import subprocess
import sys
from pathlib import Path
from typing import Any, Dict, Optional

# Required canonical microsecond metrics, in stable serialization order.
REQUIRED_METRICS = ("fused_pipeline_us", "css_cache_hit_us", "full_render_us")

# Map of canonical metric name -> environment-variable override name.
ENV_OVERRIDES = {
    "fused_pipeline_us": "SILKSURF_FUSED_PIPELINE_US",
    "css_cache_hit_us": "SILKSURF_CSS_CACHE_HIT_US",
    "full_render_us": "SILKSURF_FULL_RENDER_US",
}

GIT_SHA_RE = re.compile(r"^[0-9a-f]{40}$")
RUSTC_VERSION_RE = re.compile(r"^rustc\s+(\S+)")


class AppendError(RuntimeError):
    """Wrapper for any user-facing failure in this script."""


def run_capture(cmd: list, cwd: Optional[Path] = None) -> str:
    """Run a subprocess and return stdout, raising AppendError on failure."""
    try:
        completed = subprocess.run(
            cmd,
            check=True,
            text=True,
            capture_output=True,
            cwd=str(cwd) if cwd is not None else None,
        )
    except FileNotFoundError as exc:
        raise AppendError(f"command not found: {cmd[0]}") from exc
    except subprocess.CalledProcessError as exc:
        stderr = (exc.stderr or "").strip()
        raise AppendError(
            f"command {cmd!r} failed with exit code {exc.returncode}: {stderr}"
        ) from exc
    return completed.stdout


def get_git_sha(repo_root: Path) -> str:
    raw = run_capture(["git", "rev-parse", "HEAD"], cwd=repo_root).strip()
    if not GIT_SHA_RE.match(raw):
        raise AppendError(f"git rev-parse returned non-40-hex value: {raw!r}")
    return raw


def get_rust_version() -> str:
    raw = run_capture(["rustc", "--version"]).strip()
    match = RUSTC_VERSION_RE.match(raw)
    if not match:
        raise AppendError(f"unrecognized `rustc --version` output: {raw!r}")
    return match.group(1)


def utc_timestamp() -> str:
    """ISO-8601 UTC timestamp, second precision, suffixed Z."""
    now = _dt.datetime.now(tz=_dt.timezone.utc).replace(microsecond=0)
    # isoformat() yields '...+00:00' for tz-aware datetimes; normalize to Z.
    return now.isoformat().replace("+00:00", "Z")


def load_baseline(path: Path) -> Dict[str, Any]:
    if not path.is_file():
        raise AppendError(f"baseline file not found: {path}")
    try:
        with path.open("r", encoding="utf-8") as handle:
            data = json.load(handle)
    except json.JSONDecodeError as exc:
        raise AppendError(f"baseline {path} is not valid JSON: {exc}") from exc
    if not isinstance(data, dict):
        raise AppendError(f"baseline {path} root must be a JSON object")
    return data


def coerce_float(value: Any, field: str) -> float:
    if isinstance(value, bool):
        # bools are ints in Python; reject explicitly.
        raise AppendError(f"metric {field!r} must be a number, got bool")
    if isinstance(value, (int, float)):
        return float(value)
    if isinstance(value, str):
        try:
            return float(value)
        except ValueError as exc:
            raise AppendError(
                f"metric {field!r} string {value!r} is not numeric"
            ) from exc
    raise AppendError(f"metric {field!r} must be a number, got {type(value).__name__}")


def collect_metrics(baseline: Dict[str, Any]) -> Dict[str, float]:
    """Build the metrics object from baseline + env overrides.

    Strategy: start from baseline['metrics'] (if present), keep every numeric
    entry, then fill in / override the canonical microsecond metrics from
    environment variables. The three REQUIRED_METRICS must end up present.
    """
    raw_metrics = baseline.get("metrics", {})
    if not isinstance(raw_metrics, dict):
        raise AppendError("baseline['metrics'] must be a JSON object")

    metrics: Dict[str, float] = {}
    for key, value in raw_metrics.items():
        if isinstance(value, bool):
            # skip flag fields like css_cascade_blocked.
            continue
        if isinstance(value, (int, float)):
            metrics[str(key)] = float(value)
        # silently drop non-numeric metric entries (strings, nested objects,
        # arrays) -- the schema constrains additional metric values to numbers.

    # Apply env-var overrides for the required canonical metrics.
    for canonical, env_name in ENV_OVERRIDES.items():
        env_value = os.environ.get(env_name)
        if env_value is not None and env_value != "":
            metrics[canonical] = coerce_float(env_value, canonical)

    missing = [m for m in REQUIRED_METRICS if m not in metrics]
    if missing:
        joined = ", ".join(missing)
        env_hint = ", ".join(ENV_OVERRIDES[m] for m in missing)
        raise AppendError(
            "missing required metric(s): "
            + joined
            + ". Add them to perf/baseline.json['metrics'] or set env vars: "
            + env_hint
        )
    return metrics


def build_record(
    git_sha: str,
    rust_version: str,
    profile: str,
    metrics: Dict[str, float],
    notes: Optional[str],
) -> Dict[str, Any]:
    record: Dict[str, Any] = {
        "git_sha": git_sha,
        "timestamp": utc_timestamp(),
        "rust_version": rust_version,
        "profile": profile,
        "metrics": metrics,
    }
    if notes is not None and notes != "":
        record["notes"] = notes
    return record


def append_ndjson(path: Path, record: Dict[str, Any]) -> None:
    # Serialize without indentation so the record fits on one line.
    # sort_keys=False to preserve our intentional field order in build_record.
    line = json.dumps(record, ensure_ascii=True, separators=(",", ":"))
    if "\n" in line or "\r" in line:
        # Defense in depth -- json.dumps without indent should not emit newlines.
        raise AppendError("serialized record unexpectedly contained a newline")

    # Open in binary append mode to avoid platform newline translation.
    # If the file does not end with a newline, do not add one before our line --
    # that would create a blank record. We always end our own line with \n.
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("ab") as handle:
        handle.write(line.encode("ascii"))
        handle.write(b"\n")


def parse_args(argv: list) -> argparse.Namespace:
    repo_root_default = Path(__file__).resolve().parent.parent
    parser = argparse.ArgumentParser(
        description="Append one bench-run record to perf/history.ndjson.",
    )
    parser.add_argument(
        "--baseline",
        type=Path,
        default=repo_root_default / "perf" / "baseline.json",
        help="path to the baseline JSON to read metrics from",
    )
    parser.add_argument(
        "--history",
        type=Path,
        default=repo_root_default / "perf" / "history.ndjson",
        help="path to the rolling NDJSON history file to append to",
    )
    parser.add_argument(
        "--profile",
        choices=("release", "debug"),
        default="release",
        help="cargo build profile under which the bench was built",
    )
    parser.add_argument(
        "--notes",
        type=str,
        default=None,
        help="optional free-text annotation for this run",
    )
    parser.add_argument(
        "--repo-root",
        type=Path,
        default=repo_root_default,
        help="git repo root (used for `git rev-parse HEAD`)",
    )
    return parser.parse_args(argv)


def main(argv: Optional[list] = None) -> int:
    args = parse_args(sys.argv[1:] if argv is None else argv)

    try:
        baseline = load_baseline(args.baseline)
        metrics = collect_metrics(baseline)
        git_sha = get_git_sha(args.repo_root)
        rust_version = get_rust_version()
        record = build_record(
            git_sha=git_sha,
            rust_version=rust_version,
            profile=args.profile,
            metrics=metrics,
            notes=args.notes,
        )
        append_ndjson(args.history, record)
    except AppendError as exc:
        print(f"append_history: error: {exc}", file=sys.stderr)
        return 1
    except OSError as exc:
        print(f"append_history: I/O error: {exc}", file=sys.stderr)
        return 1

    short_sha = git_sha[:8]
    # Emit history path relative to cwd if possible, for readability.
    try:
        display_path = args.history.resolve().relative_to(Path.cwd().resolve())
    except ValueError:
        display_path = args.history
    print(f"appended run {short_sha} to {display_path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
