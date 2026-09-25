#!/usr/bin/env python3
"""Render Phase 6 hyperfine exports into published benchmark docs (C6).

Reads hyperfine export JSON files (phase6-n1.json, phase6-n20.json,
phase6-n100.json as written by phase6_hyperfine.sh --export-dir) and
produces a machine-readable results.json snapshot plus a markdown
results section spliced into the benchmarks docs page between
BENCHMARK-RESULTS markers. Standard library only.
"""

from __future__ import annotations

import argparse
import datetime
import json
import sys
from pathlib import Path

BEGIN_MARKER = "<!-- BENCHMARK-RESULTS-BEGIN -->"
END_MARKER = "<!-- BENCHMARK-RESULTS-END -->"
SCHEMA_VERSION = 1
SCALES = (1, 20, 100)


def parse_hyperfine_export(path):
    """Extract murphy/rubocop mean+stddev (seconds) from one export file."""
    try:
        payload = json.loads(Path(path).read_text())
    except (OSError, ValueError) as exc:
        raise ValueError("cannot read hyperfine export %s: %s" % (path, exc))
    results = payload.get("results")
    if not isinstance(results, list) or not results:
        raise ValueError("hyperfine export %s has no results array" % (path,))
    entry = {}
    for item in results:
        command = str(item.get("command", ""))
        key = None
        if "murphy" in command:
            key = "murphy"
        elif "rubocop" in command:
            key = "rubocop"
        if key is None or key in entry:
            continue
        try:
            entry[key] = {"mean": float(item["mean"]), "stddev": float(item["stddev"])}
        except (KeyError, TypeError, ValueError) as exc:
            raise ValueError("entry %r lacks mean/stddev: %s" % (command, exc))
    if "murphy" not in entry or "rubocop" not in entry:
        raise ValueError("export %s needs one murphy and one rubocop result" % (path,))
    return {
        "murphy_mean": entry["murphy"]["mean"],
        "murphy_stddev": entry["murphy"]["stddev"],
        "rubocop_mean": entry["rubocop"]["mean"],
        "rubocop_stddev": entry["rubocop"]["stddev"],
    }


def build_report(input_dir, commit, runner, timestamp):
    """Combine per-scale exports into a versioned report dict."""
    scales = {}
    for scale in SCALES:
        export = Path(input_dir) / ("phase6-n%d.json" % scale)
        if not export.is_file():
            raise ValueError("missing hyperfine export: %s" % (export,))
        parsed = parse_hyperfine_export(export)
        murphy_mean = parsed["murphy_mean"]
        rubocop_mean = parsed["rubocop_mean"]
        speedup = (rubocop_mean / murphy_mean) if murphy_mean > 0 else 0.0
        scales[str(scale)] = {
            "murphy_mean_s": murphy_mean,
            "murphy_stddev_s": parsed["murphy_stddev"],
            "rubocop_mean_s": rubocop_mean,
            "rubocop_stddev_s": parsed["rubocop_stddev"],
            "speedup": speedup,
        }
    return {
        "schema_version": SCHEMA_VERSION,
        "updated_at": timestamp,
        "commit": commit,
        "runner": runner,
        "method": {
            "tool": "hyperfine",
            "warmup": 2,
            "corpus": "crates/murphy-cli/tests/fixtures/builtin_only_project",
            "scales": list(SCALES),
        },
        "scales": scales,
    }


def fmt_seconds(value):
    if value >= 10:
        return "%.1fs" % value
    if value >= 1:
        return "%.2fs" % value
    return "%dms" % round(value * 1000)


def render_markdown(report):
    """Render the docs-page results section for a report."""
    plusminus = chr(177)
    times = chr(215)
    lines = [
        "Last updated: %s (commit `%s`, %s)." % (
            report["updated_at"], report["commit"], report["runner"]),
        "",
        "| Files (N) | murphy (mean) | RuboCop (mean) | Speedup |",
        "| --- | --- | --- | --- |",
    ]
    for scale in report["method"]["scales"]:
        row = report["scales"][str(scale)]
        lines.append(
            "| %s | %s (%s%s) | %s (%s%s) | %.1f%s |" % (
                scale,
                fmt_seconds(row["murphy_mean_s"]), plusminus,
                fmt_seconds(row["murphy_stddev_s"]),
                fmt_seconds(row["rubocop_mean_s"]), plusminus,
                fmt_seconds(row["rubocop_stddev_s"]),
                row["speedup"], times,
            )
        )
    lines += [
        "",
        "Means with standard deviation over hyperfine runs (--warmup 2).",
        "Higher speedup is better for murphy. Raw exports are kept as CI",
        "artifacts; docs/benchmarks/results.json holds the machine-readable snapshot.",
    ]
    return "\n".join(lines) + "\n"


def splice_markers(page, section):
    """Replace the marker-delimited region of a docs page."""
    begin = page.find(BEGIN_MARKER)
    end = page.find(END_MARKER)
    if begin == -1 or end == -1 or end < begin:
        raise ValueError("docs page lacks BENCHMARK-RESULTS markers")
    head = page[: begin + len(BEGIN_MARKER)]
    tail = page[end:]
    return "%s\n\n%s\n%s" % (head, section, tail)


def utc_now_iso():
    return (
        datetime.datetime.now(datetime.timezone.utc)
        .replace(microsecond=0)
        .isoformat()
        .replace("+00:00", "Z")
    )


def main(argv=None):
    parser = argparse.ArgumentParser(description="Render benchmark docs (C6).")
    parser.add_argument("--input-dir", required=True)
    parser.add_argument("--out-json", required=True)
    parser.add_argument("--docs-page", required=True)
    parser.add_argument("--commit", default="unknown")
    parser.add_argument("--runner", default="ubuntu-latest")
    parser.add_argument("--timestamp", default=None)
    args = parser.parse_args(argv)
    try:
        report = build_report(
            args.input_dir,
            commit=args.commit,
            runner=args.runner,
            timestamp=args.timestamp or utc_now_iso(),
        )
    except ValueError as exc:
        print("error: %s" % (exc,), file=sys.stderr)
        return 2
    out_json = Path(args.out_json)
    out_json.parent.mkdir(parents=True, exist_ok=True)
    out_json.write_text(json.dumps(report, indent=2) + "\n")
    try:
        page = Path(args.docs_page).read_text()
    except OSError as exc:
        print("error: cannot read docs page: %s" % (exc,), file=sys.stderr)
        return 2
    try:
        updated = splice_markers(page, render_markdown(report))
    except ValueError as exc:
        print("error: %s" % (exc,), file=sys.stderr)
        return 2
    Path(args.docs_page).write_text(updated)
    print("benchmark docs updated: %s + %s" % (args.out_json, args.docs_page))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
