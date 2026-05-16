# Makefile -- canonical build entry point for silksurf.
#
# WHY: Centralises all build, lint, test, and release commands so contributors
# have a single discoverable interface. Scripts under scripts/ are implementation
# details invoked FROM targets here, not the public interface.
#
# HOW:
#   make check   -- fast gate: fmt + clippy -D warnings + lint_unwrap + lint_unsafe
#   make test    -- workspace tests with -D warnings
#   make full    -- check + test + cargo deny + cargo doc
#   make fmt     -- auto-format all Rust and C sources
#   make clean   -- remove build artefacts (Rust + CMake)
#   make doc     -- build rustdoc
#   make hooks   -- install git pre-commit/pre-push hooks
#   make cross   -- cross-compile to aarch64
#   make miri    -- miri smoke (requires nightly + miri component)
#   make fuzz    -- cargo-fuzz smoke, 30s per target (requires cargo-fuzz)
#   make bench   -- run the benchmark suite
#   make release -- guarded release (requires explicit VERSION=x.y.z)
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

# CMake / C legacy variables (Phase C tree, ADR-007)
BUILD_DIR      = build
RICING_FLAGS   = -march=x86-64-v3 -O3 -flto -fomit-frame-pointer \
                 -fno-strict-aliasing -ftree-vectorize -D_SILK_NO_THREADS
GUI_LIBS       = $(shell pkg-config --cflags --libs xcb xcb-damage xcb-composite \
                     libcss libdom libhubbub libparserutils 2>/dev/null)
CMAKE_FLAGS    = -DCMAKE_EXPORT_COMPILE_COMMANDS=1 -DCMAKE_C_FLAGS="$(RICING_FLAGS)"
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

.PHONY: check test full fmt doc clean hooks cross miri fuzz bench release

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
	@command -v cargo-deny >/dev/null 2>&1 \
	    && cargo deny check advisories bans licenses sources \
	    || echo "    (cargo-deny not installed; skipping. Install: cargo install cargo-deny)"

# Build rustdoc. RUSTDOCFLAGS='-D warnings' promotes all rustdoc warnings to errors.
doc:
	@echo "==> cargo doc"
	RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --document-private-items

# Remove all build artefacts.
clean:
	cargo clean
	@rm -rf $(BUILD_DIR) logs/cores
	@rm -f core core.*
	@mkdir -p logs/cores

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

# Guarded release. Requires VERSION=x.y.z on the command line.
release:
	@[ -n "$(VERSION)" ] || (echo "Usage: make release VERSION=x.y.z" && exit 1)
	VERSION=$(VERSION) scripts/release.sh

# ---------------------------------------------------------------------------
# Legacy CMake / C tree (ADR-007: deprecate or integrate decision pending)
# ---------------------------------------------------------------------------

.PHONY: cmake-build cmake-clean gui bpe-bench \
        infer infer-diff infer-explore layout-test \
        fuzz-build fuzz-run css-fuzz-run \
        riced-build pgo-train bolt-opt \
        perf-guardrails perf-baselines perf-all \
        local-gate-fast local-gate-full local-gate core-dumps

$(BUILD_DIR)/Makefile:
	mkdir -p $(BUILD_DIR)
	cd $(BUILD_DIR) && cmake $(CMAKE_FLAGS) ..

cmake-build: $(BUILD_DIR)/Makefile
	$(MAKE) -C $(BUILD_DIR)

cmake-clean:
	rm -rf $(BUILD_DIR) logs/cores
	rm -f core core.*
	mkdir -p logs/cores

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

gui:
	@echo "Building Silksurf GUI (C/XCB legacy)..."
	mkdir -p $(BUILD_DIR)
	gcc -Iinclude -Isrc $(RICING_FLAGS) -g \
	    src/gui/main_gui.c src/gui/window.c src/gui/xcb_wrapper.c \
	    src/rendering/paint.c src/css/cascade.c \
	    src/document/css_engine.c src/document/css_select_handler.c \
	    src/css/selector.c src/document/document.c src/document/dom_node.c \
	    src/memory/arena.c \
	    -o $(BUILD_DIR)/silksurf_gui -lm $(GUI_LIBS)

bpe-bench:
	@echo "Building Neural BPE Benchmark..."
	mkdir -p $(BUILD_DIR)
	gcc -Iinclude -Isrc $(RICING_FLAGS) -g \
	    src/neural/bpe_bench.c src/neural/bpe.c src/memory/arena.c \
	    -o $(BUILD_DIR)/bpe_bench -lm
	./$(BUILD_DIR)/bpe_bench

infer:
	mkdir -p $(BUILD_DIR)
	cd $(BUILD_DIR) && cmake $(CMAKE_FLAGS) ..
	infer run --report-console-limit 10 \
	    --compilation-database $(BUILD_DIR)/compile_commands.json

infer-diff: $(BUILD_DIR)/Makefile
	infer run --reactive \
	    --compilation-database $(BUILD_DIR)/compile_commands.json

infer-explore:
	infer explore --html

layout-test:
	mkdir -p $(BUILD_DIR)
	gcc -fsanitize=undefined -g -O2 src/layout/box_model.c -o $(BUILD_DIR)/layout_test
	./$(BUILD_DIR)/layout_test

FUZZ_IN  = fuzz_in
FUZZ_OUT = fuzz_out

fuzz-build:
	mkdir -p $(BUILD_DIR)
	AFL_USE_ASAN=1 AFL_LLVM_INSTRUMENT=NATIVE afl-cc -Iinclude -Isrc $(RICING_FLAGS) -g \
	    src/fuzz_harness.c src/document/html_tokenizer.c src/memory/arena.c \
	    -o $(BUILD_DIR)/silksurf_fuzz -lm
	AFL_USE_ASAN=1 AFL_LLVM_INSTRUMENT=NATIVE afl-cc -Iinclude -Isrc $(RICING_FLAGS) -g \
	    src/css/fuzz_css.c src/css/css_tokenizer.c src/document/css_engine.c \
	    src/document/css_select_handler.c src/css/selector.c \
	    src/document/dom_node.c src/document/document.c src/memory/arena.c \
	    -o $(BUILD_DIR)/silksurf_css_fuzz -lm $(GUI_LIBS)

fuzz-run:
	mkdir -p $(FUZZ_IN)
	echo "<!DOCTYPE html><html><body>Test</body></html>" > $(FUZZ_IN)/basic.html
	echo "<div class='test'></div>" > $(FUZZ_IN)/div.html
	AFL_NO_UI=1 afl-fuzz -i $(FUZZ_IN) -o $(FUZZ_OUT) -- ./$(BUILD_DIR)/silksurf_fuzz

css-fuzz-run:
	mkdir -p fuzz_in_css
	echo "body { color: red; }" > fuzz_in_css/basic.css
	echo ".test > #id { width: 100px; padding: 10px; }" > fuzz_in_css/complex.css
	AFL_NO_UI=1 afl-fuzz -i fuzz_in_css -o fuzz_out_css \
	    -x fuzz_in/css.dict -- ./$(BUILD_DIR)/silksurf_css_fuzz
