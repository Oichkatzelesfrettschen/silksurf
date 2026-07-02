# Performance Guide

This doc consolidates non-JS and JS performance guidance. JS-specific details
are expanded in `docs/JS_ENGINE.md`.

## Goals
- CPU: prioritize lowest cycles and cache misses, allow larger binaries.
- Memory: target <26 MB RSS, stretch goal <10 MB.
- Determinism: stable timings across builds; guardrails in CI/local runs.

## Fused Pipeline Results (2026-04-13, 50-node benchmark DOM, 13 CSS rules)

The fused pipeline (`FusedWorkspace::run()`) performs style cascade, layout, and
display-list construction in a single BFS pass. Steady-state re-render on an
unchanged DOM: **9.5us** (rebuild skipped via generation check).

| Metric | Value | Notes |
|--------|-------|-------|
| ws.run() cold (fresh DOM) | 11.3-11.6us | Includes table+view rebuild |
| ws.run() warm (same DOM) | ~9.5us | Rebuild skipped via generation check |
| 3-pass baseline | ~22us | compute_styles + layout + display list |
| Speedup vs 3-pass | 2.0x | |
| Per-node cost | ~190ns (600 cycles @ 3GHz) | Hash, match, cascade, layout |

### Architecture (SoA cascade path)

The cascade hot path operates entirely on the CascadeView SoA layout.
No `dom.node()` (168 bytes) or `dom.attributes()` calls during cascade.

| Component | Size | Cache lines | Purpose |
|-----------|------|------------|---------|
| Node (AoS, avoided) | 168B | 2.6 | Full DOM node with topology |
| CascadeEntry (SoA) | 40B | 0.6 | tag + id_index + class_start/count + parent_id |
| SelectorIdent | 32B | 0.5 | SmolStr + Option<Atom>, pre-constructed |
| ComputedStyle | 264B | 4.1 | Full computed style (stack alloc) |

Key optimizations applied (in dependency order):
1. **FusedWorkspace** -- single-object reusable scratch for all pipeline state
2. **LayoutNeighborTable** -- flat BFS-level decomposition, rebuild() reuses capacity
3. **CascadeWorkspace** -- bitvec seen (Fix D), workspace class_keys (Fix 2)
4. **SmolStr font_family** -- ComputedStyle::default() zero-heap-alloc (Fix 1)
5. **Pre-resolved class_strings** -- set_attribute populates SmallStrings (Fix 3)
6. **Fused tag+id+class** -- single dom.node() call per node (Fix F)
7. **Monotonic resolve table** -- lock-free Atom resolution, materialized at phase boundaries
8. **CascadeView SoA** -- 40-byte per-node entry, flat SelectorIdent array
9. **Zero-alloc matches_selector** -- reverse index arithmetic, no Vec allocation
10. **CascadeView in matching** -- tag/id/class/parent from SoA, not 168-byte Node
11. **Generation-gated rebuild** -- skip table+view rebuild on unchanged DOM
12. **Static FALLBACK** -- LazyLock<ComputedStyle> eliminates per-node default construction

### Phase boundaries (DOM lifecycle)

The DOM operates in strictly phased mode:
- **Parse phase**: TreeBuilder calls set_attribute (interner RwLock, write path)
- **Materialize**: into_dom() builds resolve_table + increments generation
- **Render phase**: cascade reads CascadeView + resolve_fast() (lock-free, read-only)
- **Mutate**: with_mutation_batch() allows new atoms via RwLock (cold path)
- **Re-materialize**: end_mutation_batch() extends resolve_table + increments generation

FusedWorkspace detects DOM changes via `Dom::generation()` (unique instance ID +
mutation counter). Same-DOM re-renders skip table.rebuild() + cascade_view.rebuild().

### Remaining cost breakdown (50 nodes, cold path)

| Component | Cost | % of total |
|-----------|------|-----------|
| table.rebuild() | ~1.0us | 9% |
| cascade_view.rebuild() | ~1.0us | 9% |
| Cascade (hash + match + apply) | ~7.5us | 65% |
| Layout math | ~1.5us | 13% |
| Display list push | ~0.5us | 4% |

The cascade algorithm (7.5us) is now instruction-bound, not memory-bound.
Further compression would require JIT compilation of CSS selectors.

## Hot Paths (Summary)
DOM/HTML:
- HTML tokenizer (`crates/silksurf-html/src/lib.rs`): delimiter-first scans,
  memchr fast paths, character reference decoding.
- Tree builder (`crates/silksurf-html/src/tree_builder.rs`): batch node creation.
- DOM mutation (`crates/silksurf-dom/src/lib.rs`): batching + dirty-node flush.

CSS:
- Cascade view (`crates/silksurf-css/src/cascade_view.rs`): SoA materialized view.
- Selector matching (`crates/silksurf-css/src/matching.rs`): CascadeView-accelerated.
- Cascade (`crates/silksurf-css/src/style.rs`): indexed rule buckets + bitvec dedup.

Layout/Render:
- Fused pipeline (`crates/silksurf-engine/src/fused_pipeline.rs`): single BFS pass.
- Raster fill (`crates/silksurf-render/src/lib.rs`): SIMD row fill.

JS:
- Lexer/parser/VM/GC (see `docs/JS_ENGINE.md`).

## GUI Presenter Results (2026-06-30, example.com address probe)

`scripts/gui_probe.sh` drives the winit GUI, focuses the address bar, types two
ASCII characters, waits for the final input frame to present, and records app
render, buffer, and input-to-present timings separately.

| Backend | Configuration | Final render avg | Final total avg | Max input buffer | Max input-to-present | Result |
|---------|---------------|-----------------:|----------------:|-----------------:|---------------------:|--------|
| X11 softbuffer | `--release --backend x11 --runs 5` | 5.276us | 10.472us | 2.451ms | 2.474ms | Final text-input render reaches the 0.01ms target envelope, but one focus-frame buffer wait exposes softbuffer tail latency. |
| Wayland softbuffer | `--release --backend wayland --presenter softbuffer --runs 3` | 10.507us | 19.300us | 7.110us | 18.660ms | Presenter pacing avoids `buffer_mut()` stalls but makes input-to-present miss the target by orders of magnitude. |
| Wayland SHM | `--release --backend auto --presenter auto --runs 5` | 4.832us | 6.954us | 3.690us | 16.570us | Wayland auto selects SHM. Full-frame preseed and single-flush draw pumping make later address input use warm buffers and keep final text input inside the 0.01ms total CPU envelope. |

The Wayland softbuffer path previously entered a `buffer_mut()` wait of
13.993ms on the middle address input. Presenter-aware urgent redraw handling
removes that blocking call by keeping Wayland softbuffer behind pacing. The SHM
presenter preserves buffer age across same-width one-pixel height jitter
(1280x320 -> 1280x319), which drops final address-text render from hundreds of
microseconds to about 10us.

The four-buffer SHM presenter exposes a second invariant: buffers used for
partial damage must already contain the current full image. A lazy copy on
first input use fixed correctness but put a 1.6MB memory copy in the address
input path, leaving final buffer time around 241us to 284us. The current path
prefers released aged buffers and copies the full image into two released cold
buffers immediately after a full-frame present. That keeps the next focus and
typed-input frames on warm buffers, leaves one unused SHM buffer cold, and
drops final address-input buffer time to 1.550us to 5.480us.

The remaining Wayland bottleneck is no longer the renderer, full-buffer
seeding, or redundant event-pump flushing. SHM final render is below 0.01ms on
this page, final total CPU work averages 6.954us, and final input-to-present
averages 8.536us with a 16.570us worst input frame in the current 5-run probe.
`AddressFocusChrome` redraws only the address border and cursor when the
existing URL text is reusable, bringing focus-frame render into the 4.350us to
4.570us range in the same probe. The SHM event pump drains pending release
events before drawing and leaves the only flush at the committed present. A
diagnostic `SILKSURF_TRACE_SHM_PHASES=1` run shows the focus-frame `pump` at
210ns, render at 5.340us, attach/damage at 1.470us, and flush at 1.170us. The
steady final text frame had `pump` at 80ns, render at 5.060us, attach/damage
at 760ns, and flush at 790ns. The next presenter step is frame-callback or
release-event scheduling to tighten the focus-frame tail without adding
display-frame sleeps.

A post-refactor `ai-chat` address probe on 2026-06-30 keeps the same hot-path
shape after `main` and bitmap glyph lookup cleanup. The final text-input frame
reports 5.49us input-to-present, 1.28us app render, 2.59us buffer work, 3.90us
draw, and 4.00us total CPU work. `/home/eirikr/.local/bin/lizard -l rust -C 16
crates/silksurf-app/src/main.rs crates/silksurf-gui/src/winit_backend.rs`
reports zero threshold warnings for touched Rust functions.

The ChatGPT page-input probe on 2026-07-02 exercises a real cached
`https://chatgpt.com` payload with Wayland SHM. The retained presenter now
marks a buffer as ready only after `write_released_retained_buffer` accepts the
pixels, and the prebuilt focus viewport cache follows the same text-editable
input order as Tab focus. This keeps the first page-focus frame on a retained
buffer instead of re-rasterizing the viewport. The strict run
`scripts/gui_probe.sh --release --backend auto --presenter auto --probe
page-input --timeout-seconds 120 --max-total-ns 10000 --max-focus-total-ns
20000 https://chatgpt.com` reports focus render 0ns, focus buffer 4.100us,
focus total 4.420us, final typed-input render 780ns, final buffer 3.630us, and
final total 4.580us.

## Address Input First Principles

Address input latency decomposes into:

`T_input = T_event + T_state + T_text_metric + T_raster + T_damage + T_present + T_compositor`

The 0.01ms CPU target covers the app-owned terms:

`T_cpu = T_state + T_text_metric + T_raster + T_damage + T_present_submit`

It does not cover network fetch, JavaScript execution, compositor scheduling,
display refresh, or remote webpage response time. Those terms have separate
budgets and separate evidence.

The current address bar is a fixed chrome rectangle with bounded text. The hot
path avoids layout negotiation, heap allocation, full-frame raster work, and
full-surface copies. `AddressFocusChrome` changes only the focus border and
cursor when visible URL text is reusable. `AddressTextChrome` damages only the
address text strip when typing changes text. SHM buffers preserve the full
image across partial damage, so input redraws write only the changed pixels and
submit one bounded damage rectangle.

Crates earn their place only when they shrink one measured term without growing
another term beyond the target. `winit` supplies portable event delivery and
native window handles. `wayland-client`, `memfd`, and `memmap2` supply the
single-copy Wayland SHM presenter. `softbuffer` remains the portable fallback.
`cosmic-text`, `swash`, `skrifa`, `harfrust`, and `fontdb` already cover real
font shaping in the text stack; address-bar ASCII chrome currently uses a tiny
built-in bitmap path because it beats general shaping for the fixed hot path.
`cursor-icon`, `xcursor`, and `wayland-cursor` already arrive through winit and
Smithay client tooling. Cursor movement hit-tests chrome, links, and page
inputs, then asks winit for the native pointer shape without requesting a
redraw. Adding another cursor crate needs a measured cursor cost first.

Address editing now stores a byte-index cursor inside the bounded address
string. ArrowLeft and ArrowRight move the caret, Home and End move it inside
the address bar while editing, typed characters insert at the caret, and
Backspace deletes before the caret. The live Wayland SHM `address-caret` probe
on the AI-chat fixture now coalesces the focus/edit burst and damages only the
full address text strip for replacement and caret motion. The final
middle-insert frame reports 6.340us to 7.680us total CPU work and 1.970us to
2.550us render work across five runs, with no Wayland buffer-busy retry. The
same run reports 11.750us to 14.810us input-to-present because that metric
includes winit dispatch, Wayland acquire, damage/commit flush, and
compositor-visible scheduling. A stricter `--max-any-input-ns 10000` run still
fails on end-to-end presentation time even though app-owned CPU work stays
inside the 0.01ms envelope. An X11 softbuffer falsifier on the same probe
blocks in `buffer_mut()` for about 4.006ms, so Wayland SHM remains the lower
latency presenter on this host.

## AI-Chat Fixture Results (2026-06-30)

`scripts/gui_probe.sh --fixture ai-chat` serves a deterministic local page with
96 chat turns, code blocks, links, form controls, external CSS, and a script
resource. The fixture models the DOM and paint density of AI chat surfaces
without depending on external login state or network timing.

The full-document raster path rendered a 1280x60330 document into a
309114880-byte RGBA buffer. The first three release runs spent 801.053ms,
650.942ms, and 218.854ms in rasterization. Address input still stayed in the
microsecond range, but the page-render path failed the browser target.

The viewport-backed frame keeps document height for scrolling and hit testing,
but stores only the visible chrome plus visible content bitmap. The same
fixture now rasterizes a 4096000-byte viewport buffer. The first three release
runs spent 360.740us, 243.800us, and 255.360us in rasterization. Processing
budget drops from hundreds of milliseconds to 4.596ms to 4.849ms. The remaining
first-render cost is the fused style/layout/paint pass at about 3.85ms to
3.97ms for 1798 styled nodes and 1194 display items.

An O0 Wayland AI-chat smoke probe on 2026-07-02 exposes duplicated viewport
buffer initialization in `rasterize_skia_into`: `Vec::resize` writes
4,096,000 bytes with 0xff and the renderer immediately fills the same buffer
again. `resize_bytes_for_overwrite` reserves the full-raster buffer without
pre-filling it, then the existing renderer fill writes the initialized image.
The traced full-raster resize phase drops from 17.832600ms to 2.380us. The
navigation raster phase drops from 24.377701ms to 6.829842ms, and total O0
navigation build drops from 99.544898ms to 82.849724ms. The remaining O0
first-render terms are fused layout/paint and the 4,096,000 byte
`argb-pack`.

A follow-up O0 Wayland smoke probe on 2026-07-02 routes the supported visible
viewport subset directly into the ARGB presenter buffer. The fast path accepts
opaque solid fills, zero-radius opaque rounded rects, bitmap text, and RGBA
images; gradients, shadows, translucent items, unsupported text, and
antialiased corners keep the tiny-skia RGBA fallback. The AI-chat fixture first
missed only on code punctuation in five visible text items, so the bitmap table
now covers printable code punctuation used by AI chat pages. The traced run
`scripts/gui_probe.sh --o0 --backend auto --presenter auto --fixture ai-chat
--probe smoke --timeout-seconds 30 --trace-app-frame` reports
`argb-direct: 4.789450ms`, `rgba buffer: 0 bytes`, `argb buffer: 4096000
bytes`, and `total: 70.790538ms`. The previous same-day O0 path reported
`raster: 6.724633ms`, `argb-pack: 15.502070ms`, and `total: 88.828181ms`.
The remaining O0 first-navigation budget is dominated by fused
style/layout/paint and the one required ARGB viewport write.

The viewport-backed path is the first tile-cache step. The retained bitmap now
has an origin (`bitmap_scroll_y`) and a bounded height (`bitmap_height`), while
`raster_height` remains the scrollable document height. The next renderer step
is a real tile cache that tracks 256x256 or 512x512 content tiles, reuses clean
tiles across scroll, and submits multi-rect damage instead of refreshing the
whole visible bitmap after DOM mutation.

The chrome now includes bounded back, forward, home, reload, and stop controls
in the left toolbar. The controls reuse the existing history and navigation
worker paths. The overlay derives enabled state from the same history and
pending-navigation predicate that dispatch uses: Back and Forward require
history targets, Home and Reload require an idle navigation worker, and Stop
requires a pending navigation. A 3-run `example.com` probe with the controls
reports final render average 5.727us, final total average 8.270us, worst final
total 10.920us, and worst input-to-present 19.660us. A 3-run `ai-chat`
fixture probe after adding Stop reports final render average 5.073us, final
total average 7.340us, worst final total 7.810us, and worst input-to-present
15.170us. The larger fixture remains dominated by viewport size and fused
layout work.

`scripts/gui_probe.sh --fixture ai-chat --probe chrome` starts on a fixture
subpage, binds Home to the fixture root, and clicks Home, Back, Forward,
Reload, then Back again. The final Back click cannot dispatch until Reload
finishes, so the log must contain exact navigation completions for the fixture
root and subpage before the probe exits. This probe tests chrome behavior and
navigation asynchrony; it is not an address hot-path timing gate.

`scripts/gui_probe.sh --fixture ai-chat --probe stop` types the fixture slow
URL into the address bar, submits it, waits for the loading chrome to present,
and clicks Stop. The probe requires a `Navigation stopped` log and rejects a
slow-page completion before exit. One run reports the Stop chrome frame at
8.240us render, 11.290us total CPU work, and 15.210us input-to-present.

`scripts/gui_probe.sh --fixture ai-chat --probe page-input` focuses the chat
composer textarea through the native event path, types `!`, and requires
`Page input focused` plus `Page input updated` logs. Focused input edits update
the retained text item and rasterize the document damage rect directly into the
viewport buffer. Document display lists keep a 64x64 tile index, so the damage
path selects only items near the input rect instead of scanning the full
1194-item list. Pure content damage no longer repaints chrome. The damage path
also keeps reusable scratch for dirty pixels, tile item indices, and duplicate
item flags.

A 2026-06-30 release probe after the focus redraw fix reports the focus frame
as chrome damage only: `Rect(WinitDamageRect { x: 0, y: 0, width: 1280, height:
44 })`, 4.690us render, 8.920us draw, and 9.140us total CPU work. The typed
page-input frame reports 6.450us render, 9.300us draw, 9.410us total CPU work,
and 56.150us input-to-present. The focus event no longer routes through the
full-document redraw path.

Early page-input probe summaries counted the app-frame diagnostic write inside
the render callback. That made the frame timing measure the probe logger as
well as the browser. `scripts/gui_probe.sh` now keeps app-frame diagnostics
behind `--trace-app-frame`, so the default latency path measures the browser
hot path without stderr writes inside the render callback. A 2026-07-01 live
Wayland `page-input` run over three AI-chat fixture iterations reports the
final 13x22 damage frame at 600ns to 1.250us render, 3.060us to 4.060us total
CPU work, and 27.020us to 30.400us input-to-present. The three-run averages
are 820ns render, 3.630us total CPU work, and 28.280us input-to-present.

The current page-input CPU frame is inside the 0.01ms target when app-frame
diagnostics stay disabled. Rapid address typing can exhaust the four Wayland
SHM buffers when a synthetic probe requires one presented frame per typed
character; one Stop probe typed URL hit an input-to-present stall of 17.878ms
while the redraw CPU work stayed below 19us. A larger eight-buffer SHM ring
only moved that cliff from the fourth typed character to the eighth and spent
memory without solving the compositor-release boundary, so the ring stays at
four buffers. The Stop probe now coalesces URL characters in browser state and
waits only after focus, submit, and stop. Three release runs show no `Wayland
buffers busy` lines; the submit/loading chrome frame reports 8.710us, 8.870us,
and 9.130us total CPU work, and the stop-confirmation frame reports 6.660us,
6.830us, and 6.530us total CPU work. The probe fails if URL-entry burst work
exhausts the SHM ring again.

A 2026-07-01 release `page-input` probe after native cursor mapping reports the
typed input damage frame at 3.360us render, 6.880us draw, 7.010us total CPU
work, and 54.770us input-to-present. Pointer movement updates the native cursor
shape through winit and does not request a redraw. A later 2026-07-01
`--backend auto` run resolves `/run/user/1000/wayland-0`, and the app logs
`configured=Auto resolved=Wayland`. The typed input damage frame reports
3.550us render, 6.810us draw, 6.940us total CPU work, and 52.510us
input-to-present.

A follow-up 2026-07-01 live Wayland `page-input` run after navigation buffer
reuse and Taffy text-measure caching keeps the composer hot path inside the
app-owned 0.01ms CPU target. The three typed-input frames damage 13x22 pixels
and report 630ns to 800ns render, 3.040us to 3.540us draw, and 3.180us to
3.660us total CPU work. The same frames report 23.770us to 28.450us
input-to-present, which stays outside the CPU target because it includes event
delivery and compositor scheduling.

The fixture now puts the composer textarea before toolbar inputs so the same
probe exercises AI-chat composition semantics. The first textarea run exposed a
full-control damage frame: 797x72 pixels, 9.500us render, 13.410us total CPU
work, and 119.080us input-to-present. Focused form-control edits now compute a
changed-suffix text damage rect and keep input `value` attributes separate from
textarea text content. The follow-up live Wayland run damages 13x22 pixels and
reports 670ns render, 3.140us draw, 3.280us total CPU work, and 26.230us
input-to-present.

Textarea Enter now routes through the focused page control instead of the
address submit path. Address Enter still normalizes and navigates the address
bar; textarea Enter appends `\n` to DOM text content. Multiline text damage
tracks the changed line and column. A follow-up live Wayland `page-input` run
still damages 13x22 pixels for composer typing and reports 630ns render,
2.990us draw, 3.120us total CPU work, and 24.720us input-to-present.

`DamageScratch` now records the clipped damage pixels it just painted. The app
packs those scratch pixels directly into the retained ARGB frame and falls back
to retained-RGBA row packing only when scratch metadata is absent. A live
Wayland `page-input` probe over three AI-chat fixture iterations reports the
typed 13x22 damage frame at 690ns to 800ns render, 3.060us to 3.570us draw,
3.180us to 3.680us total CPU work, and 24.400us to 31.610us
input-to-present. The three-run averages are 743ns render, 3.493us total CPU
work, and 26.810us input-to-present.

A follow-up live Wayland `page-input` probe after inline style cascade support
keeps the same 13x22 typed damage rect. The three-run averages are 760ns
render, 3.234us draw, 3.357us total CPU work, and 24.884us
input-to-present.

The AI-chat fixture now loads an external classic script from `/app.js` before
layout. The navigation loader fetches the script through the cache-first
resource path, passes its text into the retained `SilkContext::with_dom`
bridge, and the script mutates `body` before first layout. `scripts/gui_probe.sh
--fixture ai-chat --probe page-input` enables navigation tracing for that
fixture and requires the external script line, `Navigation script 0 done`, and
`Navigation DOM body data-fixture=ai-chat`. A live Wayland three-run probe
records the first `app.js` fetch at 999.052us and later script cache hits at
500ns and 410ns. The bridged script runs in 128.650us to 134.171us, keeps the
typed 13x22 damage frame at 750ns to 900ns render, 3.320us to 4.650us draw,
3.420us to 4.810us total CPU work, and 26.370us to 38.780us
input-to-present. The typed-frame averages are 810ns render, 3.857us draw,
3.983us total CPU work, and 31.047us input-to-present.

The fixture also advertises `/module.js` through `rel="modulepreload"`. The
navigation loader resolves modulepreload links as `rel` tokens, deduplicates
them, and warms the cache through the same parallel resource fetcher used by
stylesheets and classic scripts. `scripts/gui_probe.sh --fixture ai-chat
--probe page-input` requires the `Modulepreload` trace line before accepting
the fixture. A live Wayland three-run probe records the first local
modulepreload fetch at 317.501us and later cache hits at 350ns and 260ns. The
typed 13x22 damage frame stays at 830ns to 1.000us render, 3.110us to 3.930us
draw, 3.230us to 4.040us total CPU work, and 25.550us to 27.480us
input-to-present. The typed-frame averages are 893ns render, 3.480us draw,
3.593us total CPU work, and 26.437us input-to-present.

The Boa DOM bridge now maps `element.src` and `element.innerHTML` writes into
live DOM attributes and text children. The AI-chat fixture creates a script
element at runtime, sets `src` to `/dynamic.js`, writes inline script text, and
appends it to `document.head`. The browser-owned dynamic classic script pass
tracks parser script node IDs, scans dirty nodes for newly reachable classic
scripts, fetches external `src` scripts through an ephemeral renderer, dedupes
dirty script nodes, and evaluates each new script node once in the retained
`SilkContext`. A live Wayland three-run page-input probe records
`/dynamic.js` fetch/execution through `Navigation dynamic script 0.0 done: 62
bytes`, records `Navigation DOM body data-dynamic-script=fetched`, and keeps
the typed 13x22 damage frame at 740ns to 820ns render, 3.000us to 3.490us draw,
3.110us to 3.600us total CPU work, and 25.770us to 27.460us
input-to-present.

Granular navigation tracing localizes the dynamic-script slowdown to repeated
default TLS provider construction. Before the fix, `BasicClient::new()` builds
a fresh rustls root store for the one-shot dynamic renderer; the AI-chat
fixture records `Navigation dynamic phase 0 renderer` at 7.536533ms to
8.267725ms and `Navigation build scripts` at 8.259734ms to 9.153886ms. After
`BasicClient::new()` clones a process-local `OnceLock<Arc<RustlsProvider>>`,
the same live Wayland probe records dynamic renderer construction at 50ns to
110ns, dynamic fetch plus eval at 414.451us to 516.071us, and the full
navigation script phase at 668.031us to 773.151us. The script bucket no longer
contains root-store reload cost; the remaining dynamic cost is local resource
fetch plus Boa eval.

The ARGB presentation packer now uses a runtime-gated SSE2 lane path on
x86/x86_64 and keeps the scalar packer as the portable tail path. A follow-up
live Wayland three-run page-input probe on the same fixture reports the full
4,096,000 byte `argb-pack` at 356.721us, 337.891us, and 430.111us during
initial navigation. The typed 13x22 damage frame stays below the 0.01ms CPU
draw target at 3.450us to 5.290us total CPU work, with 25.420us to 39.340us
input-to-present. This moves the active full-frame bug surface from byte
packing to first-present buffer acquisition, compositor scheduling, and the
remaining RGBA-to-presenter format boundary.

One-line input Enter now submits the nearest ancestor GET or POST form. The
submission path resolves the `action` attribute against the page URL, encodes
successful named input, textarea, checked checkbox, checked radio, and selected
single-select controls through `url::form_urlencoded`, keeps GET navigation
cache-first, and sends POST navigation through an uncached request body.
Unsupported methods update chrome status instead of navigating. The live
Wayland GET `form-submit` probe focuses a page input, types `!`, submits,
navigates to
`/results/?q=silk%21&mode=fast&opt=on&tier=pro&sort=recent`, and completes the
result-page repaint. `PageInputFocus` treats focus as an input-ack damage mode
because page focus does not mutate document pixels. A presenter-retained current
view buffer lets Wayland SHM attach the retained buffer for the focus frame
without running the render callback. Navigation start also prepares a retained
loading-chrome buffer after the previous focused-input frame, and submit
presents only the Reload button, Stop button, and loading status text band. A
2026-07-01 live Wayland GET run reports 4.970us focus total CPU work with 0ns
render work, 4.840us typed total CPU work, and 4.010us submit total CPU work
with 0ns render work; the navigation fetch completes separately in 424.191us.
The live Wayland POST `form-submit` probe navigates to `/posted/`, logs
`Navigation posted`, and the fixture server receives
`q=silk%21&mode=fast&opt=on&tier=pro&sort=recent`; its focus frame reports
8.750us total CPU work with 0ns render work, its typed input frame reports
5.680us total CPU work, and its submit chrome frame reports 7.350us total CPU
work with 0ns render work while the POST fetch completes separately in
561.701us. Page focus, text edit, and submit loading chrome stay inside the
0.01ms CPU target; network completion, compositor scheduling, and result-page
repaint remain separate evidence classes.

Scroll refresh now keeps the retained viewport bitmap when the window height
stays fixed and the scroll delta is smaller than the content rows. The app
copies overlapping ARGB rows, rasterizes only the newly exposed document strip
through the tile-indexed Skia damage path, and presents the visible content
rect without redrawing chrome. The live Wayland `scroll` probe on the AI-chat
fixture resolves to `/run/user/1000/wayland-0` and records `ScrollReuse` for
both directions: a 96px down-scroll frame refreshes in 255.362us with
408.312us input-to-present and 401.952us total CPU work, and a 48px up-scroll
frame refreshes in 163.791us with 291.132us input-to-present and 288.232us
total CPU work.

The follow-up full-width blit fast path keeps the same scroll semantics and
collapses visible-content copies into contiguous slice copies when frame width
equals window width. A live Wayland `scroll` probe on 2026-07-01 records the
96px down-scroll blit at 151.471us, 433.082us input-to-present, and 426.172us
total CPU work; the 48px up-scroll blit lands at 116.160us, 288.121us
input-to-present, and 285.511us total CPU work. The blit is smaller, but the
scroll path still moves full visible content because Wayland damage describes
changed buffer pixels rather than semantic document motion.

Document-coordinate damage now maps through the retained viewport scroll before
copying frame rows into the presenter buffer. The earlier down-scroll blit
under-counted copied rows when document `y` exceeded the viewport height. The
corrected live Wayland `scroll` probe on 2026-07-01 records the 96px
down-scroll refresh at 253.311us, blit at 162.491us, input-to-present at
446.352us, and total CPU work at 439.302us. The 48px up-scroll refresh lands
at 141.510us, blit at 113.001us, input-to-present at 277.231us, and total CPU
work at 274.071us. An age-one presenter-buffer shift experiment copies the
same visible content in two operations and loses to the single contiguous
retained-frame copy, so the code keeps the corrected retained-frame path.

The scratch-pack damage path removes the retained-RGBA readback from scroll
exposed-strip updates. `scripts/gui_probe.sh --probe scroll` now enables the
app-frame trace that its `ScrollReuse` check requires, so the gate is
self-contained. A live Wayland scroll run records the 96px down-scroll refresh
at 265.261us, blit at 187.540us, input-to-present at 485.961us, and total CPU
work at 478.791us. The 48px up-scroll refresh lands at 153.340us, blit at
165.570us, input-to-present at 343.030us, and total CPU work at 339.660us.

An isolated follow-up live Wayland scroll run keeps `ScrollReuse` active and
removes concurrent probe contention from the measurement. The 96px down-scroll
refresh lands at 252.511us, blit at 113.850us, input-to-present at 395.401us,
and total CPU work at 389.211us. The 48px up-scroll refresh lands at
149.620us, blit at 105.821us, input-to-present at 277.951us, and total CPU work
at 275.151us.

The Wayland SHM retained presenter now prepares two scroll viewport buffers
after a presented frame: the common 96px wheel-down target and the 48px
wheel-up target. A matching scroll input swaps the app retained frame to the
prepared viewport and asks the presenter to attach the tagged SHM buffer, so
the input frame skips row movement, exposed-strip rasterization, and presenter
buffer blitting. A live Wayland `scroll` probe on the AI-chat fixture with
`--max-total-ns 20000` passes: the 96px down-scroll reports 0ns render,
5.340us draw, and 12.150us total CPU work; the 48px up-scroll reports 0ns
render, 4.520us draw, and 10.870us total CPU work. The same run reports
14.250us and 9.030us input-to-present, so the remaining cost is Wayland
attach/flush and event-loop overhead rather than browser raster work.

Taffy tree construction was the dominant first-navigation CPU bucket on the
AI-chat fixture. `SILKSURF_TRACE_FUSED=1` split the 2026-07-01 baseline into
15.149026ms `taffy-rebuild`, 3.443128ms `taffy-compute`, and 19.345956ms fused
total for 1806 BFS nodes and 1196 display items. The layout adapter now merges
single direct text children into their parent taffy leaf, then assigns the
skipped text node the parent rect for paint. The same traced fixture records
2.943431ms `taffy-rebuild`, 2.673111ms `taffy-compute`, 6.568255ms fused total,
and 6.782776ms navigation `layout-paint`. The app also uses a runtime-gated
AVX2 RGBA-to-ARGB packer before the existing SSE2 and scalar paths; the same
run records the 4,096,000 byte `argb-pack` at 2.363479ms. These changes move
first navigation closer to the browser target while leaving the typed 13x22
page-input frame at 990ns render and 3.740us total CPU work.

A cold ChatGPT smoke after the taffy compaction and AVX2 packer fetches 571051
HTML bytes in 463.199947ms, runs navigation build work in 10.119866ms, packs
the 4,096,000 byte ARGB frame in 1.430035ms, and presents the first full frame
in 3.587633ms total CPU work. The ChatGPT modulepreload set stays scheduled
after the first build, so large bundles still do not block first presentation.

The browser bitmap now keeps chrome out of the tiny-skia viewport display list.
`rasterize_browser_viewport_into` paints page content, fills the toolbar
background directly in the caller-owned RGBA buffer, and leaves button,
address, and status text to the cheap ARGB overlay path. A 2026-07-01 live
Wayland AI-chat page-input run keeps the final typed page-input frame at
3.300us total CPU work and 28.220us input-to-present. The same run records
632.432us raster, 3.114078ms ARGB pack, and 30.460652ms navigation build total;
layout-paint remains the dominant first-navigation bucket in that sample and
needs the next profiling pass.

Taffy text measurement now keeps four cached constraint results per BFS node
instead of one. The AI-chat fixture drives repeated taffy measure probes for
the same text leaf under alternating width constraints, so a one-entry cache
thrashes. `SILKSURF_TRACE_FUSED=1 SILKSURF_TRACE_TAFFY=1` records cache hits
rising from 407 to 4259, text metric time dropping from 507.552us to
240.712us, `taffy-compute` dropping from 2.537456ms to 2.165696ms, and fused
total dropping from 3.875040ms to 3.471439ms. An eight-entry cache gives the
same hit count on this fixture and costs more memory, so four entries stay the
current low-resource point.

Wayland SHM full-frame preseed now caps post-present cold-buffer seeding at
two released buffers and acquires released aged buffers before virgin buffers.
A traced AI-chat page-input run records first-frame preseed at 1.769412ms
instead of the previous three-buffer 2.669800ms sample. A one-buffer preseed
experiment records 964.301us first-frame preseed but regresses final typed
input to 721.811us total CPU work because the second partial frame acquires a
virgin buffer and copies the full viewport. The two-buffer policy keeps the
three-run normal page-input gate inside the 0.01ms target: final typed total
CPU work is 3.580us to 4.150us, final render is 770ns to 840ns, and
input-to-present is 27.070us to 27.720us.

Wayland SHM now warms one released virgin buffer after a presented frame. The
copy runs outside the measured redraw callback, so later tiny damage frames do
not seed a full viewport on the hot path. The failing AI-chat address probe
previously hit a final address frame with 562.771us to 718.202us in the SHM
seed phase and 580.301us to 733.872us total CPU work. After idle warming and
short address-text clear spans, the five-run address gate reports final typed
CPU work at 2.890us to 3.690us, final render at 730ns to 1.080us, and
input-to-present at 4.790us to 5.600us. A five-buffer SHM experiment moves the
address-caret compositor-release stall from caret movement to the final insert
and still records about 13ms max input-to-present, so the four-buffer policy
stays the lower-memory point while buffer release pacing remains a compositor
scheduling surface.

The first full-frame app render is dominated by the retained ARGB frame copy
into the presenter buffer. `--trace-app-frame` on the AI-chat smoke records
the full-frame blit at 1.175472ms and chrome overlay at 6.900us, so chrome is
not the first-frame bottleneck. Replacing the same-size `copy_from_slice` path
with an explicit `copy_nonoverlapping` helper regresses the same smoke to
1.240312ms blit and 1.282912ms render, so the standard slice copy remains the
current fastest measured retained-frame path. A coherent-RGBA experiment packs
the retained RGBA buffer directly into the mapped SHM presenter instead of
copying the retained ARGB frame. Two follow-up smokes record 1.112303ms and
1.213233ms full-frame app blits, so direct packing into the SHM mapping does
not establish a stable win over the slice-copy path. The live AI-chat
page-input gate still reports final typed CPU work at 3.590us to 3.820us after
the experiment, which keeps the hot input path healthy but leaves first paint
dominated by writing a full 4,096,000 byte presenter buffer. The next real
first-frame step is rendering directly into a presenter-owned buffer with
retained-frame ownership changed around that buffer, or reducing the first
paint to visible tiles so a full-viewport SHM write is not required.

## Crate Sweep (2026-06-30)

Current dependencies already cover the low-latency browser spine: `winit` for
events, `softbuffer` for fallback pixels, `wayland-client` plus `memmap2` for
SHM presentation, `taffy` for layout, `html5ever` through the HTML crate,
`cosmic-text` plus `swash` and `skrifa` for text, and `tiny-skia` for CPU
anti-aliased paint.

Cargo and upstream documentation checks on 2026-07-02 confirm the current
external candidates: `cosmic-text` 0.19.0, `vello_cpu`, `vello`,
`smithay-client-toolkit` 0.20.0, `ropey` 1.6.1, and `crop` 0.4.3. These crates
remain evaluation surfaces unless a measured gap needs their mechanism. They
do not remove the current scroll copy by themselves.

Candidate decisions:

| Area | Crate | Decision | Reason |
|------|-------|----------|--------|
| GPU/vector renderer | `vello` | Evaluate behind a feature | It targets large 2D scenes through `wgpu`, but it adds GPU setup, shaders, and surface integration. Use after CPU viewport tiles expose a stable display-list boundary. |
| CPU vector renderer | `vello_cpu` | Study before adding | It targets fast CPU 2D rendering, but current softbuffer integration can require a separate render buffer instead of direct writes into the mapped presentation buffer. Use it as a benchmark and tile-design reference before adding it to the runtime path. |
| Presenter-retained viewport | current Wayland SHM backend | Implemented for focus-scroll and wheel scroll | ChatGPT focus-scroll and AI-chat wheel scroll pass the 20us CPU gate through tagged Wayland SHM buffers prepared before the input event. A new crate does not improve this path until tile invalidation, compositor scheduling, or non-Wayland fallback becomes the measured bottleneck. |
| Web compositor | `webrender` | Study, do not add yet | It is a web renderer with display-list and tile concepts, but the dependency and GL/Freetype surface is too large for the current embedded profile. Mine concepts first. |
| Browser-engine reference | `blitz-dom` | Study, do not vendor | It combines DOM, Taffy, accessibility, SVG, fonts, and custom widgets. Use as architecture input while keeping Silksurf's smaller crate boundaries. |
| CSS parser | `lightningcss` | Evaluate for conformance path | It parses browser CSS structures and could close CSS grammar gaps, but the current custom CSS pipeline is smaller and already hot-path tuned. |
| Content encoding | `flate2`, `brotli-decompressor` | Added behind `silksurf-net/content-encoding` | Real sites commonly serve compressed HTML, CSS, and JS. `flate2` was already in the lockfile; `brotli-decompressor` adds only `alloc-no-stdlib` and `alloc-stdlib`. The HTTP/1.1 parser decodes chunked transfer bodies before content decoding, then the feature decodes `br`, `gzip`, and `deflate` and strips stale entity headers. |
| HTTP/2 client | `h2`, `hyper`, `reqwest` | Keep direct `h2` and fix protocol edges | `h2` is the narrow HTTP/2 mechanism under `silksurf-net`, now refreshed to 0.4.15. `hyper` supplies a broader correct HTTP stack and `reqwest` supplies a higher-level client, but both widen the runtime surface before a direct protocol edge proves it needs another layer. The current H2 collector drains response bodies concurrently so large multiplexed module batches keep returning flow-control credit instead of falling back to HTTP/1.1. |
| WebSocket transport | `tokio-tungstenite` | Added in `silksurf-net` and exposed through `silksurf-js` | AI sites need bidirectional transport after page bootstrap. The net crate exposes a blocking text roundtrip backed by a current-thread Tokio runtime, and the Boa host installs a `WebSocket` object with constructor, `send`, `close`, `onmessage`, and `onerror`. Native-root TLS avoids a second static webpki root bundle. Persistent async sockets remain separate work. |
| JavaScript modules | `boa_engine`, `rquickjs`, `oxc_parser`, `swc_ecma_parser` | Keep Boa, execute bounded module graphs | Boa is already the embedded JS engine. `rquickjs` would add a second runtime boundary, while Oxc and SWC are parser/compiler surfaces rather than browser module execution. The current slice collects `rel="modulepreload"`, external `type="module"` roots, and inline-module static imports, then warms small static `import` / `export ... from` graph edges in the background with URL, round, and scan-byte caps. Boa `MapModuleLoader` executes small fetched external graphs during navigation while large real-site bundle sets stay on the background warm path. |
| Still images | `png`, `zune-jpeg`, `image-webp` | Added as `silksurf-image` plus image display items | Real pages need PNG, JPEG, and WebP before image layout and paint are meaningful. `png` already exists through the render stack, `zune-jpeg` keeps JPEG narrow, and `image-webp` adds pure-Rust WebP decode with only `byteorder-lite` and `quick-error` below it. The app resolves `<img src>`, fetches image resources, decodes them to RGBA8, attaches `ImageSurface` display items, and rasterizes them through a clipped nearest-neighbor software blit. |
| Accessibility | `accesskit` | Added behind `silksurf-app/accessibility` | The app builds a semantic AccessKit snapshot for browser chrome, address input, status, links, and page inputs. The feature is default-off, so the tiny default runtime and dependency tree stay unchanged. Native AT-SPI or platform adapter wiring remains separate from the semantic tree. |
| Clipboard | `arboard` | Added for address-bar copy, cut, and paste | Address-bar editing needs OS clipboard integration. The dependency disables default image features and enables Wayland data-control support, so the app imports text clipboard behavior without image clipboard machinery. Clipboard commands sit outside the per-character hot path. |
| Keyboard event model | `keyboard-types` | Add when DOM keyboard events leave chrome-local input | Browser pages need `KeyboardEvent` fields that match web semantics. Keep chrome input on the current winit path, then use `keyboard-types` at the DOM event boundary. |
| Address suggestion matcher | `nucleo` or `nucleo-matcher` | Add only for history/search suggestions | Fuzzy matching belongs in the background suggestion list, not the per-character URL mutation hot path. Keep direct address edits as bounded `String` operations and feed snapshots to a worker when history search lands. |
| Text editing buffer | `crop` or `ropey` | Do not add for address input | The address bar uses short single-line text where `String` edits stay cheaper and smaller. Revisit for textarea, history search, and page editor controls. |
| Cursor icon type | `cursor-icon` | Do not add directly | `winit` already carries cursor icon support. `CursorMoved` maps chrome buttons and links to `Pointer`, address and page inputs to `Text`, and empty page space to `Default` without redraw. |
| Rich text layout | `parley` | Do not add to the current text path | `cosmic-text` already provides shaping and glyph rasterization for this software renderer. Study Parley when editable rich text or advanced inline layout becomes the limiting conformance surface. |
| GPU text renderer | `glyphon` | Do not add to the software path | It targets `wgpu` text rendering. Use only inside a future GPU compositor feature. |
| Wayland toolkit loop | `smithay-client-toolkit` or `calloop` | Do not add to the current app loop | `winit` already owns input and display selection. Add only if the Wayland SHM presenter becomes a full native event-loop backend. |
| Memory cache | `quick_cache` | Added for decoded image surfaces | It provides byte-weighted eviction with one direct crate. The app keeps decoded image payloads behind `Arc<[u8]>`, deduplicates image URLs before fetch, and uses a fixed 64 MiB decoded-resource budget. |
| Disk cache | `cacache` | Evaluate after HTTP cache policy | It fits content-addressed resource storage, but it adds async/runtime and mmap surface. HTTP semantics come first. |
| Conformance | `wpt` tooling | Use for runner integration | Web Platform Tests provide broad browser conformance pressure. Start with parser, DOM, CSS, and layout subsets before full browser automation. |

No extra glyph or cursor crate is justified by the current measurements.
`cosmic-text` 0.19.0 is current on Cargo and already brings the shaping stack.
The lockfile resolves `swash` 0.2.7 through `cosmic-text`; crates.io also
publishes 0.2.9, which remains a dependency-refresh check rather than a direct
Silksurf dependency.
Address chrome uses a fixed bitmap path for the hot input loop. Page text uses
the existing shaping stack. Cursor crates already arrive through the windowing
stack.

Boa `MapModuleLoader` now executes bounded external module graphs during
navigation when the document exposes at most four module roots and the fetched
graph stays below 512 KiB. The AI-chat fixture proves `/module.js` imports
`/module-child.js` and mutates `body[data-module-graph=module-child]` before
layout. A live Wayland `page-input` probe records the module execution phase at
178.601us and keeps the typed input frame at 770ns render, 3.660us total CPU
work, and 28.470us input-to-present. Large ChatGPT module sets exceed the
bounded synchronous cap and stay on the background warm path so first
presentation does not wait on every application bundle.

`silksurf-image` decodes PNG, JPEG, and WebP bytes into RGBA8 surfaces.
`silksurf-app` discovers `<img src>` resources, fetches them through the
cache-first network path, and passes decoded or HTML `width`/`height`
dimensions into the fused layout pass before taffy computes boxes. The app then
appends image display items at the fused layout rect for each decoded resource.
`silksurf-render` keeps the pixel payload in an `Arc<[u8]>` so viewport
display-list filtering and scroll redraws do not copy image bytes.
`quick_cache` stores decoded image surfaces under a 64 MiB byte budget and
reuses them across navigation workers. Animated images, `srcset`, lazy loading,
alt-text fallback, and a larger resource scheduler remain open browser work.

The HTTP disk cache now persists only text-like responses. Binary responses
remain in the in-memory cache for the current process and skip UTF-8 JSON
serialization. This prevents process-to-process cache reloads from corrupting
PNG/JPEG/WebP bytes while preserving disk persistence for HTML, CSS, JSON, XML,
and JavaScript.

`scripts/gui_probe.sh --release --backend auto --presenter auto --fixture
ai-chat --probe address --runs 3` after image fetch/decode/display/raster
integration reports `Images decoded: 1/1` in every run. Wayland SHM address
input stays inside the CPU target envelope after image dimensions move through
the fused layout pass: final render averages 1.597us, final total CPU work
averages 4.560us, and final input-to-present averages 6.693us. Image resource
work lands before the interactive chrome hot path.

## Navigation Buffer Reuse (2026-07-01)

`BrowserFrameBuffers` carries the previous RGBA viewport buffer and ARGB frame
buffer into `build_browser_page_with_buffers`. The navigation apply path takes
the old buffers before page construction, restores them on parse or CSS build
failure, and installs them into the new `BrowserPage` on success. The default
`build_browser_page` wrapper keeps tests and headless callers on fresh buffers.

`SILKSURF_TRACE_NAV_BUILD=1 scripts/gui_probe.sh --release --backend auto
--presenter auto --fixture ai-chat --probe reload --timeout-seconds 60`
resolves the live display to Wayland and passes the reload cache check. The
first fixture navigation builds the 4,096,000 byte viewport frame and reports
590ns in `argb-resize`, 883.250us in `argb-pack`, and 7.243783ms total page
build. The reload reuses the buffer capacity and reports 50ns in
`argb-resize`, 782.371us in `argb-pack`, and 7.913364ms total page build.

The reused-buffer trace moves the remaining bug surface away from vector
growth. `SILKSURF_TRACE_FUSED=1 SILKSURF_TRACE_TAFFY=1` splits that surface
into the fused layout phases. Before the layout metadata change, a traced
reload spends 8.219251ms in layout-paint, with 1.476614ms in Taffy rebuild and
4.824113ms in Taffy compute. `LayoutNeighborTable` now stores `child_start`
beside `child_count`, so `TaffyLayout::rebuild` consumes flat child ranges
instead of allocating nested child vectors. Taffy also keeps a per-compute
BFS-indexed text-measure cache keyed by font size and available width.

The follow-up live Wayland reload trace reports 5.003682ms in layout-paint,
516.330us in Taffy rebuild, and 3.238582ms in Taffy compute. The text cache
records 406 hits across 7,397 text measure calls and keeps measured text scan
time at 532.300us. The reload total page build is 7.913364ms with
`argb-resize` at 50ns and `argb-pack` at 782.371us. The next navigation target
is the Taffy solve itself, not DOM child-list construction. ARGB pack remains a
separate full-viewport copy target until the presenter consumes the renderer's
native pixel format or the renderer writes directly into the presentation
buffer.

## Real Page Smoke (2026-07-01)

`SILKSURF_TRACE_NAV_BUILD=1 scripts/gui_probe.sh --release --backend auto
--presenter auto --probe smoke https://chatgpt.com` reaches first presentation
on live Wayland without script errors. A follow-up probe after modulepreload
support uses cached HTML bytes, discovers five ChatGPT `rel="modulepreload"`
resources, and warms them through the cache-first resource path. The first
preload run fetches those five JS resources from network in about 730ms; the
next warm run returns them from cache in 10.320us to 12.880us each. The warm
page build reports 9.776716ms total: HTML 1.130672ms, CSS 968.831us, inline
scripts 5.180279ms, layout-paint 731.371us, raster 501.451us, ARGB resize
520ns, ARGB pack 1.227702ms, and ARGB total 1.233262ms for a 4,096,000 byte
viewport buffer. The first full-frame Wayland SHM present reports 3.492036ms
total CPU work.

After dynamic script element accessors land in the Boa DOM bridge, a warm
ChatGPT smoke still reaches first presentation. The run reports modulepreload
cache hits at 129.150us to 132.780us, page build at 6.337441ms total, scripts
at 2.252784ms, layout-paint at 705.121us, raster at 303.400us, ARGB pack at
783.912us, and first full-frame Wayland SHM present at 3.379225ms total CPU
work. This proves the bridge accessor change does not regress the real-page
first-presentation path.

A word-load and SSSE3 pack experiment fails the real-page smoke gate even
though it improves the local fixture micro-surface. Cached ChatGPT smoke
records `argb-pack` spikes at 58.278990ms, 23.570795ms, and 45.901264ms under
that experiment. The scalar shared pack path stays below 1ms on the same
4 MiB viewport buffer after the experiment is removed, so the retained path
favors stable real-page latency over fixture-only throughput.

The retained SSE2 pack path keeps the real-page smoke gate stable. A warm
ChatGPT smoke on live Wayland reports five modulepreload cache hits at
10.760us to 13.650us, page build at 5.630789ms total, scripts at 2.078748ms,
layout-paint at 727.542us, raster at 274.621us, ARGB pack at 247.991us, and
first full-frame Wayland SHM present at 3.516353ms total CPU work. The pack
loop no longer dominates the cached ChatGPT first-presentation path.

After dynamic classic script execution lands, a warm ChatGPT smoke still passes
on live Wayland with the persistent HTTP cache. The run reports five
modulepreload cache hits at 10.720us to 13.540us, page build at 5.785574ms
total, scripts at 2.108534ms, layout-paint at 736.232us, raster at 290.971us,
ARGB pack at 252.811us, and first full-frame Wayland SHM present at 3.531088ms
total CPU work. No dynamic script work appears in that cached ChatGPT trace, so
the bounded dynamic pass does not regress the real-page first-presentation
surface.

After default TLS provider sharing lands, a warm ChatGPT smoke still passes on
live Wayland with the persistent HTTP cache. The run reports five modulepreload
cache hits at 10.980us to 14.410us, page build at 5.655381ms total, scripts at
1.918793ms, layout-paint at 720.241us, raster at 278.011us, ARGB pack at
247.691us, and first full-frame Wayland SHM present at 3.226515ms total CPU
work. The dynamic pass finds no newly appended scripts on this cached page, so
the provider sharing change preserves the real-page first-presentation path.

The HTTP/1.1 response parser now decodes chunked transfer framing before brotli,
gzip, or deflate content decoding. A cold ChatGPT smoke with an isolated
`XDG_CACHE_HOME` no longer fails with `brotli decode: Invalid Data`: live
Wayland fetches 542705 HTML bytes in 304.931324ms, builds the page in
5.830951ms, runs scripts in 1.995944ms, packs the 4,096,000 byte ARGB frame in
255.580us, and presents the first full frame in 4.012458ms CPU work.
Modulepreload warming starts after the page build and first app-side frame
preparation; the same run logs the ChatGPT modulepreload URLs as `scheduled`
after `Navigation build total`, so modulepreload no longer blocks cold first
presentation or contends with the build-time ARGB packer.

Module graph warming now follows small static `import` and `export ... from`
edges from modulepreload and `type="module"` roots. The local AI fixture imports
`/module-child.js` from `/module.js`; the live Wayland page-input probe records
`Modulepreload graph round 0: 1 fetched, 1 pending`, then fetches
`module-child.js` in round 1. The same probe keeps the typed 13x22 damage frame
at 800ns render, 3.670us draw, 3.780us total CPU work, and 26.091us
input-to-present. A cold ChatGPT smoke with the graph warmer still reaches first
presentation from an isolated cache: 542897 HTML bytes fetch in 286.320532ms,
page build 5.833452ms, scripts 2.023193ms, ARGB pack 262.591us, and first
full-frame Wayland SHM present 3.547427ms. The warmer logs ChatGPT module URLs
as `scheduled` after `Navigation build total`; large bundles stay bounded by the
scan-byte cap.

The H2 asset path now drains multiplexed response bodies concurrently instead
of reading one stream to completion before polling the next body. That returns
flow-control credit while large ChatGPT module bundles are still in flight and
removes the peer reset that forced HTTP/1.1 fallback. `h2-fetch-probe` on the
14 ChatGPT module URLs first reproduced `body data: stream error received:
unspecific protocol error detected` followed by sequential fallback in
62.793961325s. With concurrent body draining and `h2` 0.4.15, the same 14 URLs
complete over H2 in 1.468189986s with no fallback and all responses at HTTP
200. A follow-up 2026-07-01 live probe on the current ChatGPT module set
returns the same 14 HTTP 200 responses in 1.141589137s.

The Boa browser host now installs `ReadableStream`, `WritableStream`,
`TransformStream`, `TextEncoderStream`, and `TextDecoderStream` constructors.
`ReadableStream` invokes `start(controller)` synchronously with inert
`enqueue`, `close`, and `error` controller methods. The DOM bridge exposes
inert per-element `style` and `dataset` objects, reports parser-phase
`document.readyState` as `loading`, supports `getElementsByTagName`, treats
`insertBefore(newNode, null)` as append, and detaches moved nodes from their
old parent before append/insert. `document.cookie` now persists lightweight
name/value pairs in the Boa host document, and `navigator.cookieEnabled`
reports true for feature detection. `crypto.getRandomValues` fills typed-array
style objects with OS randomness, and `crypto.randomUUID` returns RFC 4122
version-4-shaped identifiers. `AbortController` and `AbortSignal.abort()`
provide signal state, abort listeners, `throwIfAborted`, and pre-aborted
`fetch` rejection without adding a runtime dependency. React's SSR fragment
drain no longer loops on `e.firstChild`, and the ChatGPT smoke logs `Initial
host callbacks: 3`.

The fixed CSS parser no longer spins on malformed functional selector tails.
`target/release/bench_css <cached-chatgpt-root.css>` parses the cached ChatGPT
root stylesheet in 37.784160ms instead of timing out after 10s. The selector
parser treats `None` after the EOF sentinel as EOF, so malformed selector
arguments such as `:where(.)` terminate.

`SILKSURF_TRACE_FUSED=1 SILKSURF_TRACE_TAFFY=1` splits the ChatGPT smoke
layout-paint path. Before deterministic text metrics, Taffy measured 31 text
nodes and spent 49.387682ms inside `silksurf_text::measure_text`. After
deterministic metrics, the same 31 text calls cover 453 bytes, spend 3.270us
inside measurement, and leave fused style/layout/paint at 641.503us. The
layout crate no longer cold-starts cosmic-text font discovery from the Taffy
measure closure.

Remaining first-frame browser gaps are explicit: inline script execution costs
about 6ms when the bootstrap path runs successfully, CSS parsing still costs
about 1ms on the capped first-paint path, and first full-frame present still
copies the full viewport into the Wayland SHM buffer.

## Browser Chrome Status Feedback (2026-07-02)

Cursor motion over a page link now updates the chrome status text with the
link target and requests a status-only chrome redraw. Moving away from the
link restores the navigation status text through the same status-only damage
path. Navigation start, navigation success, navigation error, Stop, and
unsupported form submission clear stale hover status through the same status
setter.

`scripts/gui_probe.sh --release --backend auto --presenter auto --fixture
ai-chat --probe hover --timeout-seconds 90` passes on live Wayland. Hover over
the first fixture link presents only the status rectangle, `x=1000`, `y=8`,
`width=170`, `height=28`. The hover-enter frame reports 6.100us buffer
acquisition, 3.430us render, 9.570us draw, and 9.750us total CPU work. The
hover-leave frame reports 2.820us buffer acquisition, 2.620us render, 5.470us
draw, and 5.590us total CPU work.

`scripts/gui_probe.sh --release --backend auto --presenter auto --fixture
ai-chat --probe chrome --timeout-seconds 90` passes on live Wayland. The probe
drives Home, Back, Forward, Reload, and Stop-style chrome navigation through
native primary-click events and leaves chrome-only frames bounded to the
44-pixel toolbar damage region.

## Real Page Input (2026-07-01)

`scripts/gui_probe.sh --release --backend auto --presenter auto --probe
page-input --timeout-seconds 90 --max-total-ns 10000 https://chatgpt.com`
passes on live Wayland. The page-input path focuses ChatGPT node 354, scrolls
the offscreen editor into view, and presents the typed `!` as an 18x22 damage
rect. The final typed frame reports 4.630us buffer acquisition, 900ns render,
5.570us draw, and 5.750us total CPU work.

The first focus-scroll frame no longer enters the 60ms to 70ms cold Unicode
text path. Common Unicode punctuation maps through the built-in bitmap text
renderer, and background modulepreload graph warming stops before large
recursive rounds compete with first interaction.

`scripts/gui_probe.sh --release --backend auto --presenter auto --probe
page-input --runs 3 --timeout-seconds 90 --max-total-ns 10000
https://chatgpt.com` passes on live Wayland. The final typed frame reports
5.890us, 6.020us, and 6.160us total CPU work, with an 18x22 damage rect,
4.750us to 5.110us buffer acquisition, and 820ns to 980ns render work.

A 2026-07-02 follow-up after status-only hover damage still passes the same
strict real-page input gate. The final typed frame reports an 18x22 damage
rect, 4.940us buffer acquisition, 900ns render, 5.910us draw, and 6.080us
total CPU work on live Wayland.

The page-input probe now reports the focus-scroll frame separately through
`--max-focus-total-ns`. Large focus jumps use the direct full-viewport raster
path instead of the damage-scratch path, so the ChatGPT focus frame drops from
about 945.973us to 599.652us total CPU work in one live Wayland run. The frame
still presents full damage and still fails a 20us focus budget:
`focus_total_ns 466741 exceeds 20000`. This surface remains open for retained
viewport tiles or precomputed text/glyph layers.

`BrowserFrame` now carries a one-shot focus viewport cache for the first
offscreen page input target. The navigation build rasterizes the likely focus
viewport once, packs it as ARGB, and the focus handler swaps that buffer into
the active frame when the computed scroll offset matches. A live Wayland
ChatGPT probe with `--max-focus-total-ns 3000000` reports a focus frame at
175.150us total CPU work, with 5.110us buffer acquisition and 169.820us render
work. The typed `!` frame still passes the 0.01ms CPU gate at 6.220us total.
A later non-traced run after adding disabled-by-default render phase tracing
reports the same typed path at 7.040us total CPU work and a focus frame at
220.071us while background modulepreload graph warming is active.

Page-input focus no longer dirties browser chrome when the address bar is not
editing. A cached focus-scroll frame now presents only the visible content
rows, `x=0`, `y=44`, `width=1280`, `height=756`, instead of the full
1280x800 window. One live Wayland run reports 184.510us render, 189.350us
draw, and 189.530us total CPU work for that content-only focus frame. The
typed `!` frame remains inside the CPU target at 6.500us total.

The tighter focus gate still fails: the same live path reports
`focus_total_ns 155531 exceeds 20000`, and a follow-up content-only focus run
reports `focus_total_ns 235351 exceeds 20000`. `SILKSURF_TRACE_APP_FRAME=1
SILKSURF_TRACE_SHM_PHASES=1` splits the failure surface: the focus frame spends
191.570us in the app blit, 5.180us in chrome overlay, 2.680us in Wayland
attach/damage, and 1.490us in flush. This proves the open focus-scroll bug is
the large SHM content-area write, not buffer acquisition, cursor handling,
chrome damage, or input target selection.

A 2026-07-02 live Wayland ChatGPT probe before retained presentation reports
the focus frame at 200.900us render, 206.810us draw, and 206.990us total CPU
work while the typed `!` frame reports 770ns render, 6.630us draw, and 6.850us
total CPU work. The tight focus gate still fails at
`focus_total_ns 245560 exceeds 20000`, while the typed frame still passes at
6.960us total CPU work. This keeps the open bug on focus-scroll presentation,
not typed input.

`SILKSURF_TRACE_NAV_BUILD=1 SILKSURF_TRACE_RENDER_FULL=1` adds full-viewport
render phase evidence. A traced ChatGPT navigation reports the initial visible
viewport raster at 371.361us for 16 display items. The focus viewport cache
reports 1.555724ms in first buffer resize, 63.470us in fill, 141.351us in
paint, 1.769385ms total full render for 15 display items, and 3.218008ms for
the full focus-cache phase including ARGB packing. The first-build cache cost
is separate from the focus-event copy cost; the next durable fix moves the
focus viewport into a presenter-owned retained buffer or tile cache before the
input event.

The Wayland SHM presenter now exposes retained-buffer primitives and the winit
browser path uses them for focus-scroll. `WaylandShmRetainedTag` names a
prepared buffer, the presenter copies a released buffer from caller-owned
pixels under that tag after the first present, and focus requests tagged
presentation with the visible-content damage rect instead of calling the render
callback. A strict live Wayland ChatGPT probe with `--max-focus-total-ns 20000`
passes: the focus frame reports 0ns render, 4.930us draw, and 5.230us total CPU
work, while the typed `!` frame reports 1.050us render, 6.260us draw, and
6.440us total CPU work. The same run reports 463.169us focus input-to-present
and 974.749us typed input-to-present, so compositor/event scheduling remains a
separate evidence class outside the app-owned CPU budget.

A follow-up strict live Wayland ChatGPT probe after form-submit retained
navigation start still passes the page-input gate. The cached navigation loads
308309 bytes in 17.160us, presents the first full frame in 3.474626ms, focuses
node 354 through retained full-content damage at 0ns render and 5.470us total
CPU work, and then presents the final typed `!` as `x=12`, `y=674`,
`width=106`, `height=18`. The typed frame reports 5.180us buffer acquisition,
1.530us render, 6.870us draw, 6.930us total CPU work, and 81.650us
input-to-present.

The screenshot that shows leading text such as `22f` maps to a poisoned
persistent HTTP cache entry, not to the current parser or painter hot path.
The cached ChatGPT root body begins with an HTTP chunk-size line before
`<!DOCTYPE html>` and still carries `Transfer-Encoding: chunked`. That stores
transport framing as document bytes, so the HTML parser receives stray text
before the document element and later paint shows that transport text. The
disk cache now loads only decoded text entries and refuses rows with
`Transfer-Encoding` or `Content-Encoding`; stale encoded cache rows force a
fresh fetch and cannot keep contaminating later renders.

After the stale cache row is replaced, the ChatGPT root cache stores 542771
decoded bytes with no transfer or content encoding header and begins directly
at `<!DOCTYPE html>`. The strict live Wayland page-input probe focuses text
node 344 and updates it, so the target-selection surface is repaired. The
typed `!` frame reports `x=12`, `y=756`, `width=106`, `height=18`, with
3.880us buffer acquisition, 1.040us render, 4.950us draw, and 5.090us total
CPU work. The first focus frame still misses the 20us focus gate at
577.562us total CPU work because it falls back to full damage instead of a
retained current-view present. That keeps the next performance bug on retained
current-view availability for first synthetic focus, not on text input,
transport decoding, or DOM target selection.

The remaining ChatGPT "little text" surface after cache hygiene is browser
completeness. The root HTML shell parses and first presentation is fast, but
large ChatGPT module graphs exceed the bounded synchronous execution caps and
stay on the background warm path. CSS coverage remains secondary until a clean
decoded document still paints sparsely; the current evidence first implicates
JavaScript hydration and browser host APIs such as DOM events, CSSOM, storage,
network scheduling, and persistent WebSocket/session behavior. Rendering hot
path evidence stays separate: the retained focus frame and typed input frame
meet the app-owned CPU budget on live Wayland.

## Binary Size Pressure (2026-06-30)

`cargo machete` reports no unused workspace dependencies after removing the
unused `silksurf-render` edge from `silksurf-gui`.

`cargo bloat -p silksurf-app --release --bin silksurf-app -n 20` reports a
20.1MiB release binary with a 12.4MiB `.text` section. The largest functions
are AWS-LC AVX512 AES-GCM encrypt/decrypt at 332.0KiB each, taffy grid sizing
at 79.2KiB, X11 dynamic loader setup at 70.0KiB, Boa realm initialization at
60.9KiB, taffy child layout at 60.8KiB, CSS cascade at 52.1KiB, taffy flex
layout at 48.1KiB, skrifa glyph hint dispatch at 43.8KiB, H2 parallel fetch at
42.5KiB, and the fused pipeline at 41.1KiB.

`cargo llvm-lines -p silksurf-app --bin silksurf-app` reports 180655 LLVM IR
lines. The top codegen pressure remains event-loop plumbing and the large app
entry path: `calloop::EventLoop::dispatch_events` at 4198 lines,
`silksurf_app::main` at 3719 lines, the app render/input closure at 2714 lines,
and winit Linux/X11/Wayland event-loop setup in the next tier.

## Plan (Next)
- ChatGPT-scale benchmark (397 nodes) to validate L1 cache pressure predictions.
- HTTP/2 parallel fetch for first-render network latency reduction.
- In-process stylesheet cache (Arc<Stylesheet> keyed by CSS text hash).
- rkyv zero-copy stylesheet archive for cross-process cold start.
- SoA DOM conversion for layout pass (separate from cascade SoA).

## Benchmarks and Guardrails
Core benchmarks:
- `cargo run -p silksurf-engine --bin bench_pipeline`
- `cargo run -p silksurf-css --bin bench_selectors -- --guard`
- `cargo run -p silksurf-css --bin bench_selectors -- --workload`
- `cargo run -p silksurf-css --bin bench_cascade_guard`

Guardrails:
- `make perf-guardrails` (thresholds via `PIPELINE_US`, `SELECTORS_NS`, `CASCADE_US`)
- Optional RSS check: `MAX_RSS_KB=26000 make perf-guardrails`

### Interner Microbenchmarks (local interners)
- `cargo bench -p silksurf-core --bench interner`
- `cargo bench -p silksurf-js --bench interner`

Representative medians from one local run:

| Crate | Scenario | Median |
|---|---|---:|
| `silksurf-core` | insert-heavy (10k unique keys) | `944 us` |
| `silksurf-core` | resolve path (10k symbols) | `13.48 us` |
| `silksurf-core` | repeated-key hit (100k hits) | `2.016 ms` |
| `silksurf-js` | insert-heavy (10k unique keys) | `1.445 ms` |
| `silksurf-js` | lookup `get` path (10k existing keys) | `305.6 us` |
| `silksurf-js` | resolve path (10k symbols) | `9.700 us` |
| `silksurf-js` | repeated-key hit (100k hits) | `2.685 ms` |

Notes:
- This establishes a baseline for the post-`lasso` local interners in both crates.
- No low-risk optimization was applied in this pass: no benchmark indicated a clear regression requiring code changes.
- Scope boundary: this benchmark/documentation pass adds no CI schedule-trigger changes.

## Optimization Tooling
- PGO: `./scripts/pgo_build.sh bench_pipeline`
- BOLT: `./scripts/bolt_build.sh bench_pipeline`
- `cargo flamegraph`, `cargo valgrind`, `cargo bloat`, `cargo llvm-lines`

## Notes
- Use `release-riced` profile for max throughput (see `Cargo.toml`).
- Keep profile-level changes in the workspace `Cargo.toml` only.
