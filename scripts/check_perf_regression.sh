#!/usr/bin/env bash
# check_perf_regression: compare the two most recent entries in
# perf/history.ndjson and fail if any tracked metric regressed by 5% or
# more. Tracked metrics are the SilkSurf hot-path numbers documented in
# docs/PERFORMANCE.md:
#
#   fused_pipeline_us  -- end-to-end engine pipeline (steady state)
#   css_cache_hit_us   -- CSS cache-hit cost (parser warm path)
#   full_render_us     -- full render time (parse + cascade + layout + paint)
#
# Why 5%: above measurement noise on the local-gate runner (we observe
# ~2-3% jitter run-to-run on bench_pipeline) but tight enough to surface
# real regressions before they compound. Tighter thresholds get noisy;
# looser thresholds let drift through.
#
# Why two entries (not baseline.json): baseline.json is hand-curated for
# release tracking and updates infrequently. history.ndjson is appended
# every CI run, so consecutive comparisons surface regressions the moment
# they land rather than at the next baseline refresh.
#
# Why python3 (not jq or awk): python3 is the most universally available
# of the three and gives us clean JSON parsing + arithmetic in one
# dependency. jq would also work but is not guaranteed installed; awk
# JSON parsing is fragile.
#
# Exit codes:
#   0  no regression (or insufficient history); gate passes
#   1  regression of >=5% on a tracked metric; gate fails
#   2  internal error (missing python3, malformed JSON, etc.)
#
# Wired into local-gate slow pass; safe to run anytime.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
HISTORY="${REPO_ROOT}/perf/history.ndjson"

# Threshold: fail when a metric grows by THIS_FRACTION or more vs prior run.
# Expressed as a fraction (0.05 = 5%). Kept in sync with docs/PERFORMANCE.md.
THRESHOLD_FRACTION="0.05"

if ! command -v python3 >/dev/null 2>&1; then
    echo "check_perf_regression: ERROR: python3 not found on PATH" >&2
    exit 2
fi

if [ ! -f "${HISTORY}" ]; then
    # No history file at all -- this is the first run on a fresh checkout.
    # Emit an advisory and exit 0 so CI doesn't fail on a clean clone.
    echo "check_perf_regression: WARN: ${HISTORY} does not exist; nothing to compare"
    exit 0
fi

# Hand off to python3 for JSON parsing + comparison. We pass the file
# path and threshold as argv to keep the heredoc free of shell expansion
# surprises.
python3 - "${HISTORY}" "${THRESHOLD_FRACTION}" <<'PY'
import json
import sys
from pathlib import Path

# Metrics to compare (key in JSON, human-readable label, unit).
TRACKED = [
    ("fused_pipeline_us", "fused_pipeline_us", "us"),
    ("css_cache_hit_us",  "css_cache_hit_us",  "us"),
    ("full_render_us",    "full_render_us",    "us"),
]

history_path = Path(sys.argv[1])
threshold = float(sys.argv[2])

# Read all non-blank lines. ndjson means one JSON object per line; blanks
# (trailing newline, accidental gaps) are tolerated to keep the file
# robust to manual edits.
entries = []
for raw in history_path.read_text(encoding="utf-8").splitlines():
    line = raw.strip()
    if not line:
        continue
    try:
        entries.append(json.loads(line))
    except json.JSONDecodeError as exc:
        print(f"check_perf_regression: ERROR: malformed JSON line: {exc}",
              file=sys.stderr)
        sys.exit(2)

if len(entries) < 2:
    # Insufficient history -- one entry means no prior to compare against;
    # zero means the file exists but is empty (the placeholder created by
    # P3.S2). Either way, advise and pass.
    print(f"check_perf_regression: WARN: only {len(entries)} entry/entries "
          f"in {history_path.name}; need >=2 to compare. Skipping.")
    sys.exit(0)

latest, previous = entries[-1], entries[-2]

# Pull metrics. They may live at the top level OR nested under
# "metrics" -- accept both shapes so the script keeps working as
# perf/append_history.py (P3.S3) firms up the schema.
def get_metric(entry, key):
    if key in entry:
        return entry[key]
    if "metrics" in entry and isinstance(entry["metrics"], dict):
        return entry["metrics"].get(key)
    return None

regressions = []
checked = 0
for key, label, unit in TRACKED:
    prev_val = get_metric(previous, key)
    cur_val  = get_metric(latest, key)
    if prev_val is None or cur_val is None:
        # Metric missing from one of the two entries -- skip silently.
        # During the perf schema bring-up not every entry will carry every
        # field; we don't want to spam warnings.
        continue
    try:
        prev_f = float(prev_val)
        cur_f  = float(cur_val)
    except (TypeError, ValueError):
        print(f"check_perf_regression: WARN: non-numeric {label} "
              f"(prev={prev_val!r}, cur={cur_val!r}); skipping",
              file=sys.stderr)
        continue
    checked += 1
    if prev_f <= 0:
        # Defensive: division by zero / negative baselines are nonsense
        # for timing metrics. Flag and skip rather than mask.
        print(f"check_perf_regression: WARN: non-positive baseline for "
              f"{label} (prev={prev_f}); skipping", file=sys.stderr)
        continue
    delta_frac = (cur_f - prev_f) / prev_f
    if delta_frac >= threshold:
        pct = delta_frac * 100.0
        regressions.append(
            f"REGRESSION: {label} {prev_f:g} -> {cur_f:g} {unit} "
            f"(+{pct:.1f}%)"
        )

if regressions:
    for line in regressions:
        print(line)
    print()
    print(f"check_perf_regression: FAIL ({len(regressions)} metric(s) "
          f"regressed by >={threshold*100:.0f}%)")
    sys.exit(1)

if checked == 0:
    # Two entries exist but neither carries any tracked metric in a form
    # we recognise. Treat as advisory pass; the schema work in P3.S2/S3
    # will populate these fields.
    print("check_perf_regression: WARN: no tracked metrics found in last "
          "two entries; nothing compared")
    sys.exit(0)

print(f"perf OK ({checked} metric(s) within {threshold*100:.0f}% tolerance)")
sys.exit(0)
PY
