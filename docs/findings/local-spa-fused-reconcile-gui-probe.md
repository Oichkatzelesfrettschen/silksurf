# Local-SPA Fused-Relayout Reconcile Through the Running App

**Date**: 2026-07-18
**Mechanism**: `scripts/gui_probe.sh --probe page-reconcile` builds
`silksurf-app`, serves one of three hermetic reconcile fixtures, opens the
native window on a live Wayland surface, and synthesizes one trusted
`PrimaryClick { x: 200, y: 200 }`. The click travels `dispatch_native_click`
(`crates/silksurf-app/src/js_events.rs`) into the page's JS handler, which
performs a mutation that escapes the retained text-only fast path: an element
attribute rewrite, a keyed list reorder, or a subtree replace.
`repaint_runtime_dirty_nodes` (`crates/silksurf-app/src/runtime_repaint.rs`)
rejects the fast path, reruns the fused style/layout/paint pipeline, and the app
presents a full frame.
**Question**: does a layout-affecting click mutation drive a *visible repaint*
through the running app's fused relayout branch, closing the reconcile cases
the retained text-only rung
(`docs/findings/local-spa-click-repaint-gui-probe.md`) left open?

## Verdict

The three fused-relayout reconcile cases are closed on the running-app evidence
class. A synthesized GUI click drives each mutation through the full app path --
winit input synthesis, native-click dispatch into the JS handler, DOM mutation,
fused style/layout/paint, and native present -- and the changed page reaches the
screen as a full frame. All three cases pass three consecutive runs and the app
self-exits.

The dividing mechanism is `repaint_runtime_dirty_nodes`. Its retained fast path
(`repaint_runtime_text_only_dirty_nodes`) returns a result only when every dirty
node is a same-geometry text node. An element attribute change, a reordered list
parent, and a replaced subtree parent are not same-geometry text nodes, so
control falls through to the fused branch. There `dirty_nodes_damage_rect`
(`crates/silksurf-app/src/redraw_geometry.rs`) returns `None` on the first
non-text, non-input dirty node, so the layout-affecting mutation presents
`BrowserRedrawMode::Full`. The new `Runtime fused repaint: dirty_nodes=N
mode=Full` trace names the branch, and the probe asserts it fires while the
`Runtime text repaint` marker does not -- the load-bearing proof that the click
routed through fused relayout, not the fast path.

This is a distinct evidence class from both the bridge findings and the retained
text-only rung. The bridge findings
(`docs/findings/boa-react-bundle-throughput.md`,
`docs/findings/react-interaction-commit-latency.md`) drive a synthetic DOM and
read committed DOM text. The text-only rung drives the real winit surface but
stays on the retained damage-rect fast path. This finding drives the real winit
surface through the fused relayout that a keyed reorder, attribute update, or
subtree swap requires. The DOM-mutation correctness is carried by the bridge
findings; this rung proves the running app relays out and presents the result.

## Method

- Each fixture parks a full-width tap button at the top of the page body, below
  the 44 px chrome strip, so window coordinate (200, 200) lands inside it and
  `hit_test_event_target` resolves the button. The three fixtures differ only in
  what the JS `click` handler mutates.
  - **Attribute**: the handler rewrites `card.className` from `card` to
    `card expanded`, a rule that changes the element height and background. One
    element node dirties.
  - **Keyed reorder**: the handler detaches the first list item and reappends it
    at the end. `append_child` rejects an already-parented node, so the handler
    removes before it reappends; the list order permutes. Three nodes dirty.
  - **Subtree replace**: the handler assigns `panel.innerHTML`. `inner_html_set`
    removes the existing children and reparses the fragment into fresh element
    nodes. Nine nodes dirty.
- The assertion (`check_page_reconcile_probe_log`) requires the synthesized
  `PrimaryClick`, the `Runtime fused repaint` marker, and `mode=Full`, and it
  fails if the `Runtime text repaint` fast-path marker appears. The runtime-text
  probe asserts the inverse -- fast path present, fused marker absent -- so the
  two probes fence the branch selector from both sides.
- The probe exits from inside the app after the full frame presents; a hang is
  caught by the 60 s per-run timeout as a non-clean exit.

## Environment

- CPU: AMD Ryzen 5 5600X3D (6c/12t), kernel 7.1.3-2-cachyos, release build.
- Display: live Wayland surface (`wayland-0`), presenter auto, window 1280x330.
- Load: near-idle host (1-minute load ~1 at run time), so the timings below are
  a quiet-host steady state.

## Data

Three runs per case, each the full frame for the click's committed repaint.
`input_to_present` covers the click event through the presented frame; `draw` is
the CPU repaint work; `render` is the viewport raster. Times are avg (min-max)
in microseconds.

| case | mutation | dirty_nodes | input_to_present | draw | render |
|---|---|---|---|---|---|
| attribute | className rewrite | 1 | 194.1 (178.1-225.8) | 61.7 (57.6-69.4) | 54.9 (52.2-59.8) |
| keyed reorder | remove + append item | 3 | 241.0 (220.3-275.6) | 72.5 (69.1-79.3) | 65.2 (63.7-68.2) |
| subtree replace | innerHTML reparse | 9 | 260.9 (205.4-299.3) | 73.9 (60.6-85.2) | 62.4 (54.8-68.2) |

The fused relayout presents a full frame, so it carries no damage rect; the
causal link rides the `Runtime fused repaint: dirty_nodes=N` trace plus the DOM
state, with N tracking each mutation's structural footprint (1, 3, 9). The CPU
`draw` here is ~55-74 us against the retained text-only path's ~16 us in
`docs/findings/local-spa-click-repaint-gui-probe.md`: a full-viewport raster
plus a fused style/layout/paint rerun costs several times the retained
same-width text write, which is the reason the fast path exists.

## Falsifiers and scope

- The mutations that route here are layout-affecting by construction. A
  same-width text write stays on the retained fast path (proven separately); an
  input `value` edit reaches the fused branch but presents a damage rect through
  the `is_editable_input_node` allowance in `dirty_nodes_damage_rect`, a
  different sub-case than the `mode=Full` proven here.
- `input_to_present` is the full-viewport present cost at window 1280x330.
  Larger viewports and deeper trees scale `render` and `draw`.
- This finding proves the fused repaint path over three runs per case and
  reports frame timing, not a latency distribution. A per-click GUI latency
  sweep on the fused path is separate follow-up work, as it was for the
  text-only rung.

## Reproducer

```sh
make gui-probe-attr-reconcile
make gui-probe-reorder-reconcile
make gui-probe-subtree-reconcile
# or directly, e.g.:
scripts/gui_probe.sh --release --backend auto --presenter auto \
  --fixture subtree-reconcile --probe page-reconcile --runs 3 --timeout-seconds 60
```
