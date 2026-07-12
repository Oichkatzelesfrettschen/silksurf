# Makefile -- canonical build entry point for silksurf.
#
# The Makefile presents the public build, lint, test, conformance, and release
# interface. Scripts under scripts/ carry target implementation details.
#
#   make check   -- fast gate: fmt + clippy -D warnings + lint_unwrap
#                   + lint_unsafe + lint_glossary + lint_doc_links
#                   + lint_cleanroom
#   make test    -- workspace tests with -D warnings
#   make full    -- check + test + cargo deny + cargo doc
#   make fmt     -- auto-format all Rust and C sources
#   make clean   -- remove build artifacts (Rust + historical harness dirs)
#   make doc     -- build rustdoc
#   make hooks   -- install git pre-commit/pre-push hooks
#   make cross   -- cross-compile to aarch64
#   make miri    -- miri smoke (requires nightly + miri component)
#   make fuzz    -- cargo-fuzz smoke, 30s per target (requires cargo-fuzz)
#   make bench   -- run the benchmark suite
#   make gui-probe -- run the live winit GUI probe when a display is present
#   make gui-probe-o0 -- run the live GUI probe with opt-level 0
#   make gui-probe-o0-ai-chat -- run the O0 AI-chat page-input canary
#   make conformance-html-css -- run the HTML/CSS harness subset
#   make verify-conformance-sources -- verify retained HTML/CSS source bytes
#   make fetch-conformance-sources -- refresh retained HTML/CSS source bundle
#   make fetch-conformance-test-corpora -- fetch ignored external test corpora
#   make release   -- guarded release (requires explicit VERSION=x.y.z)
#
# All Rust targets pass RUSTFLAGS='-D warnings'. This is the project policy.
# Do NOT set RUSTFLAGS globally in .cargo/config.toml -- that breaks IDEs.
#
# See docs/development/LOCAL-GATE.md for the full reference.

# ---------------------------------------------------------------------------
# Tooling flags
# ---------------------------------------------------------------------------

RUSTFLAGS_DENY := RUSTFLAGS='-D warnings'

CLIPPY_DENY := \
    -D clippy::correctness \
    -D clippy::suspicious \
    -D clippy::perf \
    -D clippy::complexity

# Read MSRV from workspace; single source of truth.
MSRV := $(shell awk -F'"' '/^rust-version =/ {print $$2; exit}' Cargo.toml)

# Build-tree hygiene. The build/ and build-* directories, infer-out, and
# fuzz_out* trees are historical C-harness outputs (AD-024, harness removed);
# clean keeps removing them so stale checkouts converge.
CARGO_TARGET_DIRS = target silksurf-js/target
BUILD_ARTIFACT_DIRS = build build-* infer-out perf/results \
                      fuzz_out fuzz_out_css fuzz/artifacts logs/cores
BIN           ?= bench_pipeline
CRATE         ?= silksurf-engine
PERF_OPTS     ?= -e cycles:u -j any,u
PERF2BOLT_OPTS ?=
BOLT_OPTS     ?= -reorder-blocks=ext-tsp -reorder-functions=cdsort \
                 -split-functions -icf -use-gnu-stack

# ---------------------------------------------------------------------------
# Default target
# ---------------------------------------------------------------------------

.DEFAULT_GOAL := check

# ---------------------------------------------------------------------------
# Rust targets (primary)
# ---------------------------------------------------------------------------

.PHONY: check test full fmt doc clean clean-cargo clean-build-artifacts hooks cross miri fuzz bench gui-probe gui-probe-o0 gui-probe-o0-ai-chat conformance conformance-html-css verify-conformance-sources fetch-conformance-sources fetch-conformance-test-corpora release

# Fast gate: format check + clippy -D warnings + lint helpers.
# Wired into pre-commit hook.
check:
	@echo "==> rustfmt check"
	cargo fmt --all -- --check
	@echo "==> clippy -D warnings"
	$(RUSTFLAGS_DENY) cargo clippy --workspace --all-targets -- $(CLIPPY_DENY)
	@if [ -x scripts/lint_unwrap.sh ]; then scripts/lint_unwrap.sh; fi
	@if [ -x scripts/lint_unsafe.sh ]; then scripts/lint_unsafe.sh; fi
	@if [ -x scripts/lint_glossary.sh ]; then scripts/lint_glossary.sh; fi
	@if [ -x scripts/lint_doc_links.sh ]; then scripts/lint_doc_links.sh; fi
	@if [ -x scripts/lint_cleanroom.sh ]; then scripts/lint_cleanroom.sh; fi

# Workspace tests with -D warnings.
# Wired into full gate (pre-push hook) via the full target.
test:
	@echo "==> workspace tests -D warnings"
	$(RUSTFLAGS_DENY) cargo test --workspace

# Full gate: check + test + deny + doc.
# Wired into pre-push hook.
# Optional: MIRI=1 make full  or  FUZZ=1 make full
full: check test deny doc
ifeq ($(MIRI),1)
	$(MAKE) miri
endif
ifeq ($(FUZZ),1)
	$(MAKE) fuzz
endif
	@echo
	@echo "OK: make full passed."

# Auto-format all Rust sources.
fmt:
	cargo fmt --all

# Dependency policy check.
deny:
	@echo "==> cargo deny"
	@if command -v cargo-deny >/dev/null 2>&1; then \
	    cargo deny check advisories bans licenses sources; \
	else \
	    echo "    (cargo-deny not installed; skipping. Install: cargo install cargo-deny)"; \
	fi

# Build rustdoc. RUSTDOCFLAGS='-D warnings' promotes all rustdoc warnings to errors.
doc:
	@echo "==> cargo doc"
	RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --document-private-items

# Remove Cargo outputs, build trees, runtime logs, and generated artifacts.
clean: clean-cargo clean-build-artifacts
	@mkdir -p logs/cores

# cargo clean owns the workspace target tree; the explicit removals cover
# historical nested target directories created by running Cargo in subtrees.
clean-cargo:
	cargo clean
	@for dir in $(CARGO_TARGET_DIRS); do \
	    if [ -d "$$dir" ]; then rm -rf "$$dir"; fi; \
	done

# Build artifact directories stay reproducible and untracked.
clean-build-artifacts:
	@rm -rf $(BUILD_ARTIFACT_DIRS)
	@rm -f core core.*

# Install git pre-commit / pre-push hooks.
hooks:
	scripts/install-git-hooks.sh

# Cross-compile to x86_64 + aarch64-unknown-linux-gnu.
cross:
	scripts/cross_build.sh

# Miri smoke (opt-in; requires: rustup toolchain install nightly --component miri).
miri:
	@echo "==> miri smoke"
	@rustup +nightly component list --installed 2>/dev/null | grep -q '^miri' \
	    || (echo "miri not installed on nightly. Run: rustup toolchain install nightly --component miri" && exit 1)
	cargo +nightly miri test -p silksurf-core -p silksurf-css --lib

# Fuzz smoke (opt-in; requires: cargo install cargo-fuzz).
fuzz:
	@echo "==> cargo-fuzz smoke (30s per target)"
	@command -v cargo-fuzz >/dev/null 2>&1 \
	    || (echo "cargo-fuzz not installed. Run: cargo install cargo-fuzz" && exit 1)
	@for target in html_tokenizer html_tree_builder css_tokenizer css_parser js_runtime; do \
	    echo "    -- $$target"; \
	    cargo +nightly fuzz run "$$target" -- -max_total_time=30 -runs=200000 || true; \
	done

# Benchmark suite.
bench:
	@echo "==> cargo bench"
	$(RUSTFLAGS_DENY) cargo bench --workspace

GUI_PROBE_ARGS ?= --release --backend auto
GUI_PROBE_O0_ARGS ?= --o0 --backend auto
GUI_PROBE_O0_AI_CHAT_ARGS ?= --o0 --backend auto --presenter auto --fixture ai-chat --probe page-input --runs 3 --timeout-seconds 60

# Live GUI smoke. This target requires a working Wayland or X11 session.
gui-probe:
	scripts/gui_probe.sh $(GUI_PROBE_ARGS)

# Live GUI O0 smoke. This target exercises the custom dev-o0 Cargo profile.
gui-probe-o0:
	scripts/gui_probe.sh $(GUI_PROBE_O0_ARGS)

# O0 browser-latency canary for the AI-chat fixture.
gui-probe-o0-ai-chat:
	scripts/gui_probe.sh $(GUI_PROBE_O0_AI_CHAT_ARGS)

# Run the published conformance harness set.
conformance:
	scripts/conformance_run.sh

# Run the HTML/CSS parser and synthetic browser-engine subset.
conformance-html-css:
	scripts/conformance_run.sh html5lib css wpt

# Verify retained primary source bytes without network access.
verify-conformance-sources:
	scripts/verify_html_css_conformance_sources.sh

# Refresh retained HTML/CSS source material with the recorded Mozilla UA.
fetch-conformance-sources:
	scripts/fetch_html_css_conformance_sources.sh

# fetch-conformance-test-corpora keeps external HTML/CSS tests under silksurf-extras/.
fetch-conformance-test-corpora:
	scripts/fetch_html_css_test_corpora.sh

# Guarded release. Requires VERSION=x.y.z on the command line.
release:
	@[ -n "$(VERSION)" ] || (echo "Usage: make release VERSION=x.y.z" && exit 1)
	VERSION=$(VERSION) scripts/release.sh

# ---------------------------------------------------------------------------
# Optimized-build and perf targets (Rust)
# ---------------------------------------------------------------------------

.PHONY: riced-build pgo-train bolt-opt \
        perf-guardrails perf-baselines perf-all \
        local-gate-fast local-gate-full local-gate core-dumps

core-dumps:
	mkdir -p logs/cores
	@mv -f core core.* logs/cores/ 2>/dev/null || true

riced-build:
	EXTRA_RUSTFLAGS='-D warnings' scripts/riced_build.sh -p $(CRATE) --bin $(BIN)

pgo-train:
	CRATE="$(CRATE)" EXTRA_RUSTFLAGS='-D warnings' scripts/pgo_build.sh $(BIN)

bolt-opt:
	CRATE="$(CRATE)" EXTRA_RUSTFLAGS='-D warnings' \
	    PERF_OPTS="$(PERF_OPTS)" PERF2BOLT_OPTS="$(PERF2BOLT_OPTS)" \
	    BOLT_OPTS="$(BOLT_OPTS)" scripts/bolt_build.sh $(BIN)

perf-guardrails:
	$(RUSTFLAGS_DENY) python3 scripts/perf_guardrails.py

perf-baselines:
	./perf/run_baselines.sh

perf-all: perf-guardrails perf-baselines

# Backward-compat aliases -- prefer make check / make full going forward.
local-gate-fast: check
local-gate-full: full
local-gate: full
