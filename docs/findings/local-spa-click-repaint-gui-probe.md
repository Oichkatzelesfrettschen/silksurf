# Local-SPA Click-to-Repaint Through the Running App

**Date**: 2026-07-17
**Mechanism**: `scripts/gui_probe.sh --fixture page-click --probe page-click`
builds `silksurf-app`, serves a hermetic counter page, opens the native
window on a live Wayland surface, and synthesizes one trusted
`PrimaryClick { x: 200, y: 200 }` through the winit event loop. The click
travels `dispatch_native_click` (`crates/silksurf-app/src/js_events.rs`):
hit-test the tap target, dispatch `mousedown` -> `mouseup` -> `click`, run the
page's JS `click` handler, mutate the counter text node, and repaint the
dirty node. The app presents a `Damage(Rect)` frame carrying the new text.
**Question**: does a trusted click drive a *visible repaint* through the
running app, closing the gap the bridge-level result
(`docs/findings/react-interaction-commit-latency.md`) left open as
local-spa-rung-gui-probe?

## Verdict

The local-spa rung is closed on the running-app evidence class. A synthesized
GUI click drives the full app path -- winit input synthesis, native-click
dispatch into the JS handler, DOM text mutation, retained dirty-node repaint,
and native present -- and the mutated counter reaches the screen as a damage
frame, not merely as a Dom text change. Three consecutive runs pass and the
app self-exits after the repaint presents (`gui_probe: page click repaint OK`
x3, exit 0).

This is a distinct evidence class from the bridge result. The bridge finding
drives a synthetic Dom through `dispatch_dom_event` and reads committed Dom
text; this finding drives the real winit surface, so the click passes through
GUI hit-testing, the `mousedown/mouseup/click` sequence, `repaint_runtime_
dirty_nodes`, and softbuffer/Wayland present. The two together span the
interaction path from event synthesis to pixels.

The fixture is plain JavaScript, not React: React's production UMD needs a
network fetch, and tests carry no network. React's own reconcile-to-text-
commit is already proven at the bridge in
`docs/findings/boa-react-bundle-throughput.md`; a hermetic
`addEventListener('click', ...)` handler that assigns `firstChild.textContent`
exercises the identical DOM mutation the app repaints. The GUI probe isolates
the app repaint path, not the framework.

## Method

- The tap target is a full-width 320 px button at the top of the page body,
  below the 44 px chrome strip, so window coordinate (200, 200) lands inside
  it and `hit_test_event_target` resolves the button.
- The handler assigns a fixed-width counter (`clicks:0` -> `clicks:1`, both
  eight characters). Same-width text keeps the mutation on the retained
  text-only repaint fast path (`repaint_runtime_text_only_dirty_nodes`), so
  the app emits `Runtime text repaint: dirty_nodes=1` and presents a damage
  rect without a fused relayout.
- The assertion (`check_page_click_probe_log`) requires three log markers in
  one run: the synthesized `PrimaryClick`, the `dirty_nodes=1` retained
  repaint, and a `mode Damage(Rect` present. Absence of any one fails the
  run.
- The probe exits from inside the app after the damage frame presents
  (`exit_frame_delay: 0`); a hang would be caught by the 60 s per-run
  timeout as a non-clean exit.

## Environment

- CPU: AMD Ryzen 5 5600X3D (6c/12t), kernel 7.1.3-2-cachyos, release build.
- Display: live Wayland surface (`/run/user/1000/wayland-0`), presenter auto,
  window 1280x457.
- Load: busy host (1-minute load ~12 at run time). The path is proven
  regardless of load; the timings below are an upper bound, not a floor.

## Data

Three runs, each the damage frame for the click's committed repaint:

| run | input_to_present | draw | render | buffer | damage rect |
|---|---|---|---|---|---|
| 1 | 99.32 us | 15.47 us | 11.16 us | 4.23 us | (143, 386, 26, 48) |
| 2 | 100.07 us | 15.75 us | 10.76 us | 4.91 us | (143, 386, 26, 48) |
| 3 | 104.08 us | 16.43 us | 11.18 us | 5.18 us | (143, 386, 26, 48) |

The damage rect is one line of counter text (26x48 px). `input_to_present`
covers the click event through the presented frame, including the JS handler,
retained repaint, and Wayland present. The CPU repaint work (`draw`) is
~16 us; the rest is buffer acquisition and present scheduling.

The ~100 us figure is steady-state, reproduced across seven fresh app
processes (three `--runs 3` passes plus four standalone `--runs 1` launches,
all 98-104 us; each `run_probe_once` spawns a new process, so none is warmed
by a prior run). The very first launch of a freshly linked binary presented
its first click frame at ~3 ms -- cold page cache for the new executable and
first Wayland buffer allocation -- and that cost did not recur on any
subsequent launch. The reported datum is the reproducible steady state, not
the one-time cold-surface bring-up.

## Falsifiers and scope

- One click, one text node, same-width mutation: the retained fast path. A
  keyed list reorder, an attribute-only update, and a subtree replace route
  through the fused-layout branch of `repaint_runtime_dirty_nodes`; each is
  proven 2026-07-18 on its own probe page in
  `docs/findings/local-spa-fused-reconcile-gui-probe.md` (mode Full at
  input_to_present ~190-260 us).
- `input_to_present` is the minimal-damage case (one text line). Larger
  damage rects add raster area to `render` and `draw`.
- The bridge finding times 100 dispatch-to-commit cycles with order
  statistics; this finding proves the app repaint path over three runs and
  reports its frame timing, not a latency distribution. A per-click GUI
  latency sweep on this path is separate follow-up work.

## Reproducer

```sh
make gui-probe-page-click
# or directly:
scripts/gui_probe.sh --release --backend auto --presenter auto \
  --fixture page-click --probe page-click --runs 3 --timeout-seconds 60
```
