# Boa reference checkout cleanup

Date: 2026-05-21

## Decision

`silksurf-extras/boa` is not required to build SilkSurf. The live workspace
depends on published crates:

- `boa_engine = { version = "0.21", features = ["annex-b"] }`
- `boa_runtime = "0.21"`

`cargo metadata --no-deps` resolves those dependencies from crates.io, not from
the nested checkout. `cargo search boa_engine --limit 3` reported
`boa_engine = "0.21.1"`, so the current `^0.21` constraint already admits the
latest patch release in the 0.21 line.

## Local nested checkout state

The nested checkout had local cargo-fuzz related edits:

- `Cargo.toml`: excluded `fuzz` from the Boa workspace.
- `tests/fuzz/Cargo.toml`: changed edition from 2021 to 2024.
- `tests/wpt/Cargo.toml`: changed edition from 2021 to 2024.
- `fuzz/`: added a separate cargo-fuzz workspace with parser, compiler, eval,
  and default fuzz targets.

That state was not a SilkSurf dependency and was not suitable to merge into
SilkSurf. It belonged either in a Boa fork or as a separate fuzz experiment, not
as a dirty nested checkout inside the kept SilkSurf project.

## Cleanup

The nested `silksurf-extras/boa` checkout was moved to Trash after this note was
recorded. SilkSurf should continue to use the public Boa crate API unless a
future task explicitly creates a maintained Boa fork branch with its own tests,
PR, and merge path.

## Related recovery branch

The local branch `recovery/scope-crashed-session` carried useful historical
ideas, but current `main` already contains the durable results: `CascadeView`,
`FusedWorkspace`, the lock-free resolve table, `tls-probe`, and the documented
9.5us steady-state fused pipeline result. The branch also deleted current
documentation and test fixtures, so merging it raw would regress the project.
It was treated as superseded recovery state rather than as mergeable work.
