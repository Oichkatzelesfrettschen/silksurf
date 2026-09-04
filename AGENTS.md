# silksurf Agent and Developer Reference

## Instruction Source

This root AGENTS.md is the durable instruction file for silksurf.

Nested AGENTS.md files may add narrower rules for their subtree. When rules
conflict, the narrower file controls only inside its subtree.

## Hard Rules

- Checked-in text is emoji-free. An emoji carries no information its word does
  not, and it breaks greps, widens diffs, and renders as a box or a double-width
  cell wherever the glyph is missing. Typographic substitutes stay out on the
  same grounds: straight quotes over curly ones, `--` over an em dash, `...` over
  an ellipsis glyph. Symbols that carry meaning stay in -- mathematical
  operators, Greek letters, arrows in state transitions, box-drawing in diagrams,
  the degree and micro signs, and CJK such as the `.公司` public suffix
  `crates/silksurf-core/src/psl.rs` handles. A name keeps the spelling its owner
  uses. `scripts/lint_text_hygiene.sh` enforces this over authored markdown.
- Rust source follows rustfmt. Do not hand-align Rust against prose taste.
- Treat warnings as defects. Touched Rust code builds with warnings denied.
- Keep changes surgical. Do not reformat unrelated files.
- Use `docker compose`, never legacy `docker-compose`.
- Do not commit secrets, local absolute paths, private hostnames, or generated
  machine-only state.
- Paths in checked-in work are repository-relative or PATH-resolved tool names.
  Discover the repository root with `repo_root=$(git rev-parse --show-toplevel)`.
- Hazardous or experimental paths open on an exact opt-in value. Unset, empty,
  and zero-valued gates stay closed.
- A load-bearing claim binds to a named source: crate, module, function, spec
  rule, test case, or measurement. Provenance detail rides in the commit
  message and the finding.

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

### Stating mechanism as fact

State what a thing is and does, in positive declarative form, third-person
present tense: `the paint list drops subtrees whose computed display resolves
to none`. Name the mechanism and let the binding constraint stand as fact;
correctness follows from the mechanism, so the reviewer assumes it and
correctness claims and contrast framing fall away.

A boundary takes its positive dual. Write the restriction (`debug builds
only`), the named home (`chronology lives in the commit message`), or the
mechanism itself (`the caller retains the allocation`; `this glyph cache
resolves only through the shaped-run path`; `fetch resolvers park in the
GC-rooted registry and drain at microtask checkpoint`). A stacked absence
collapses to the positive fact its members share -- `a repaint that fits the
damage rect completes in one pass`. Each positive form entails the absence a
negation would state, so the specimen stays off the page. A hard-stop safety
or security boundary keeps its prohibition, where that is the whole content.

Uncertainty names the mechanism and the guarded, disabled, or falsifiable
path. Reproduced-but-unspecified behavior names where it was observed;
conjecture is marked (`appears to`) or removed.

### Comment class and placement

Four decisions generate a comment: semantic role, evidence class, language
form, and placement. The role fixes which facts the comment carries; an API
contract, a cross-crate invariant, a one-line layout fact, and an unsafe-code
proof have different information shapes and share no template.

- Semantic role: contract (behavior, ownership, lifetime, error conditions),
  translation (spec rule to data structure), local invariant, engine or
  platform quirk, representation (buffer layout, state transition), safety
  proof, or module-scope navigation.
- Evidence class: specified behavior takes the plain indicative; measured
  behavior names the measurement; conjecture is marked or removed.
- Language form: Rust doc or line comment, field annotation, or a compact
  semantic table.
- Placement: architecture that persists across a module lives at module or
  type scope; the point of use carries only the local link in the chain; a
  branch comment states what distinguishes the branch, at the branch.

Rust code uses `//!` for module contracts, `///` for public items and fields,
`# Safety` / `# Panics` / `# Errors` sections when those obligations exist,
and `// SAFETY:` immediately before each unsafe operation. A `// SAFETY:`
comment proves every precondition the operation relies on -- lifetime,
ownership, aliasing, synchronization. A bare non-nullness claim is an
incomplete proof:

```rust
// SAFETY: `surface` outlives the returned slice, and softbuffer holds the
// only mapping of the buffer for the duration of the borrow.
```

### Source comment shape

A full mechanism comment orders its facts, and each fact stands on the one
before it:

1. Load-bearing claim: `Taffy caches the measured text size per generation.`
2. Authority by name: the crate, function, spec rule, or WPT case. A stable
   spec section may trail the named rule as disambiguation.
3. Consequence, with an inline code fragment when clearer than prose.
4. Test reference when the comment explains a fixed failure.
5. Env gates or flags, grouped at the end of the block.

Use the smallest applicable subset; most comments carry one or two elements,
and a comment carrying one fact is one sentence. Default to a one-line
trailing comment on the load-bearing line over a function-header paragraph.
One thought per comment: a multi-clause sentence fusing separate steps splits
into stacked comments. Mechanism controls length -- the count of distinct
load-bearing facts sets it, and a line threshold does not. Mechanical code
reads bare.

Prefer a causal connective when one fact forces another, or sequence when the
order itself is the mechanism (`The parser closes the implied paragraph. Then
it inserts the block element. Then layout sees a single flow root.`). Either
beats one passive sentence with three clauses, and both beat imperative
narration. A compact semantic table or diagram that encodes a buffer layout,
bit layout, or state transition is content; delimiter lines, banner boxes, and
wrappers such as `// =====` are decoration.

### TODO comments

A deferred-work comment opens with `TODO:`, `FIXME:`, `XXX:`, or `HACK:`, and
a new marker comes from that four-item set. It names three mechanism elements:

- missing work: the function, module, spec rule, or crate boundary needing the
  change;
- deferral reason: the API, evidence, or platform constraint blocking
  completion;
- tracking artifact: a durable function name, spec chapter, roadmap entry, or
  public issue URL. When no external issue exists, the named function or spec
  chapter is the tracking artifact.

A TODO body carries mechanism only. Reviewer breadcrumbs, PR-thread
references, phase labels, AGENTS.md rule citations, and deictic references
such as `currently` and `this crate` live in the commit message or PR
description.

### Durable names

Use names from mechanism or content, not chronology, actors, work sessions, or
review process. This governs branches, commit subjects, PR titles, doc and
finding filenames, tests, bundles, and checked-in identifiers.

To derive one: read the artifact, state in one line what it does or contains,
isolate the mechanism and object, then name those.

- `measures React click-to-commit latency` -> `react-interaction-commit-latency`
- `supervises the native runtime in a child process` ->
  `native-runtime-supervision`

Forbidden load-bearing identity: waves, phases, missions, agents, worktrees,
sessions, reviewers, PR numbers, task numbers, and dates that do not describe
content. Phase and wave terms may appear only as secondary registry metadata,
such as a `phase:` field in finding frontmatter; that field is their one home.

A name describes what is inside, not the act of collecting, grouping, staging,
or sequencing. `tranche` names no content and stays out. `set`, `batch`, and
`group` stay out only as ordinal containers (`set5`, `batch_2`); descriptive
domain compounds are fine (`batch_size`, `group_map`).

The first commit subject matters because a squash merge reuses it even when
the PR title was corrected later. Set branch name, first commit subject, and
PR title before first push.

### Findings and agent-loaded Markdown

Finding documents carry chronology: dated frontmatter, `last_verified`,
`evidence_class`, dated filenames, ordered predecessors. A PR or issue
reference pairs with a durable identifier.

- Wrong: `the fix landed via PR #58`
- Right: `landed in 640166e (silksurf_core::engine_protocol wire framing);
  PR #58 for cross-link`

Markdown loaded by agents uses exactly one H1, heading depth no deeper than
`###`, frontmatter on programmatically loaded files, language tags on code
fences, and exact cross-references (`scripts/lint_doc_links.sh` gates the
links). Rule text is direct positive-declarative statement. Tables appear only
when columns carry independent comparison value; bullets carry ownership,
lookup, and rule lists. Slice-loaded text stands without nearby context.

## Engineering Posture

- Every implementation decision is a genuine solution. When blocked, rescope
  and trace the root cause; do not hack around it, silently narrow scope, or
  leave core logic behind a TODO.
- Cleanroom boundary: production code and `silksurf-specification/` never
  depend on `diff-analysis/` (enforced by `scripts/lint_cleanroom.sh`).
- `silksurf-extras/` and `silksurf-js/test262/` are untracked reference
  checkouts; they inform behavior and supply no copied code. Implementations
  derive from `silksurf-specification/` and the documented reasoning in
  `docs/`.
- Update specifications and ADRs (`docs/design/ARCHITECTURE-DECISIONS.md`)
  before or with the code they govern. State the mechanism, then the
  consequence, then the evidence.
- Scope cuts are named, never silent: a deferred piece gets one line in the
  owning roadmap with the mechanism name and the reason.
- Treat a surprising deviation as evidence, not noise. Preserve it, name it,
  and decide whether it changes the model.
- Investigate before asserting. Memory, prior summaries, and recalled context
  are leads; AGENTS.md and the source are authority.

### Evidence rank

When sources conflict, higher rank controls. An implementation-affecting
architecture claim needs a rank 1 through 4 source by name; claims without
that backing are hypotheses.

1. Live run: traced GUI frame, live page interaction, measured latency or RSS.
2. Test oracle: WPT, test262, and workspace tests, when the oracle is
   spec-grounded.
3. Specification: HTML, CSS, ECMA-262, Fetch, URL, and the documents under
   `silksurf-specification/`.
4. Crate source and ADRs in `docs/design/ARCHITECTURE-DECISIONS.md`.
5. Benchmarks and microbenchmarks, which bound CPU work rather than
   end-to-end behavior.
6. Documentation and comments, only when consistent with ranks 1 through 5.

Build success, workspace-test success, conformance success, and live-browser
behavior stay separate evidence classes. A conformance claim rests on a
conformance run; build-only evidence proves a build.

### Reasoning checks

- Hypothesis tree: list plausible root causes, rank them by evidence cost and
  likelihood, test the cheapest decisive case first, and prune on
  falsification.
- Opposition review: after forming a synthesis, argue the strongest contrary
  case. If it survives, the finding stays unresolved and names the next
  evidence needed.
- Claim audit: before committing a finding, list each implementation claim and
  the ranked source that backs it.
- Recorded prediction: a prediction stands after observation. Deviation is the
  finding, and it opens a new investigation rather than editing the
  prediction.

Stop and report when a hypothesis survives three independent falsification
attempts, fails in an unexpected way, requires a non-obvious architecture
choice, or a measurement contradicts a rank-1 or rank-2 source.

### Synthesis over selection

When merging parallel branches or review findings, preserve all non-refuted
content; the default resolution is union plus synthesis. Mechanism and
evidence decide; branch age, chronology, and author do not. Selection applies
only when the discarded side is empirically refuted or superseded by a
verified line-level diff with recorded rationale. `git merge -X theirs`,
`git checkout --theirs`, blanket conflict-marker stripping, and unreviewed
deletion are not synthesis.

A synthesis improves the material: a stronger mechanism model, a
cross-reference, a validation matrix, a sharper evidence class, a
source-grounded invariant, a reusable probe, or a semantic-preserving
refactor. Merging text without improving the model is not synthesis.

### Agent coordination

Use at most three concurrent subagents. Subagents are read-only evidence
collectors unless the user authorizes a different role; each carries a bounded
task, input scope, expected output, and citation requirement. The parent owns
synthesis, conflict resolution, implementation choices, commit pushes, file
deletion, build-configuration changes, warning suppression, and final claims.

Agent task tracking is transient working state. Durable state lands in code,
tests, commit messages, findings, documentation, or retained bundles.

## Commits and Pull Requests

Commit subjects use a component prefix and a concise mechanism:
`silksurf-js: park fetch resolvers in a GC-rooted registry`. The subject
carries component and mechanism only; issue and PR links ride in trailers.

The body makes the invariant, the change, and the evidence reviewable in one
to five sentences: name the root cause or constraint, name the fix, cite the
spec rule, function, or crate boundary when load-bearing, and state test
movement plainly. Commit prose is emoji-free, declarative present tense,
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
  `lizard -l rust -C 16 <paths>` for touched files.
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
- Use `lizard -l rust -C 16` for touched Rust complexity gates.
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

## Claude Code notes

These notes came from the retired standalone `CLAUDE.md` loader and hold the Claude Code specifics that a tool-generic guide leaves out. A rule that applies to every agent lives in the sections above.

### Loading rule

### Build and test entry points

- Fast gate: `make check` (rustfmt, clippy -D warnings, lint_unwrap,
  lint_unsafe, lint_glossary, lint_doc_links, lint_cleanroom,
  lint_text_hygiene)
- Test: `make test` (workspace tests, warnings denied)
- Full gate: `make full` (check + test + cargo deny + rustdoc); required
  before merge-ready claims
- Reference: `docs/development/LOCAL-GATE.md`; CI is strict-local-only (AD-009)

### Claude Code operating notes

Inspect the real repository with Claude Code tools before editing. Memory,
prior summaries, and recalled context are leads; `AGENTS.md` and the source
are authority.

Inspect the diff after every edit. The adversarial staged-diff read runs before
any commit or completion claim.

Claude Code task tracking is transient working state; durable state lands in
code, tests, commit messages, findings, documentation, or retained bundles.
For any task involving two or more steps, track progress under durable
mechanism names and rescope as discoveries land.

Subagent limits, the read-only default, and evidence rank live in `AGENTS.md`
under `Agent coordination` and `Evidence rank`.

### Response shape

Responses report the changed mechanism, the evidence used, the validation run,
the checks not run and why, and the remaining risks or unresolved falsifiers.
Chained reasoning appears when it explains the next action or a validation
requirement; the rest of the deliberation lives in thoughtspace. Responses are
plain ASCII mechanism prose under durable names.

### Orientation pointers

- Product: Rust workspace (13 crates under `crates/` + `silksurf-js`); the
  legacy C tree is retired per AD-024.
- Decisions: `docs/design/ARCHITECTURE-DECISIONS.md`
- Specs: `silksurf-specification/`
- Roadmaps: `docs/roadmaps/SPA-CAPABILITY-ROADMAP.md` (forward),
  `docs/roadmaps/DEBT-RECONCILIATION-ROADMAP.md` (debt)
