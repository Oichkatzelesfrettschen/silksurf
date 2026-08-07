#!/usr/bin/env python3
"""Validate every published measurement artifact against its schema.

`perf/schema.json` declared `additionalProperties: false` and a required field
set that nothing checked: `perf/append_history.py` builds each record by hand
and imports no validator. A schema no run enforces documents an intention
rather than a shape.

This gate reads `perf/history.ndjson` line by line against `perf/schema.json`,
reads every conformance scorecard against `docs/conformance/scorecard.schema.json`,
and reports the file, the record index, and the failing JSON pointer. It exits 0
when `jsonschema` is absent, because the merge gate stays runnable on a host
without it; `--require-validator` makes the absence an error instead.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any, Iterator

REPO_ROOT = Path(__file__).resolve().parents[1]

PERF_HISTORY = Path("perf/history.ndjson")
PERF_SCHEMA = Path("perf/schema.json")
SCORECARD_SCHEMA = Path("docs/conformance/scorecard.schema.json")
SCORECARD_GLOB = "docs/conformance/*-scorecard.json"


def load_json(path: Path) -> Any:
    return json.loads((REPO_ROOT / path).read_text(encoding="utf-8"))


def ndjson_records(path: Path) -> Iterator[tuple[int, Any]]:
    """Yields (line number, parsed record) for every non-blank line."""

    text = (REPO_ROOT / path).read_text(encoding="utf-8")
    for number, line in enumerate(text.splitlines(), start=1):
        if not line.strip():
            continue
        yield number, json.loads(line)


def build_validator(schema_path: Path):
    import jsonschema
    from referencing import Registry, Resource

    schema = load_json(schema_path)

    # Every schema in the tree registers under its own $id, and each $ref is a
    # repository-relative path that resolves against the referring schema's $id.
    # The registry therefore satisfies every reference from disk without a
    # network fetch.
    registry = Registry()
    for candidate in sorted(REPO_ROOT.glob("**/*.schema.json")):
        if any(part in {"target", "vendor", "silksurf-extras"} for part in candidate.parts):
            continue
        document = json.loads(candidate.read_text(encoding="utf-8"))
        identifier = document.get("$id")
        if identifier is None:
            continue
        registry = registry.with_resource(identifier, Resource.from_contents(document))

    validator_class = jsonschema.validators.validator_for(schema)
    validator_class.check_schema(schema)
    return validator_class(schema, registry=registry)


def report(errors: list[str], source: str, error: Any) -> None:
    pointer = "/".join(str(part) for part in error.absolute_path) or "(root)"
    errors.append(f"{source}: {pointer}: {error.message}")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--require-validator",
        action="store_true",
        help="fail when the jsonschema package is absent instead of skipping",
    )
    args = parser.parse_args(argv)

    try:
        import jsonschema  # noqa: F401
        import referencing  # noqa: F401
    except ImportError as missing:
        message = f"validate_measurement_artifacts: {missing.name} is not installed"
        if args.require_validator:
            print(f"{message}; --require-validator makes that an error", file=sys.stderr)
            return 1
        print(f"{message}; skipping schema validation")
        return 0

    errors: list[str] = []
    checked = 0

    history = REPO_ROOT / PERF_HISTORY
    if history.is_file():
        validator = build_validator(PERF_SCHEMA)
        for number, record in ndjson_records(PERF_HISTORY):
            checked += 1
            for error in validator.iter_errors(record):
                report(errors, f"{PERF_HISTORY}:{number}", error)

    scorecard_schema = REPO_ROOT / SCORECARD_SCHEMA
    if scorecard_schema.is_file():
        validator = build_validator(SCORECARD_SCHEMA)
        for path in sorted(REPO_ROOT.glob(SCORECARD_GLOB)):
            checked += 1
            document = json.loads(path.read_text(encoding="utf-8"))
            for error in validator.iter_errors(document):
                report(errors, str(path.relative_to(REPO_ROOT)), error)

    if errors:
        print("validate_measurement_artifacts: FAILED", file=sys.stderr)
        for error in errors:
            print(f"  - {error}", file=sys.stderr)
        return 1

    print(f"validate_measurement_artifacts: OK ({checked} records validate)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
