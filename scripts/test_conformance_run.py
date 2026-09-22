"""Conformance publication keeps stale output distinct from fresh failing runs."""

import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


class ConformancePublication(unittest.TestCase):
    def test_fresh_output_and_failure_status(self):
        root = Path(__file__).resolve().parents[1]
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            command = output / "cargo"
            scorecard = output / "wpt-scorecard.json"
            original = b'{"original":true}\n'
            environment = dict(
                os.environ,
                PYTHON=sys.executable,
                SCORECARD_DIR=str(output),
                PATH=str(output) + os.pathsep + os.environ["PATH"],
            )
            for exit_status, emits_scorecard in [
                (42, False),
                (0, False),
                (7, True),
                (0, True),
            ]:
                scorecard.write_bytes(original)
                command.write_text(
                    "#!/bin/sh\n"
                    + (
                        'for argument do destination="$argument"; done\n'
                        "printf '%s\\n' '{\"fresh\":true}' > \"$destination\"\n"
                        if emits_scorecard
                        else ""
                    )
                    + f"exit {exit_status}\n"
                )
                command.chmod(0o755)
                result = subprocess.run(
                    [str(root / "scripts/conformance_run.sh"), "wpt"],
                    cwd=root,
                    env=environment,
                    capture_output=True,
                    check=False,
                )
                self.assertEqual(
                    result.returncode,
                    exit_status if exit_status else int(not emits_scorecard),
                    result.stderr,
                )
                if emits_scorecard:
                    published = json.loads(scorecard.read_text())
                    self.assertTrue(published["fresh"])
                    self.assertIn("measurement_environment", published)
                else:
                    self.assertEqual(scorecard.read_bytes(), original)


if __name__ == "__main__":
    unittest.main()
