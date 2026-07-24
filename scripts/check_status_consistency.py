#!/usr/bin/env python3
"""Reject drift between canonical status prose and machine-readable inputs."""

from __future__ import annotations

import json
import re
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def read_text(relative: str) -> str:
    return (ROOT / relative).read_text(encoding="utf-8")


def fail(errors: list[str], message: str) -> None:
    errors.append(message)


def require_text(errors: list[str], path: str, needle: str) -> None:
    text = read_text(path)
    normalized = re.sub(r"\s+", " ", text)
    if needle not in text and needle not in normalized:
        fail(errors, f"{path}: missing required text: {needle!r}")


def forbid_text(errors: list[str], path: str, needle: str) -> None:
    if needle in read_text(path):
        fail(errors, f"{path}: stale text is forbidden: {needle!r}")


def main() -> int:
    errors: list[str] = []

    cargo = tomllib.loads(read_text("Cargo.toml"))
    members = cargo["workspace"]["members"]
    crate_members = [member for member in members if member.startswith("crates/")]
    if len(crate_members) != 13:
        fail(errors, f"Cargo.toml: expected 13 crates/* members, found {len(crate_members)}")
    if "silksurf-js" not in members:
        fail(errors, "Cargo.toml: silksurf-js must remain a workspace member")
    if len(members) != 14:
        fail(errors, f"Cargo.toml: expected 14 total members, found {len(members)}")

    rust_version = cargo["workspace"]["package"]["rust-version"]
    toolchain = tomllib.loads(read_text("rust-toolchain.toml"))["toolchain"]["channel"]
    if rust_version != toolchain:
        fail(
            errors,
            "toolchain drift: Cargo.toml rust-version "
            f"{rust_version!r} != rust-toolchain.toml channel {toolchain!r}",
        )

    wpt = json.loads(
        read_text("crates/silksurf-engine/conformance/wpt-scorecard.json")
    )
    if wpt.get("runner_kind") != "wpt-synthetic":
        fail(errors, "wpt-scorecard.json: runner_kind must remain explicit")
    if wpt.get("pass") != wpt.get("total") or wpt.get("fail") or wpt.get("skip"):
        fail(errors, "wpt-scorecard.json: current canonical status expects an all-pass run")

    wpt_fraction = f"{wpt['pass']}/{wpt['total']}"
    for path in ("README.md", "docs/STATUS.md"):
        require_text(errors, path, wpt_fraction)
        require_text(errors, path, "synthetic")
        require_text(errors, path, "not vendored")

    require_text(errors, "README.md", rust_version)
    require_text(errors, "docs/STATUS.md", rust_version)
    require_text(errors, "docs/STATUS.md", "ControlFlow::Wait")
    require_text(errors, "docs/STATUS.md", "99.81%")
    require_text(errors, "docs/STATUS.md", "69.38%")
    require_text(
        errors,
        "crates/silksurf-net/README.md",
        "Persistent WebSocket transport",
    )
    require_text(
        errors,
        "docs/roadmaps/BROWSER-FUNCTIONALIZATION-ACTION-PLAN.md",
        "Decision gates",
    )

    stale_claims = {
        "README.md": ("63/63",),
        "docs/ARCHITECTURE.md": (
            "Integration Plan (Open)",
            "nightly-2026-04-05",
        ),
        "docs/JS_ENGINE.md": (
            "Integrate `silksurf-js` behind `silksurf-engine`",
            "Introduce packed bytecode instruction formats",
        ),
        "crates/silksurf-app/README.md": (
            "one-line stub",
            "eventually, P6",
        ),
        "crates/silksurf-net/README.md": (
            "persistent async browser sockets",
        ),
    }
    for path, needles in stale_claims.items():
        for needle in needles:
            forbid_text(errors, path, needle)

    if errors:
        print("status-consistency check failed:", file=sys.stderr)
        for error in errors:
            print(f"  - {error}", file=sys.stderr)
        return 1

    print(
        "status-consistency: OK "
        f"({len(crate_members)} crates/* + silksurf-js, Rust {rust_version}, "
        f"synthetic WPT {wpt_fraction})"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
