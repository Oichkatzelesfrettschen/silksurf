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
