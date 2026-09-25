#!/usr/bin/env python3
"""Unit tests for scripts/perf/render_benchmarks.py (C6). Standard library only."""

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import render_benchmarks as rb


def fake_export(murphy_mean, murphy_std, rubocop_mean, rubocop_std):
    return {
        "results": [
            {"command": "murphy lint x", "mean": murphy_mean, "stddev": murphy_std},
            {"command": "rubocop --format json x", "mean": rubocop_mean, "stddev": rubocop_std},
        ]
    }


class ParseTest(unittest.TestCase):
    def test_parses_both_commands(self):
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "e.json"
            p.write_text(json.dumps(fake_export(0.5, 0.05, 2.0, 0.2)))
            got = rb.parse_hyperfine_export(p)
        self.assertAlmostEqual(got["murphy_mean"], 0.5)
        self.assertAlmostEqual(got["rubocop_mean"], 2.0)

    def test_missing_rubocop_rejected(self):
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "e.json"
            p.write_text(json.dumps({"results": [{"command": "murphy lint x", "mean": 1.0, "stddev": 0.1}]}))
            with self.assertRaises(ValueError):
                rb.parse_hyperfine_export(p)

    def test_empty_results_rejected(self):
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "e.json"
            p.write_text(json.dumps({"results": []}))
            with self.assertRaises(ValueError):
                rb.parse_hyperfine_export(p)

    def test_broken_json_rejected(self):
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "e.json"
            p.write_text("{not json")
            with self.assertRaises(ValueError):
                rb.parse_hyperfine_export(p)


class ReportTest(unittest.TestCase):
    def write_inputs(self, d):
        for n, mm, rm in ((1, 0.5, 2.0), (20, 1.0, 8.0), (100, 5.0, 60.0)):
            (Path(d) / ("phase6-n%d.json" % n)).write_text(
                json.dumps(fake_export(mm, mm / 10, rm, rm / 10)))
        return d

    def test_speedup_math(self):
        with tempfile.TemporaryDirectory() as d:
            report = rb.build_report(
                self.write_inputs(d), commit="abc", runner="r", timestamp="t")
        self.assertAlmostEqual(report["scales"]["1"]["speedup"], 4.0)
        self.assertAlmostEqual(report["scales"]["20"]["speedup"], 8.0)
        self.assertAlmostEqual(report["scales"]["100"]["speedup"], 12.0)
        self.assertEqual(report["schema_version"], 1)
        self.assertEqual(report["commit"], "abc")

    def test_missing_scale_rejected(self):
        with tempfile.TemporaryDirectory() as d:
            with self.assertRaises(ValueError):
                rb.build_report(d, commit="c", runner="r", timestamp="t")

    def test_markdown_table(self):
        with tempfile.TemporaryDirectory() as d:
            report = rb.build_report(
                self.write_inputs(d), commit="abc", runner="r", timestamp="t")
        md = rb.render_markdown(report)
        self.assertIn("| 1 |", md)
        self.assertIn("| 20 |", md)
        self.assertIn("| 100 |", md)
        self.assertIn("4.0", md)
        self.assertIn("abc", md)

    def test_splice_roundtrip(self):
        page = "head\n%s\nmiddle\n%s\ntail\n" % (rb.BEGIN_MARKER, rb.END_MARKER)
        out = rb.splice_markers(page, "SECTION\n")
        self.assertIn("SECTION", out)
        self.assertIn(rb.BEGIN_MARKER, out)
        self.assertIn(rb.END_MARKER, out)
        self.assertIn("tail", out)
        # second splice replaces, never duplicates
        out2 = rb.splice_markers(out, "NEW\n")
        self.assertIn("NEW", out2)
        self.assertNotIn("SECTION", out2)

    def test_splice_without_markers_rejected(self):
        with self.assertRaises(ValueError):
            rb.splice_markers("no markers here\n", "x\n")

    def test_main_end_to_end(self):
        with tempfile.TemporaryDirectory() as d:
            self.write_inputs(d)
            out_json = str(Path(d) / "out" / "results.json")
            page = Path(d) / "bench.md"
            page.write_text("t\n%s\nSTALE-PLACEHOLDER\n%s\n" % (rb.BEGIN_MARKER, rb.END_MARKER))
            rc = rb.main(["--input-dir", d, "--out-json", out_json,
                          "--docs-page", str(page), "--commit", "deadbee",
                          "--runner", "test-runner", "--timestamp", "2026-01-01T00:00:00Z"])
            self.assertEqual(rc, 0)
            saved = json.loads(Path(out_json).read_text())
            self.assertEqual(saved["commit"], "deadbee")
            body = page.read_text()
            self.assertIn("deadbee", body)
            self.assertNotIn("STALE-PLACEHOLDER", body)


if __name__ == "__main__":
    unittest.main()
