#!/usr/bin/env python3
"""Focused parser and statistic tests for locality_probe.py."""

from __future__ import annotations

import importlib.util
import tempfile
import unittest
from pathlib import Path

MODULE_PATH = Path(__file__).with_name("locality_probe.py")
SPEC = importlib.util.spec_from_file_location("locality_probe", MODULE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("cannot load locality_probe.py")
locality_probe = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(locality_probe)


class LocalityProbeTests(unittest.TestCase):
    def test_parse_size_bytes(self) -> None:
        self.assertEqual(locality_probe.parse_size_bytes("32K"), 32 * 1024)
        self.assertEqual(locality_probe.parse_size_bytes("96M"), 96 * 1024 * 1024)
        self.assertEqual(locality_probe.parse_size_bytes("1G"), 1024**3)

    def test_last_level_cache_uses_highest_data_or_unified_level(self) -> None:
        descriptors = [
            {"level": 1, "type": "Data", "size_bytes": 32 * 1024},
            {"level": 2, "type": "Unified", "size_bytes": 1024 * 1024},
            {"level": 3, "type": "Unified", "size_bytes": 32 * 1024 * 1024},
            {"level": 3, "type": "Instruction", "size_bytes": 64 * 1024 * 1024},
        ]
        self.assertEqual(locality_probe.last_level_cache_bytes(descriptors), 32 * 1024 * 1024)

    def test_parse_gnu_time(self) -> None:
        parsed = locality_probe.parse_gnu_time(
            """
            Maximum resident set size (kbytes): 24576
            Minor (reclaiming a frame) page faults: 41
            Major (requiring I/O) page faults: 2
            Voluntary context switches: 7
            Involuntary context switches: 3
            """
        )
        self.assertEqual(parsed["max_rss_kb"], 24576)
        self.assertEqual(parsed["minor_page_faults"], 41)
        self.assertEqual(parsed["major_page_faults"], 2)
        self.assertEqual(parsed["voluntary_context_switches"], 7)
        self.assertEqual(parsed["involuntary_context_switches"], 3)

    def test_parse_perf_stat(self) -> None:
        counters = locality_probe.parse_perf_stat(
            "1000\t\tcycles\t100.00\t100.00\n"
            "2500\t\tinstructions\t100.00\t100.00\n"
            "<not supported>\t\tcache-misses\t0.00\t0.00\n"
        )
        self.assertEqual(counters["cycles"], 1000)
        self.assertEqual(counters["instructions"], 2500)
        self.assertIsNone(counters["cache-misses"])

    def test_nearest_rank(self) -> None:
        values = [1, 2, 3, 4, 5]
        self.assertEqual(locality_probe.nearest_rank(values, 50), 3.0)
        self.assertEqual(locality_probe.nearest_rank(values, 95), 5.0)

    def test_read_cache_topology(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            index = root / "index3"
            index.mkdir()
            (index / "level").write_text("3\n", encoding="utf-8")
            (index / "type").write_text("Unified\n", encoding="utf-8")
            (index / "size").write_text("32M\n", encoding="utf-8")
            (index / "shared_cpu_list").write_text("0-7\n", encoding="utf-8")
            (index / "ways_of_associativity").write_text("16\n", encoding="utf-8")
            (index / "coherency_line_size").write_text("64\n", encoding="utf-8")
            topology = locality_probe.read_cache_topology(root)
        self.assertEqual(len(topology), 1)
        self.assertEqual(topology[0]["size_bytes"], 32 * 1024 * 1024)
        self.assertEqual(topology[0]["shared_cpu_list"], "0-7")


if __name__ == "__main__":
    unittest.main()
