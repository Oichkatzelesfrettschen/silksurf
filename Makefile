# ------------------------------------------------------------------------- 
# SilkSurf Root Makefile (CMake Wrapper) 
# ------------------------------------------------------------------------- 

BUILD_DIR = build
CORE_DUMP_DIR = logs/cores
RICING_FLAGS = -march=x86-64-v3 -O3 -flto -fomit-frame-pointer -fno-strict-aliasing -ftree-vectorize -D_SILK_NO_THREADS
GUI_LIBS = $(shell pkg-config --cflags --libs xcb xcb-damage xcb-composite libcss libdom libhubbub libparserutils)
CMAKE_FLAGS = -DCMAKE_EXPORT_COMPILE_COMMANDS=1 -DCMAKE_C_FLAGS="$(RICING_FLAGS)"
BIN ?= bench_pipeline
CRATE ?= silksurf-engine
EXTRA_RUSTFLAGS ?= -D warnings
PERF_OPTS ?= -e cycles:u -j any,u
PERF2BOLT_OPTS ?=
BOLT_OPTS ?= -reorder-blocks=ext-tsp -reorder-functions=cdsort -split-functions -icf -use-gnu-stack

all: build

$(BUILD_DIR)/Makefile:
	mkdir -p $(BUILD_DIR)
	cd $(BUILD_DIR) && cmake $(CMAKE_FLAGS) ..

build: $(BUILD_DIR)/Makefile
	$(MAKE) -C $(BUILD_DIR)

clean:
	rm -rf $(BUILD_DIR) $(CORE_DUMP_DIR)
	rm -f core core.*
	mkdir -p $(CORE_DUMP_DIR)

.PHONY: core-dumps

core-dumps:
	mkdir -p $(CORE_DUMP_DIR)
	@mv -f core core.* $(CORE_DUMP_DIR)/ 2>/dev/null || true

.PHONY: riced-build pgo-train bolt-opt

riced-build:
	EXTRA_RUSTFLAGS="$(EXTRA_RUSTFLAGS)" ./scripts/riced_build.sh -p $(CRATE) --bin $(BIN)

pgo-train:
	CRATE="$(CRATE)" EXTRA_RUSTFLAGS="$(EXTRA_RUSTFLAGS)" ./scripts/pgo_build.sh $(BIN)

bolt-opt:
	CRATE="$(CRATE)" EXTRA_RUSTFLAGS="$(EXTRA_RUSTFLAGS)" PERF_OPTS="$(PERF_OPTS)" PERF2BOLT_OPTS="$(PERF2BOLT_OPTS)" BOLT_OPTS="$(BOLT_OPTS)" ./scripts/bolt_build.sh $(BIN)

.PHONY: perf-guardrails

perf-guardrails:
	RUSTFLAGS="$(EXTRA_RUSTFLAGS)" python3 scripts/perf_guardrails.py

.PHONY: perf-baselines perf-all

perf-baselines:
	./perf/run_baselines.sh

perf-all: perf-guardrails perf-baselines

# ------------------------------------------------------------------------- 
# GUI & Rendering 
# ------------------------------------------------------------------------- 
.PHONY: gui

gui: clean
	@echo "Building Silksurf GUI (Style-Driven)..."
	mkdir -p $(BUILD_DIR)
	gcc -Iinclude -Isrc $(RICING_FLAGS) -g \
		src/gui/main_gui.c \
		src/gui/window.c \
		src/gui/xcb_wrapper.c \
		src/rendering/paint.c \
		src/css/cascade.c \
		src/document/css_engine.c \
		src/document/css_select_handler.c \
		src/css/selector.c \
		src/document/document.c \
		src/document/dom_node.c \
		src/memory/arena.c \
		-o build/silksurf_gui -lm \
		$(GUI_LIBS)

# ------------------------------------------------------------------------- 
# Neural Components (Phase 6) 
# ------------------------------------------------------------------------- 
.PHONY: bpe-bench

bpe-bench: clean
	@echo "Building Neural BPE Benchmark..."
	mkdir -p $(BUILD_DIR)
	gcc -Iinclude -Isrc $(RICING_FLAGS) -g \
		src/neural/bpe_bench.c \
		src/neural/bpe.c \
		src/memory/arena.c \
		-o build/bpe_bench -lm
	@echo "Running BPE Benchmark..."
	./build/bpe_bench

# ------------------------------------------------------------------------- 
# Static Analysis (Facebook Infer) 
# ------------------------------------------------------------------------- 
.PHONY: infer infer-diff infer-explore

infer: clean
	@echo "Starting Semantic Analysis with Facebook Infer..."
	mkdir -p $(BUILD_DIR)
	cd $(BUILD_DIR) && cmake $(CMAKE_FLAGS) ..
	infer run --report-console-limit 10 --compilation-database $(BUILD_DIR)/compile_commands.json

infer-diff: $(BUILD_DIR)/Makefile
	infer run --reactive --compilation-database $(BUILD_DIR)/compile_commands.json

infer-explore:
	infer explore --html

# ------------------------------------------------------------------------- 
# Layout Engine (Geometry & Box Model) 
# ------------------------------------------------------------------------- 
.PHONY: layout-test

layout-test:
	@echo "Building Layout Engine with UBSan..."
	mkdir -p $(BUILD_DIR)
	gcc -fsanitize=undefined -g -O2 src/layout/box_model.c -o build/layout_test
	@echo "Running Layout Test..."
	./build/layout_test

# ------------------------------------------------------------------------- 
# Dynamic Analysis (AFL++ Fuzzing) 
# ------------------------------------------------------------------------- 
.PHONY: fuzz-build fuzz-run css-fuzz-run

FUZZ_IN = fuzz_in
FUZZ_OUT = fuzz_out

fuzz-build: clean
	@echo "Building with AFL++ (LLVM-NATIVE) instrumentation, ASAN, and RICING flags..."
	mkdir -p $(BUILD_DIR)
	# LLVM-NATIVE uses compiler's built-in trace-pc-guard, bypassing plugin version issues
	AFL_USE_ASAN=1 AFL_LLVM_INSTRUMENT=NATIVE afl-cc -Iinclude -Isrc $(RICING_FLAGS) -g \
		src/fuzz_harness.c \
		src/document/html_tokenizer.c \
		src/memory/arena.c \
		-o build/silksurf_fuzz -lm
	@echo "Building CSS Fuzzer..."
	AFL_USE_ASAN=1 AFL_LLVM_INSTRUMENT=NATIVE afl-cc -Iinclude -Isrc $(RICING_FLAGS) -g \
		src/css/fuzz_css.c \
		src/css/css_tokenizer.c \
		src/document/css_engine.c \
		src/document/css_select_handler.c \
		src/css/selector.c \
		src/document/dom_node.c \
		src/document/document.c \
		src/memory/arena.c \
		-o build/silksurf_css_fuzz -lm \
		$(GUI_LIBS)

fuzz-run:
	mkdir -p $(FUZZ_IN)
	echo "<!DOCTYPE html><html><body>Test</body></html>" > $(FUZZ_IN)/basic.html
	echo "<div class='test'></div>" > $(FUZZ_IN)/div.html
	@echo "Starting HTML AFL++ in SMART MODE..."
	AFL_NO_UI=1 afl-fuzz -i $(FUZZ_IN) -o $(FUZZ_OUT) -- ./build/silksurf_fuzz

css-fuzz-run:
	mkdir -p fuzz_in_css
	echo "body { color: red; }" > fuzz_in_css/basic.css
	echo ".test > #id { width: 100px; padding: 10px; }" > fuzz_in_css/complex.css
	@echo "Starting CSS AFL++ in SMART MODE with Dictionary..."
	AFL_NO_UI=1 afl-fuzz -i fuzz_in_css -o fuzz_out_css -x fuzz_in/css.dict -- ./build/silksurf_css_fuzz

.PHONY: all build clean core-dumps riced-build pgo-train bolt-opt
