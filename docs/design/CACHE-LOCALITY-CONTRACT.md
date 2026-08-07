# Cache-Locality Contract

**Status**: adaptive design hypothesis with an executable measurement lane
**Scope**: SilkSurf native foreground mode
**Authority**: `perf/locality-budget.json`, `scripts/locality_probe.py`, and retained workload measurements

SilkSurf treats a browser as a bounded set of cooperating state machines rather
than one permanently hot monolith. The shell owns chrome, profiles, permissions,
view routing, and frame composition. A content engine owns one page's loading,
JavaScript, DOM, style, layout, and paint state. Brokers mediate network, files,
and other privileged resources. A frame plane transfers pixels independently
from the control plane.

This separation preserves security and recovery boundaries while keeping the
foreground path small. Process count, crate count, executable size, mapped
bytes, RSS, private dirty memory, and cache residency are different quantities
and are measured separately.

## Cache capacity is an input

SilkSurf has no fixed 32 MiB cache requirement. The earlier 20 MiB hot-set on a
32 MiB LLC formulation came from a product concept, not from a measured knee,
and therefore does not govern implementation.

Cache-local means that the runtime discovers or records the effective cache
capacity available to the foreground workload, measures how its latency and
miss behavior scale, and selects the smallest execution mode that preserves
responsiveness. A nominal LLC size from sysfs is an upper bound, not an exclusive
allocation. Affinity, chiplet topology, cache allocation technology, resctrl,
virtualization, other threads, the kernel, and competing processes all reduce
or fragment the effective share.

The measurement lane uses 8, 16, 32, 64, and 96 MiB as experimental points.
They are sweep coordinates, not product classes or gates. A different host or
workload may place its useful knee elsewhere.

## Adaptive execution modes

The native engine has three locality modes.

### Whole-pipeline mode

The measured foreground knee plus an interference reserve fits the effective
cache share. Parser, style, layout, paint, scheduler, and active-view state may
remain retained together when retention costs less than recomputation.

### Phase-local mode

The whole foreground set does not fit, but one pipeline phase and its live
inputs do. Parse, style, layout, and paint displace one another deliberately.
Derived state is retained only when a measurement shows that it reduces total
misses, bandwidth, and interaction latency.

### Streaming mode

Even a phase-local set exceeds the effective cache share or memory-pressure
policy. Traversals become chunked, inactive state is serialized or discarded,
memoization shrinks, and the visible viewport remains the bounded unit of work.

Mode selection is currently a measured design rule rather than an implemented
runtime switch. No mode threshold enters `make full` until controlled sweeps
establish its variance and falsifiers.

## Covered and excluded state

The native foreground contract covers:

- browser-chrome event and damage paths;
- one active native-engine view;
- HTML, CSS, layout, paint, scheduler, and control-plane hot state;
- a bounded JavaScript runtime core used by the active page;
- metadata needed to advance and present the visible viewport.

The contract excludes:

- decoded images, audio, and video payloads;
- GPU textures and compositor-owned surfaces;
- disk and operating-system page caches;
- inactive views after freezing or serialization;
- large page JavaScript heaps and WebAssembly memories;
- optional WPE, Wry, Servo, or CEF compatibility processes.

Excluded state is not free. It remains subject to RSS, bandwidth, eviction, and
lifecycle measurements; it simply does not define whether the foreground
pipeline itself is cache-local.

Compatibility backends receive their own measurements. Their presence does not
weaken or redefine the native-mode contract.

## Existing locality mechanisms

The current tree already contains mechanisms that fit this model:

- `CascadeView` stores selector input in a compact structure-of-arrays view;
- `FusedWorkspace` reuses topology, style, layout, and paint scratch;
- DOM generations skip rebuilds when structure and selector inputs stay stable;
- retained text repaint updates a display item without a full layout pass;
- viewport-backed rasterization keeps full-document pixels out of the active
  frame;
- Wayland retained buffers preserve clean pixels across bounded damage;
- engine protocol v1 separates control messages from shared frame buffers;
- `EventIngress` drains asynchronous events on its own thread and fails closed
  when its count or wire-byte budget overflows.

These mechanisms reduce work and allocation. They do not by themselves prove a
bounded working set or a useful cache-residency knee.

## Process model

The default native topology stays deliberately small:

```text
shell process
    chrome, profiles, permissions, view routing, frame composition
        |
        | protocol v1 control plane
        v
foreground native-engine process
    one runtime actor owning the active BrowserPageRuntime
        |
        | sealed shared frame buffers
        v
shell presenter
```

Re-executing the same binary maps the same file-backed text pages, which the
kernel may share physically between processes. Mutable heaps remain private and
must not be mirrored across the boundary. The shell stores view metadata and
frame handles; the engine alone stores DOM, JavaScript, style, layout, display
list, and raster scratch.

One process per tab is not the default policy. The first functional shell uses
one resident foreground native view. Inactive native views freeze, serialize,
or move to a bounded worker pool only after measured trust and restoration
requirements justify the additional resident state. A compatibility engine is
an opt-in process outside the native locality contract.

## Worker ownership and thread shape

`BrowserPageRuntime` has one owner. The navigation path uses this shape:

```text
blocking command-reader thread
        |
        v
bounded command queue
        |
        v
runtime actor and sole stdout event writer
    BrowserPageRuntime
    host callbacks
    navigation completion
    damage production
```

The command reader keeps `Stop` and `Shutdown` readable while a network load is
in flight. Fetch work may run elsewhere, but completion returns to the runtime
actor before DOM, JavaScript, layout, or paint state changes. The runtime actor
serializes every event, which prevents multiple producers from interleaving
protocol envelopes and avoids another event-writer queue.

The shell's `EventIngress` thread remains the single owner of the child event
pipe. The winit thread drains typed events without blocking.

## Queue and control-plane bounds

A count bound alone does not bound memory. `EventIngress` therefore carries an
exact encoded-wire charge with every queued event. Reservation occurs before
insertion, dequeue releases the exact charge, rejected sends undo their
reservation, and overflow records a typed cause before disconnection.

`EVENT_QUEUE_BYTE_BUDGET` equals one protocol-maximum envelope. That value is a
liveness floor: it is the smallest budget under which every decodable message
can cross. It is not a cache-locality target. Fifteen maximum-size title events
can consume the budget while the 256-entry count queue still has room.

Worker-owned navigation produces the first real event-size distribution. That
evidence determines whether field-specific limits, coalescing, or a smaller
backlog improve locality without rejecting legitimate traffic.

Control events follow these intended coalescing rules:

- a newer title, URL, cursor, status, progress, or metrics sample supersedes an
  older undelivered value for the same view;
- load-state transitions retain order;
- crash, permission, download, and file-chooser events are never coalesced;
- damage rectangles merge when their union costs less than another metadata
  entry;
- excessive damage metadata collapses to one full-viewport rectangle.

## Frame-plane contract

Frame bytes stay outside the control-plane working set.

The native engine writes the visible viewport directly into a sealed memfd from
a bounded reusable pool. A Unix-domain socketpair transfers the descriptor with
`SCM_RIGHTS`; `FrameReady` carries only the connection-local token, generation,
length, and bounded damage. The shell maps and presents that storage without an
intermediate full-frame copy, then returns ownership with `ReleaseFrame`.

Pool size follows measured compositor release latency and the selected locality
mode. Four buffers are not a universal rule. The smallest pool that avoids
producer stalls under the active presenter is the correct pool.

Full-document pixel buffers stay prohibited. Document height remains metadata
for scrolling and hit testing; pixels cover only the visible viewport and
bounded overscan.

## Data-layout rules

Hot traversals choose representations from access patterns:

- stable integer identifiers over pointer graphs;
- structure-of-arrays when a pass reads a small field subset across many nodes;
- array-of-structures when one operation consumes most fields of one object;
- interned names and compact values where comparison dominates;
- retained arenas and reusable scratch where lifetimes match page or frame;
- regional invalidation over global recomputation;
- bounded caches with named eviction and accounting;
- no persistent background task or extension context without a measured budget.

A smaller source type or binary is not automatically a smaller working set.
Measurements decide whether compression, recomputation, or retention wins for a
specific path and cache capacity.

## JavaScript boundary

`silksurf-js` delegates ECMAScript execution to `boa_engine`. Retained evidence
shows interactive React commits are much cheaper than initial multi-megabyte
bundle evaluation. Native mode therefore optimizes foreground responsiveness
and bounded state before peak JavaScript benchmark throughput.

A future compiler tier follows two constraints:

- generated code has a hard code-cache budget and eviction policy selected from
  the effective cache share;
- the interpreter remains the bounded fallback.

No optimizing JIT belongs on the critical path until bundle traces demonstrate
that its additional code and metadata reduce total misses and interaction cost.
Native AI-chat mode bypasses production website bundles and uses the same
viewport, retained-rendering, and bounded-state rules.

## Measurement lane

`scripts/locality_probe.py` runs one workload repeatedly and records:

- monotonic wall time and workload exit status;
- maximum RSS;
- minor and major faults;
- voluntary and involuntary context switches;
- Linux cache topology from sysfs;
- cycles, instructions, generic cache references and misses, branches, branch
  misses, migrations, and faults when `perf` exposes them.

The probe records counter availability and kernel-policy failures rather than
silently dropping them. Privilege modifiers are stripped only for derivation
lookup, so `cycles` and `cycles:u` produce the same record shape. Scheduler
events under a user-only modifier are recorded as unobservable instead of false
zeros; rusage carries context-switch counts.

The first valid `bench_pipeline` run used the required `parallel-render` feature.
Five runs on the development host recorded median IPC 1.47, generic cache-miss
ratio 0.197, maximum RSS 10,184 KiB, median elapsed time 998 ms, and about 9.9
billion instructions. The host reports a 96 MiB LLC. Those values validate the
measurement lane and bound that workload on that host; they do not locate a
capacity knee.

Generic cache counters do not identify working-set size. Controlled sweeps vary
active state and effective cache allocation, then compare latency and miss-rate
knees against the observed cache topology. A 96 MiB host can participate in the
sweep, but it cannot by itself establish behavior at 8, 16, or 32 MiB. Those
points require hosts with those capacities or a verified cache partition.

Initial workloads are:

1. fixed browser-chrome text input;
2. native-engine protocol command/event exchange;
3. AI-chat page input with 96 turns;
4. retained same-box text mutation;
5. structural reconcile that runs fused layout;
6. 100, 1,000, and 10,000-turn native transcript virtualization;
7. worker startup, navigation, frame submission, crash detection, and restart.

Each retained record names the commit, build profile, cache topology, governor,
affinity, competing load, command, fixture, and counter availability. Those
facts come from `scripts/measurement_environment.py`, whose `capture()` output
`perf/measurement-environment.schema.json` defines; the conformance scorecards
and `perf/history.ndjson` embed the same object under the same key, so one
record shape describes every measurement the repository publishes. A workload
run that exits nonzero marks the record failed and makes the probe exit nonzero.
A mode threshold becomes a gate only after repeated rank-1 measurements
establish its variance and falsifiers.

One host cannot separate host-specific behavior from general behavior. The
development host reports a 96 MiB last-level cache, so it cannot establish
behavior at the 8, 16, or 32 MiB sweep coordinates
`perf/locality-budget.json` lists;
`docs/findings/measurement-provenance-envelope.md` records that as the standing
limit on every capacity claim.

Example, against a binary built before the probe so the record measures the
workload rather than Cargo:

```sh
cargo build --release -p silksurf-engine --bin bench_pipeline \
  --features parallel-render
python3 scripts/locality_probe.py \
  --name fused-pipeline \
  --repeat 20 \
  --output perf/results/fused-pipeline-locality.json \
  -- ./target/release/bench_pipeline
```

## Sequencing with native-runtime extraction

The cache-adaptive work changes implementation choices without changing issue
#53's acceptance criteria:

```text
asynchronous EventIngress and bounded shutdown       landed
event-queue wire-byte accounting                      landed
cache-adaptive locality contract                      this landing
worker command reader plus single runtime actor       worker-owned navigation
worker-owned BrowserPageRuntime                       page state extraction
sealed memfd frame pool plus SCM_RIGHTS                frame plane
input, incremental damage, crash recovery             DG-1 completion
controlled capacity and state-size sweeps             mode-selection evidence
```

Backend comparison remains useful, but it does not define the default product.
The native engine is the locality-first foreground path. Compatibility engines
are measured fallbacks for sites outside the native coverage envelope.

## Falsifiers

The contract changes when evidence shows any of the following:

- no stable latency or miss-rate knee appears as effective cache capacity varies;
- a single adaptive mode performs as well as all three across representative
  hosts and workloads;
- process separation duplicates enough private hot state to cost more than its
  security and recovery boundary saves;
- phase-local recomputation loses to retained state at every measured capacity;
- an optional compatibility engine matches the native working set and latency;
- one foreground worker cannot satisfy required trust-domain isolation;
- queue or frame metadata consumes a material fraction of the measured knee;
- JavaScript code and heap dominate foreground misses after transcript and DOM
  virtualization.

Cache-local remains an explicit, testable, capacity-adaptive design hypothesis.
A marketing cache number never becomes an implementation invariant without a
measured mechanism behind it.
