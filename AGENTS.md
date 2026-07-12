# silksurf Agent and Developer Reference

## Instruction Source

This root AGENTS.md is the durable instruction file for silksurf.

Nested AGENTS.md files may add narrower rules for their subtree. When rules
conflict, the narrower file controls only inside its subtree.

## Hard Rules

- Keep checked-in text ASCII unless a file already requires another encoding.
- Rust source follows rustfmt. Do not hand-align Rust against prose taste.
- Treat warnings as defects. Touched Rust code builds with warnings denied.
- Keep changes surgical. Do not reformat unrelated files.
- Use `docker compose`, never legacy `docker-compose`.
- Do not commit secrets, local absolute paths, private hostnames, or generated
  machine-only state.

## Source Comments and Durable Prose

Source comments, commit subjects, commit bodies, PR text, durable docs, and
agent-authored checked-in prose use direct, declarative present tense.

Write the mechanism first. Name the API, crate, data structure, invariant, or
runtime boundary that makes the statement true. State the consequence plainly.
Avoid ceremonial WHY/WHAT/HOW blocks in durable comments when one direct
mechanism paragraph carries the same information.

Good shape:

```rust
/*
 * softbuffer maps the native window surface as 0xAARRGGBB words.
 * RedrawRequested copies the current frame into that mapped slice and
 * presents it without allocating a second frame buffer.
 */
```

Avoid first-person project narration, contrast framing, phase names, task
numbers, PR chronology, reviewer notes, agent notes, and "finish later" prose
in source comments. Put history and tradeoffs in commit messages, PR
descriptions, or design docs.

Use durable names for branches, commits, docs, bundles, tests, and findings.
Name the target, mechanism, and evidence or outcome. Do not use phase, wave,
mission, sprint, session, agent, or chronology labels as primary names.

## Engineering Posture

- Every implementation decision is a genuine solution. When blocked, rescope
  and trace the root cause; do not hack around it, silently narrow scope, or
  leave core logic behind a TODO.
- Cleanroom boundary: production code and `silksurf-specification/` never
  depend on `diff-analysis/` (enforced by `scripts/lint_cleanroom.sh`).
- Update specifications and ADRs (`docs/design/ARCHITECTURE-DECISIONS.md`)
  before or with the code they govern. State the mechanism, then the
  consequence, then the evidence.
- Scope cuts are named, never silent: a deferred piece gets one line in the
  owning roadmap with the mechanism name and the reason.

## Commits and Pull Requests

Commit subjects use a component prefix and a concise mechanism:
`silksurf-js: park fetch resolvers in a GC-rooted registry`. The subject
carries component and mechanism only; issue and PR links ride in trailers.

The body makes the invariant, the change, and the evidence reviewable in one
to five sentences: name the root cause or constraint, name the fix, cite the
spec rule, function, or crate boundary when load-bearing, and state test
movement plainly. Commit prose is plain ASCII, declarative present tense,
American English. A body that reads like a worklog -- nested bullets from a
coarse squash, several sub-components -- means the commits were not granular
enough: split them or compress to the aggregate mechanism.

Chronology, build invocations, tool output, host names, and validation
checklists live in the PR description, not the commit body. Historical design
debate about rejected alternatives lives in the commit message or PR, never
in source comments.

Each commit is buildable, reviewable, and bisectable. Formatting churn and
logic changes ride separate commits. One logical change per commit; one topic
per PR. No fixup commits arrive for review.

Branch names, first commit subjects, and PR titles carry durable mechanism
names, set before first push. Wave, phase, mission, session, sprint, and
agent labels never serve as primary names.

Trailers:

- `Fixes:` names only the earlier commit that introduced the defect.
- `Closes:` carries issue URLs.
- AI disclosure lives in commit trailers alone, never in file headers or
  source comments. Use `Assisted-by: <tool> (<model>)` for mixed human/AI
  work and `Generated-by: <tool> (<model>)` when AI generated almost the
  entire change. `Co-authored-by:` is reserved for human co-authors. Trivial
  mechanical changes may omit disclosure.

## Rust Workflow

- Read the current code path before editing.
- Prefer existing crate boundaries and helpers.
- Add dependencies only when the crate solves a real missing mechanism and fits
  the low-resource browser profile.
- Run rustfmt on touched Rust files.
- Keep touched Rust functions at cyclomatic complexity 16 or lower. Use
  `/home/eirikr/.local/bin/lizard -l rust -C 16 <paths>` for touched files.
- Run targeted checks while developing:

```sh
RUSTFLAGS='-D warnings' cargo check -p silksurf-app --all-targets
```

- Run the full local gate before merge-ready claims when time and host support
  allow it:

```sh
scripts/local_gate.sh full
```

If a required check is not run, report `not run` with the reason.

## Browser Front-End Direction

silksurf targets a low-resource, responsive browser profile. GUI work prefers
small event loops, direct buffers, bounded allocations, cache reuse, and clear
latency evidence over broad framework surface.

Address input, chrome redraw, and page interaction work targets a 0.01 ms
hot-path budget. Measure the CPU work separately from compositor scheduling,
buffer acquisition, network fetch, page execution, and display refresh. Treat
microbenchmarks, traced GUI frames, and live webpage interaction as different
evidence classes.

A working front end requires a real browser surface:

- network fetch with cache and TLS policy;
- HTML tree construction with head/body semantics;
- CSS cascade and computed style;
- layout with hidden/non-rendered subtree filtering;
- paint list construction that excludes style/script metadata text;
- text shaping and links/forms/input events;
- native window presentation with low idle CPU;
- navigation controls, URL entry, reload/stop, history, and status feedback.

Build claims, runtime claims, rendering claims, and browser-frontend claims are
separate evidence classes.

## Analysis Tools

Use the cheapest tool that falsifies the claim before reaching for heavier
instrumentation.

- Use `rg`, `fd`, `cargo tree`, and `cargo machete` for source and dependency
  surface discovery.
- Use `/home/eirikr/.local/bin/lizard -l rust -C 16` for touched Rust
  complexity gates.
- Use `rust-analyzer`, `cargo llvm-lines`, `cargo bloat`, `cargo udeps`,
  `cargo deny`, `scc`, and `cloc` for call-surface, binary, dependency,
  policy, and size pressure.
- Use `cflow`, `cscope`, `global`, `ctags`, and `readtags` for the legacy C
  and XCB tree. Do not treat `cflow` as Rust call-graph evidence.
- Use `hyperfine` for repeatable command timing. Keep GUI input timing in
  built-in trace output when measuring address input, chrome redraw, buffer
  acquisition, compositor wait, or input-to-present time.
- Use `perf stat`, `perf record`, `flamegraph`, `hotspot`, `sysprof-cli`,
  `uftrace`, `valgrind --tool=callgrind`, `strace`, `ltrace`, `bpftrace`,
  and `heaptrack` when microbenchmarks do not explain latency, allocation,
  scheduler, syscall, indirect-call, cache, or buffer-wait behavior.
- Use `wayland-info`, `wev`, `xprop`, `xwininfo`, `Xvfb`, and `xvfb-run` for
  display-backend evidence. Prefer the live Wayland or X11 backend when the
  bug depends on compositor behavior.
- Use `likwid-topology` and `likwid-perfctr` when the host CPU topology,
  counters, cache pressure, or memory bandwidth shape a performance claim.
