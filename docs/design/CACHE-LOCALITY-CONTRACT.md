# Cache-Locality Contract

**Status**: design hypothesis with an executable measurement lane
**Scope**: SilkSurf native foreground mode
**Authority**: `perf/locality-budget.json`, `scripts/locality_probe.py`, and retained workload measurements

SilkSurf treats a browser as a bounded set of cooperating state machines rather
than one permanently hot monolith. The shell owns chrome, profiles, permissions,
view routing, and frame composition. A content engine owns one page's loading,
JavaScript, DOM, style, layout, and paint state. Brokers mediate network, files,
and other privileged resources. A frame plane transfers pixels independently
from the control plane.

This separation preserves security boundaries while keeping the foreground path
small. Process count, crate count, executable size, RSS, and cache residency are
different quantities and are measured separately.

## Meaning of cache-local

The 32 MiB objective applies to a latency-critical working set, not to the
installed browser, mapped address space, total RSS, decoded media, or every open
view.

The initial hypothesis is:

- target LLC capacity: 32 MiB;
- target foreground hot code and data: 20 MiB;
- interference and associativity slack: 12 MiB.

A working set equal to nominal cache capacity is not a residency design. Set
conflicts, prefetching, kernel activity, other processes, and shared-core users
consume effective capacity. The 20 MiB target therefore leaves more than one
third of a 32 MiB LLC outside the foreground budget.

No current measurement proves that SilkSurf meets this target. The values in
`perf/locality-budget.json` are hypotheses and remain outside `make full` until a
controlled sweep establishes host-specific and cross-host thresholds.

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
- engine protocol v1 separates small control messages from future shared frame
  buffers;
- `EventIngress` drains asynchronous events on its own thread and fails closed
  when the count-bounded queue overflows.

These mechanisms reduce work and allocation. They do not by themselves prove a
bounded working set or low cache-miss rate.

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
one foreground native worker and freezes or serializes inactive native views.
Future isolation assigns processes by measured trust and failure boundaries,
with a bounded worker pool and explicit eviction. A compatibility engine remains
an opt-in process outside the native locality contract.

## Worker ownership and thread shape

The worker preserves cache affinity by giving `BrowserPageRuntime` one owner.
The next navigation slice uses this shape:

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

The runtime actor serializes every event. It replaces the proposed second
stdout producer rather than adding a third event-writer thread. Background
fetch work may run elsewhere, but completion returns to the runtime actor before
DOM, JavaScript, layout, or paint state changes.

The shell's `EventIngress` reader thread remains the single owner of the child
event pipe. The winit thread drains typed events without blocking.

## Queue and control-plane bounds

A count bound alone does not bound memory. `EVENT_QUEUE_DEPTH = 256` can retain
many large `FrameReady`, `UrlChanged`, or `TitleChanged` events, each legal near
the protocol's message or string limit. Before worker-owned navigation emits
those events, ingress gains a wire-byte budget in addition to its count budget.

The first byte-budget rule is:

- one protocol-maximum envelope always fits;
- cumulative queued wire bytes never exceed one protocol-maximum envelope;
- reservation occurs before queue insertion;
- dequeue releases the exact recorded wire-byte charge;
- overflow records a typed cause before the sender disconnects;
- the supervisor terminates the worker on the next transport operation.

This is a correctness and locality bound, not a statement that 1 MiB control
messages are desirable. Later field-specific limits may reduce URLs, titles,
status strings, and damage metadata independently.

Control events also follow coalescing rules:

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

The pool size follows measured compositor release latency. Four buffers are not
a universal rule. The smallest pool that avoids producer stalls under the
selected presenter is the correct pool.

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
specific path.

## JavaScript boundary

`silksurf-js` delegates ECMAScript execution to `boa_engine`. The current
retained evidence shows interactive React commits are much cheaper than initial
multi-megabyte bundle evaluation. Native mode therefore optimizes foreground
responsiveness and bounded state before peak JavaScript benchmark throughput.

A future compiler tier follows two constraints:

- generated code has a hard code-cache budget and eviction policy;
- the interpreter remains the bounded fallback.

No optimizing JIT belongs on the critical path until bundle traces demonstrate
that its additional code and metadata reduce total misses and interaction cost.

Native AI-chat mode bypasses production website bundles entirely and uses the
same viewport, retained-rendering, and bounded-state rules.

## Measurement lane

`scripts/locality_probe.py` runs one workload repeatedly and records:

- monotonic wall time;
- maximum RSS;
- minor and major faults;
- voluntary and involuntary context switches;
- Linux cache topology from sysfs;
- cycles, instructions, generic cache references and misses, branches, branch
  misses, migrations, and faults when `perf` is available.

The probe records counter availability and kernel-policy failures rather than
silently dropping them. Generic cache counters do not identify a working-set
size, so controlled sweeps vary active document state and compare the miss-rate
and latency knees against the host's LLC topology.

Initial workloads are:

1. fixed browser-chrome text input;
2. native-engine protocol command/event exchange;
3. AI-chat page input with 96 turns;
4. retained same-box text mutation;
5. structural reconcile that runs fused layout;
6. 100, 1,000, and 10,000-turn native transcript virtualization;
7. worker startup, navigation, frame submission, crash detection, and restart.

Each record names the commit, build profile, CPU topology, governor, competing
load, command, and fixture. A threshold becomes a gate only after repeated
rank-1 measurements establish its variance and falsifiers.

Example:

```sh
python3 scripts/locality_probe.py \
  --name fused-pipeline \
  --repeat 20 \
  --output perf/results/fused-pipeline-locality.json \
  -- cargo run --release -p silksurf-engine --bin bench_pipeline
```

## Sequencing with native-runtime extraction

The locality work changes the next issue #53 slices without changing their
acceptance criteria:

```text
asynchronous EventIngress and bounded shutdown       landed
locality contract and measurement lane               this landing
byte-accounted event backlog                          next transport refinement
worker command reader plus single runtime actor       worker-owned navigation
worker-owned BrowserPageRuntime                       page state extraction
sealed memfd frame pool plus SCM_RIGHTS                frame plane
input, incremental damage, crash recovery             DG-1 completion
controlled working-set and miss-rate sweeps           DG-1 evidence
```

Backend comparison remains useful, but it does not define the default product.
The native engine is the locality-first foreground path. Compatibility engines
are measured fallbacks for sites outside the native coverage envelope.

## Falsifiers

The contract changes when evidence shows any of the following:

- the 20 MiB target has no latency or miss-rate knee on representative hosts;
- process separation duplicates enough private hot state to cost more than the
  security and recovery boundary saves;
- phase-local recomputation beats retained state under the same workload;
- an optional compatibility engine matches the native working set and latency;
- one foreground worker cannot satisfy required trust-domain isolation;
- queue or frame metadata consumes a material fraction of the foreground budget;
- JavaScript code and heap dominate foreground misses after transcript and DOM
  virtualization.

Until those measurements exist, cache-local is an explicit, testable design
hypothesis rather than a marketing claim.
