# SilkSurf Technical Debt Audit (2026-04-06)

## Scope and source of truth
- Canonical sources: `docs/`, `silksurf-specification/`, CI policy (local-first/local-only; manual `workflow_dispatch` cloud entrypoint), and workspace manifests.
- External evidence: crates.io/cargo tooling, RustSec, OSV, deps.dev, and upstream repository metadata.

## Debt register (current)
| Category | Finding | Action taken | Status |
| --- | --- | --- | --- |
| Toolchain debt | Nightly pin drift between repo/docs/CI | Pinned `rust-toolchain.toml` and CI to `nightly-2026-04-05`; synced toolchain docs | Resolved |
| Build policy debt | Rust warning gate was not enforced in CI | Added `lint-rust` CI job with `RUSTFLAGS='-D warnings'` check and strict clippy bug/complexity lint gate | Resolved |
| Policy-noise debt | `cargo deny` emitted `license-not-encountered` warning noise in workspace policy output | Cleaned root `deny.toml` license config so policy checks run warning-free in current workspace state | Resolved |
| Dependency security debt | `bincode` flagged unmaintained (RustSec) | Removed `bincode` usage from engine cache serialization; migrated cache serialization to `serde_json` | Resolved |
| Dependency overlap debt | Multiple unused/stub deps and feature flags | Removed unused deps/features across workspace manifests (css/layout/net/render/app/engine/js/html) | Resolved |
| Implementation debt | Criterion deprecated `black_box` usage in benches | Migrated to `std::hint::black_box` in JS benches | Resolved |
| Governance debt | No workspace-level deny policy | Added root `deny.toml`; wired advisories/bans/licenses/sources checks | Resolved |
| Lint hotspot debt | Non-JS clippy hotspots in engine/app/tests required cleanup for hard-gate stability | Applied targeted non-JS fixes across engine/app/tests; strict clippy deny set now clean for this scope | Resolved |
| Transitive duplication debt | `hashbrown` was previously duplicated via transitive `lasso` path | Removed lasso-based path from active workspace graph; duplicate-version debt cleared in current `cargo tree -d` results | Resolved |
| Tooling manifest debt | `fuzz/Cargo.toml` previously triggered cargo-machete structural warning | Fixed fuzz manifest structure; `cargo machete --with-metadata` now reports no warning in workspace audit run | Resolved |

## Quality gates (current)
- `RUSTFLAGS='-D warnings' cargo check --workspace --all-targets` passes.
- `cargo clippy --workspace --all-targets -- -D clippy::correctness -D clippy::suspicious -D clippy::perf -D clippy::complexity` passes.
- `cargo test --workspace` passes.
- `cargo deny check advisories bans licenses sources` passes clean (no policy-warning noise in this wave).
- `cmake -B build && cmake --build build && ctest --test-dir build` passes.

## Notes on crate fitness
- Remaining direct dependencies are actively maintained and currently up to date.
- No direct GitLab-hosted dependency repositories were found in workspace direct dependencies.
- Full dependency evidence and command outputs are in `docs/archive/dependencies/CRATE_AUDIT_2026-04-06.md`.
- CSS-focused pure-Rust landscape and tiered recommendations are tracked in `docs/archive/dependencies/CSS_CRATE_LANDSCAPE_2026-04-06.md` (analysis-only, no dependency additions in this pass).
- CI scope note: routine gating is local-first/local-only; cloud CI remains manual-only via `workflow_dispatch` (no push/PR auto gates in this policy wave).
