# React Interaction-to-Commit Latency over the JS/DOM Bridge

**Date**: 2026-07-16
**Mechanism**: `silksurf-js/src/bin/bundle_probe.rs --click-repeat n`
dispatches n trusted clicks at a React 18 useState counter
(`silksurf-js/probes/react_counter_click.js`) and times each cycle from
`dispatch_dom_event` through the host-scheduler pump that lets React
commit, asserting the committed DOM text (`clicks:{n}`) after every
click.
**Question**: what does one user interaction cost once the bundle is
loaded -- the keystroke/click -> setState -> reconcile -> commit path
that docs/findings/boa-react-bundle-throughput.md left unmeasured?

## Verdict

One committed interaction costs well under a millisecond at the median:
over 100 consecutive clicks, dispatch-to-committed-DOM latency is
**p50 0.76 ms, p95 1.15 ms, p99 2.41 ms, max 3.37 ms** (min 0.68,
mean 0.84, stdev 0.36). No warm-up drift: the first ten clicks mean
0.86 ms against 0.84 ms for the rest. Every one of the 100 commits
reached the Dom (the per-click check gates each sample).

Interaction latency therefore does not pressure the local-spa rung;
initial bundle evaluation does. The same session's 20-run eval sweep
puts boa's parse+execute at 0.4-0.9 MB/s medians under a busy host
(data below), so a framework page pays its cost up front, then runs
interactions two orders of magnitude inside the 100 ms
perceptible-response budget.

A second observation rides the contrast between the two metrics:
bundle eval is memory-bound while the interaction path is not. Between
a load-22 sweep and the CPU-weight-isolated sweep, boa eval medians
moved by tens of percent and V8's did too (shared L3 and DRAM
contention from a ~10-thread competitor), yet click-latency medians
moved under 10 percent (0.84 -> 0.76 ms). The commit path's working
set fits in cache; the eval path's AST construction does not.

## Method

- The timed section covers synthetic click dispatch (capture/target/
  bubble walk, React's delegated listener), the microtask drain, and
  host-callback pumping until the queues go idle; React's commit lands
  inside it. Target resolution (`getElementById`) runs before the clock
  starts.
- The pump spins without sleeping (`pump_host_queues(ctx, true)`): the
  stock 1 ms sleep granularity would dominate sub-millisecond samples.
- Every iteration asserts the committed count in
  `document.body.textContent` before the next click, so a sample is
  only recorded for a commit that actually reached the Dom.
- Order statistics (nearest-rank) over all samples; no samples
  discarded. Warm-up is reported, not trimmed.

## Environment

- CPU: AMD Ryzen 5 5600X3D (6c/12t, L3 96 MiB), cpufreq governor
  `performance`, boost disabled (stable clocks).
- Kernel 7.1.3-2-cachyos; rustc 1.94.1; boa_engine 0.21.1 release
  build; node v22.23.1 (V8 baseline).
- Load discipline: the host carried a persistent ~10-thread competing
  workload (1-minute load 23 at measurement start; the exact figures
  ride in data/measurement-host.txt). Probe processes ran inside a
  `systemd-run --user --scope -p CPUWeight=10000` cgroup, which wins
  CPU-time contention (~100x default weight) but cannot shield the
  shared L3 and DRAM from the competitor. Absolute eval numbers are
  therefore upper bounds; a quiet-host rerun is a named falsifier.
  Interaction latency proved insensitive to this (see Verdict), so its
  numbers stand as measured.
- Bundles pinned by sha256 (`d949f1...` react 18.3.1 UMD, `35f4f9...`
  react-dom 18.3.1 UMD); not vendored (no network in tests).

## Data

Interaction latency, 100 consecutive clicks, each commit asserted:

| metric | ms |
|---|---|
| min | 0.681 |
| p50 | 0.762 |
| mean | 0.843 |
| p90 | 0.900 |
| p95 | 1.153 |
| p99 | 2.407 |
| max | 3.368 |
| stdev | 0.360 |

Eval-throughput sweep, 20 runs per engine per bundle, order statistics
(nearest rank), same session and isolation. boa times ride
bundle_probe's own eval_ms (parse+execute inside a live context); V8
times ride hrtime around `vm.runInContext` in node 22 with self/window
aliases. react-dom evaluates after react in one shared context on both
engines. Neither includes process or context startup.

| bundle | bytes | boa p50 (min-p95) ms | V8 p50 (min-p95) ms | p50 ratio | boa MB/s |
|---|---|---|---|---|---|
| react 18.3.1 | 10 751 | 11.5 (6.4-19.3) | 1.1 (1.0-1.6) | 10.2x | 0.93 |
| react-dom 18.3.1 | 131 835 | 247.5 (156.3-295.8) | 9.2 (8.2-12.6) | 26.9x | 0.53 |
| moment 2.30.1 | 58 890 | 75.9 (33.9-103.1) | 5.5 (4.9-9.7) | 13.9x | 0.78 |
| lodash 4.17.21 | 73 015 | 200.1 (87.9-242.0) | 15.5 (12.5-72.7) | 12.9x | 0.36 |

These medians run 2.5-3x the single-run values recorded on 2026-07-12
in docs/findings/boa-react-bundle-throughput.md while V8 moved far
less; candidate mechanisms are the competing workload's L3/DRAM
pressure (boa's AST-walking eval is allocation-heavy and memory-bound)
and the host's disabled CPU boost at sweep time. The 2026-07-12
numbers stay the quiet-host reference; this sweep bounds the busy-host
case and supplies the first distribution data.

Raw per-click samples: `docs/findings/data/react-interaction-commit-latency.csv`.
Eval-throughput sweep (same session): `docs/findings/data/boa-eval-throughput-sweep.csv`.

## Falsifiers and scope

- The probe drives a synthetic Dom through `dispatch_dom_event`; the
  running-app path (GUI input synthesis in
  `crates/silksurf-app/src/js_events.rs`, dirty-node paint, native
  present) is a separate evidence class, proven 2026-07-17 in
  `docs/findings/local-spa-click-repaint-gui-probe.md` (a synthesized
  Wayland-surface click drives a damage-frame repaint at
  input_to_present ~100 us).
- One component, one state hook, a two-node commit. List
  reconciliation with keys, attribute-only updates, and deep trees
  scale the reconcile phase; the running-app fused relayout for a keyed
  reorder, an attribute rewrite, and a subtree replace is proven
  2026-07-18 in
  `docs/findings/local-spa-fused-reconcile-gui-probe.md` (mode Full at
  input_to_present ~190-260 us).
- The spin pump burns a core while waiting; on the GUI path the winit
  wake deadline replaces it, so app-observed latency adds scheduler
  and present time on top of these numbers.

## Reproducer

```sh
cargo build --release -p silksurf-js --bin bundle_probe
# fetch react/react-dom 18.3.1 production UMD from unpkg (hashes above), then:
target/release/bundle_probe --shared --dom --pump --click inc --click-repeat 100 \
  react.production.min.js react-dom.production.min.js \
  'silksurf-js/probes/react_counter_click.js=if (document.body.textContent.indexOf("clicks:{n}") < 0) throw 0'
```
