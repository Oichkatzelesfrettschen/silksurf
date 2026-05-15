# Reproducible Builds

> Build silksurf so that two operators on two machines, given the same
> commit, produce byte-identical release artifacts.

## WHY

Reproducibility is the mechanical underpinning of every supply-chain claim
silksurf can make. Without it:

- A signed binary proves only that *someone* signed it, not that the
  signature corresponds to the source the operator can read.
- Downstream packagers cannot verify that the tarball you ship matches
  the source they review.
- An auditor cannot detect a compromised build host, because every
  rebuild looks legitimately different.

With reproducibility:

- Any third party can rebuild from the tag and `sha256sum` the artifact;
  if the hash matches, the binary is provably the source.
- The CycloneDX SBOM (`scripts/generate_sbom.sh`) becomes load-bearing
  evidence rather than a polite formality.
- cosign / sigstore signatures gain meaning beyond identity attestation.

This is the same threat model that motivated the Reproducible Builds
project (https://reproducible-builds.org) and the Go toolchain's
deterministic build mode.

## WHAT

Two environment variables and a build-flag discipline.

### SOURCE_DATE_EPOCH

Set to the Unix timestamp of the *commit being built*. Rust's compiler,
cargo, and many crates that embed timestamps (cyclonedx, build-info-style
crates, anything calling `SystemTime::now()` at build time) honour this
variable when present and fall back to wall-clock time otherwise.

### CARGO_BUILD_RUSTFLAGS / RUSTFLAGS

Pin codegen flags so independent rebuilds choose the same instruction
selection. The release profile already pins `lto = "fat"`,
`codegen-units = 1`, and `panic = "abort"` (see root `Cargo.toml`),
which removes the largest sources of nondeterminism. The remaining
discipline is to avoid setting `-C target-cpu=native` for distributable
artifacts -- that flag is host-specific and breaks reproducibility
across machines with different microarchitectures.

### Toolchain pinning

`rust-toolchain.toml` already pins to 1.94.1. Reproducibility requires
that two operators use the *exact* same toolchain; rustup honours
`rust-toolchain.toml` automatically, so no extra step is needed if
operators have rustup installed.

## HOW

### Reproducible release build

```sh
# Step 1: capture the commit timestamp.
export SOURCE_DATE_EPOCH=$(git log -1 --format=%ct)

# Step 2: clean any previous artifacts to remove cached side-effects
# from non-deterministic builds.
cargo clean -p silksurf-app

# Step 3: build. The release profile in Cargo.toml already supplies
# lto/codegen-units/panic; no extra RUSTFLAGS needed for the canonical
# release.
cargo build --workspace --release

# Step 4: hash the artifact.
sha256sum target/release/silksurf-app
```

Two operators running these four commands on the same commit, with the
same Rust toolchain (`rustup show active-toolchain`), should obtain the
same SHA-256.

### Cross-target reproducibility

Cross builds reproduce in the same way; the env var is unchanged.

```sh
export SOURCE_DATE_EPOCH=$(git log -1 --format=%ct)
cargo build --workspace --release --target aarch64-unknown-linux-gnu
sha256sum target/aarch64-unknown-linux-gnu/release/silksurf-app
```

(See `docs/development/CROSS-COMPILE.md` for the cross toolchain setup
silksurf uses on the perf lab's aarch64 SBC fleet.)

### Verifying with sha256sum

The simplest end-to-end check. Two operators each produce a hash, then
compare. If the hashes differ, neither side knows *where* the divergence
is; that is what diffoscope is for.

```sh
# Operator A on machine_a:
sha256sum target/release/silksurf-app > silksurf-app.sha256.machine_a

# Operator B on machine_b:
sha256sum target/release/silksurf-app > silksurf-app.sha256.machine_b

# Diff:
diff silksurf-app.sha256.machine_a silksurf-app.sha256.machine_b
```

### Verifying with diffoscope

When two builds *should* match but the hashes differ, diffoscope is the
right tool to localise the divergence. It walks ELF binaries, debug
sections, embedded strings, archive metadata, and presents a structured
diff.

```sh
# Install once:
#   pacman -S diffoscope                 # Arch / CachyOS
#   apt install diffoscope               # Debian / Ubuntu
#   pip install --user diffoscope        # generic

diffoscope \
    target/release/silksurf-app \
    /path/to/other-operators/silksurf-app
```

Common sources of divergence diffoscope will surface:

- Build paths embedded in the debug-info (DWARF `DW_AT_comp_dir`).
  Mitigation: build inside an identical absolute path on both machines,
  or set `RUSTFLAGS=-Csplit-debuginfo=packed --remap-path-prefix=...`.
- Timestamps in object-file metadata. Mitigation: `SOURCE_DATE_EPOCH`
  (already set above).
- Random suffixes in symbol names. Rust uses `-Z` flags for this on
  nightly only; on stable, ensure both operators use the same toolchain
  channel (1.94.1 stable, not nightly).

### What the tag-time release script does

`scripts/release.sh` runs `cargo build --workspace --release` and the
local-gate but does NOT itself fix `SOURCE_DATE_EPOCH`. Two reasons:

1. The tag-time build is for *gating* (does this commit even build?),
   not for distribution. Distribution-quality artifacts are produced by
   `cargo dist build` after the tag is pushed.
2. Forcing the env var inside the script would mask the requirement
   from auditors. Operators must learn the discipline: set it explicitly
   when building distributable artifacts.

Use the explicit recipe above when producing artifacts intended for
external consumers.

## Known non-determinism we have NOT yet eliminated

- **Linker-introduced build IDs.** GNU ld embeds a `--build-id=sha1`
  entry into the ELF .note.gnu.build-id section. Most distros want this
  for symbol lookup; if you need bit-identical binaries across machines
  with different linkers, pass `RUSTFLAGS="-C link-arg=-Wl,--build-id=none"`.
- **mimalloc vendor objects.** mimalloc 0.1 vendors C objects whose
  determinism depends on the host C compiler. Pin to the same `cc`
  version (`cc --version`) across operators to avoid variance.
- **Filesystem ordering inside tarballs.** `cargo dist` uses
  reproducible-tar settings by default, but if you produce tarballs by
  hand, prefer `tar --sort=name --mtime=@${SOURCE_DATE_EPOCH}
  --owner=0 --group=0 --numeric-owner`.

When silksurf's release matures past the bring-up phase, these will
move from "known and documented" to "verified by the local-gate".

## Related

- `scripts/release.sh` -- the tag-time release driver.
- `scripts/generate_sbom.sh` -- CycloneDX SBOM generation.
- `docs/development/CROSS-COMPILE.md` -- cross-target build flow.
- `docs/development/LOCAL-GATE.md` -- merge gate definition.
- ADR-008 (`docs/design/ARCHITECTURE-DECISIONS.md`) -- toolchain pinning.
- ADR-009 (`docs/design/ARCHITECTURE-DECISIONS.md`) -- strict-local CI.
- https://reproducible-builds.org/specs/source-date-epoch/ --
  upstream `SOURCE_DATE_EPOCH` specification.
