# Crate Audit Deep Report (2026-04-06)

## Audit method
1. Upgrade pass via `cargo upgrade --incompatible` and lockfile refresh via `cargo update`.
2. Currency checks via `cargo outdated --workspace`.
3. Security checks via `cargo audit`, OSV batch API, and deps.dev advisory-key API.
4. Policy checks via `cargo deny check advisories bans licenses sources`.
5. Redundancy checks via `cargo machete --with-metadata` and `cargo tree -d`.
6. Build integrity checks via strict warning and test gates.

## High-impact remediations

### Toolchain and CI
- Pinned nightly to `nightly-2026-04-05` in:
  - `rust-toolchain.toml`
  - `.github/workflows/ci.yml`
  - `docs/TOOLCHAIN.md`
  - `docs/ARCHITECTURE.md`
  - `docs/development/BUILD.md`
- Added `lint-rust` job to CI with:
  - `cargo fmt --all -- --check`
  - `RUSTFLAGS='-D warnings' cargo check --workspace --all-targets`
  - `cargo clippy --workspace --all-targets -- -D clippy::correctness -D clippy::suspicious -D clippy::perf -D clippy::complexity`

### Dependency modernization
- Upgraded workspace and crate requirements to latest compatible/incompatible sets where valid.
- Removed unimplemented or unused dependency surfaces:
  - `silksurf-css`: removed unused `bincode`, `dashmap`, `rayon` and dead `parallel` feature
  - `silksurf-layout`: removed unused `aligned-vec`, `rayon` and dead `parallel`/`soa` features
  - `silksurf-net`: removed unused `flume`, `memmap2` and dead `async-fetch`/`cache` features
  - `silksurf-render`: removed unused `wide`
  - `silksurf-app`: removed unused direct `silksurf-net`/`silksurf-tls` deps (kept via engine feature path)
  - `silksurf-engine`: removed unused `serde`, `silksurf-gui`, `silksurf-js` optional deps and dead feature surface
  - `silksurf-js`: removed unused `regress`, `num-traits`, `phf_codegen`, `proptest`, `tracing-test`
  - `silksurf-html`: removed unused dev-dependency `serde`

### Security advisory remediation
- RustSec flagged `bincode` as unmaintained (`RUSTSEC-2025-0141`).
- Initially migrated to `postcard`; RustSec then flagged transitive `atomic-polyfill` unmaintained via `heapless`.
- Final fix: removed `bincode` and `postcard`, switched engine stylesheet disk cache serialization to `serde_json` (`from_slice` / `to_vec`).

## External-source audit outcomes

### RustSec (`cargo audit`)
- Final result: no vulnerability/unmaintained warnings in `Cargo.lock`.

### OSV (batch query for direct dependencies)
- Queried all direct external crate/version pairs in workspace.
- Result: `osv_vulns=0`.

### deps.dev (v3 package/version advisory keys)
- Queried all direct external crate/version pairs.
- Result: `deps_dev_advisories=0`.

### Upstream repository health (GitHub/GitLab)
- Enumerated direct dependency repository URLs from `cargo metadata`.
- Direct external set audited: 32 crate/version pairs.
- Host coverage:
  - GitHub: all direct external dependencies
  - GitLab: none in direct external dependency set
- GitHub API check on all 32 direct repos:
  - `archived=false`, `disabled=false` for all checked repos
  - Recent push activity observed across the set.

## Policy/governance
- Added workspace `deny.toml`.
- `cargo deny check advisories bans licenses sources` final status:
  - advisories: ok
  - bans: ok
  - licenses: ok
  - sources: ok

## Follow-up remediation (post-audit)
1. Removed the lasso-based transitive path that previously forced an older `hashbrown`; active workspace duplicate report now no longer lists `hashbrown` (`cargo tree -d`).
2. `fuzz/Cargo.toml` structure was corrected; `cargo machete --with-metadata` now runs without the prior manifest parse warning.
3. CI clippy gate was hardened to include `-D clippy::complexity`.
4. Root `deny.toml` was cleaned to remove `license-not-encountered` warning noise so `cargo deny check advisories bans licenses sources` is clean in current workspace runs.
5. Additional non-JS clippy hotspots were reduced in engine/app/tests to keep the targeted deny-set gate stable without expanding lint scope.
6. Interner microbench harnesses/baselines were added for `silksurf-core` and `silksurf-js`; canonical command list and representative medians are tracked in `docs/PERFORMANCE.md`.

## Residual debt and decisions
1. Full clippy `-D warnings --all-features` is intentionally not used as a hard gate due non-bug style lint noise and allocator-feature conflict; targeted deny set is enforced instead.
2. This wave intentionally avoided scheduled CI additions; no `schedule`-trigger job expansion is part of the remediation set.

## Validation commands used
- `cargo update`
- `RUSTFLAGS='-D warnings' cargo check --workspace --all-targets`
- `cargo clippy --workspace --all-targets -- -D clippy::correctness -D clippy::suspicious -D clippy::perf -D clippy::complexity`
- `cargo test --workspace`
- `cmake -B build && cmake --build build && ctest --test-dir build --output-on-failure`
- `cargo audit`
- `cargo outdated --workspace`
- `cargo deny check advisories bans licenses sources`
- `cargo tree -d`
- `cargo machete --with-metadata`
