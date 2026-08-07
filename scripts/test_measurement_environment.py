#!/usr/bin/env python3
"""Round-trip tests for the measurement provenance envelope.

`scripts/validate_measurement_artifacts.py` checks the artifacts a run already
produced. These tests check the producers: that `capture()` satisfies its own
schema, that `inject()` preserves a pre-captured envelope rather than taking a
fresh one, and that a record `perf/append_history.py` writes validates against
`perf/schema.json` through the `$ref` to the envelope schema. The committed
history predates the envelope, so nothing else exercises that reference.
"""

from __future__ import annotations

import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO_ROOT / "scripts"))

import measurement_environment as env  # noqa: E402

ENVELOPE_SCHEMA = REPO_ROOT / "perf" / "measurement-environment.schema.json"
HISTORY_SCHEMA = REPO_ROOT / "perf" / "schema.json"
APPEND_HISTORY = REPO_ROOT / "perf" / "append_history.py"


def validator_for(schema_path: Path):
    """Builds a validator whose registry resolves every sibling $ref from disk."""

    import jsonschema
    from referencing import Registry, Resource

    registry = Registry()
    for candidate in sorted((REPO_ROOT / "perf").glob("*.json")):
        document = json.loads(candidate.read_text(encoding="utf-8"))
        identifier = document.get("$id")
        if identifier is not None:
            registry = registry.with_resource(identifier, Resource.from_contents(document))
    schema = json.loads(schema_path.read_text(encoding="utf-8"))
    return jsonschema.validators.validator_for(schema)(schema, registry=registry)


def jsonschema_available() -> bool:
    return importlib.util.find_spec("jsonschema") is not None and (
        importlib.util.find_spec("referencing") is not None
    )


class EnvelopeTests(unittest.TestCase):
    def test_last_level_cache_prefers_the_deepest_data_carrying_level(self) -> None:
        descriptors = [
            {"level": 1, "type": "Data", "size_bytes": 32 * 1024},
            {"level": 3, "type": "Unified", "size_bytes": 96 * 1024 * 1024},
            {"level": 3, "type": "Instruction", "size_bytes": 128 * 1024 * 1024},
        ]
        self.assertEqual(env.last_level_cache_bytes(descriptors), 96 * 1024 * 1024)

    def test_capture_names_the_commit_and_the_compiler(self) -> None:
        envelope = env.capture()
        self.assertEqual(envelope["schema_version"], env.SCHEMA_VERSION)
        self.assertIn("host", envelope)
        self.assertIn("last_level_cache_bytes", envelope["host"])
        self.assertIn("rustc", envelope["toolchain"])

    @unittest.skipUnless(jsonschema_available(), "jsonschema is not installed")
    def test_capture_satisfies_its_own_schema(self) -> None:
        validator_for(ENVELOPE_SCHEMA).validate(env.capture())

    def test_inject_keeps_a_pre_captured_envelope(self) -> None:
        # conformance_run.sh captures before any harness runs so git.dirty
        # describes the tree the measurement ran against rather than the
        # scorecard's own write.
        prior = env.capture()
        prior["timestamp_utc"] = "2000-01-01T00:00:00Z"
        with tempfile.TemporaryDirectory() as directory:
            artifact = Path(directory) / "scorecard.json"
            artifact.write_text(json.dumps({"runner": "probe"}), encoding="utf-8")
            env.inject(artifact, envelope=prior)
            written = json.loads(artifact.read_text(encoding="utf-8"))
        self.assertEqual(
            written["measurement_environment"]["timestamp_utc"], "2000-01-01T00:00:00Z"
        )
        self.assertEqual(written["runner"], "probe")

    @unittest.skipUnless(jsonschema_available(), "jsonschema is not installed")
    def test_append_history_writes_a_record_that_validates(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            history = Path(directory) / "history.ndjson"
            result = subprocess.run(
                [
                    sys.executable,
                    str(APPEND_HISTORY),
                    "--history",
                    str(history),
                    "--profile",
                    "release",
                    "--notes",
                    "envelope round-trip test",
                ],
                cwd=REPO_ROOT,
                check=False,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            record = json.loads(history.read_text(encoding="utf-8").splitlines()[0])
        self.assertIn("measurement_environment", record)
        validator_for(HISTORY_SCHEMA).validate(record)


if __name__ == "__main__":
    unittest.main()
