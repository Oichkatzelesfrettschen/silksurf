# AFL++ Fuzzing Strategy for Boa JavaScript Engine
**Date**: 2025-12-30
**Purpose**: Identify optimization opportunities and edge cases before cleanroom implementation

---

## EXECUTIVE SUMMARY

**Goal**: Fuzz Boa v0.21 to discover:
1. Crashes/panics (stability issues to avoid)
2. Slowdowns (pathological performance cases)
3. Coverage gaps (incomplete testing)
4. Edge cases (unusual code patterns to handle)

**Strategy**: Use AFL++ with three specialized fuzz targets (parser, compiler, eval) and a diverse corpus from Test262, QuickJS, and real npm packages.

**Duration**: 10-minute quick fuzzing session per target (30 minutes total)

**Success Metrics**:
- Coverage: >90% code coverage in parser/compiler/runtime
- Crashes: Document all panics/crashes found
- Slowdowns: Identify inputs causing >10x slowdown
- Insights: Categorize findings for cleanroom avoidance

---

## FUZZ TARGETS

### Target 1: Parser (`fuzz_parser`)

**Purpose**: Find pathological parsing cases

**What It Tests**:
- Lexer tokenization (string handling, UTF-8 edge cases)
- Parser AST construction (deeply nested expressions, stack overflow)
- Error recovery (malformed syntax, incomplete programs)
- Memory safety (buffer overflows, use-after-free in AST building)

**Expected Findings**:
- Stack overflow on deeply nested expressions (e.g., 10,000 nested functions)
- Panic on unusual Unicode characters in identifiers
- Slowdown on ambiguous grammar constructs
- Out-of-memory on pathologically large AST nodes

**Corpus**:
- 100 sampled Test262 tests (diverse language features)
- Hand-crafted edge cases (deeply nested, Unicode stress)
- Mutation: AST-aware (swap operators, add nesting, change identifiers)

---

### Target 2: Compiler (`fuzz_compiler`)

**Purpose**: Find bytecode compilation bottlenecks

**What It Tests**:
- AST → bytecode translation (opcode selection, stack size calculation)
- Optimization passes (constant folding, dead code elimination)
- Stack size estimation (prevent runtime stack overflows)
- Instruction selection (inefficient bytecode patterns)

**Expected Findings**:
- Inefficient bytecode generation (10+ opcodes for simple operations)
- Incorrect stack size calculation (runtime stack underflow/overflow)
- Slow compilation on certain AST patterns (loops, closures, generators)
- Missing optimization opportunities (constant expressions not folded)

**Corpus**:
- Passing Test262 tests (valid ASTs only)
- Complex control flow (try/catch, async/await, generators)
- Mutation: Semantic (add/remove scopes, change variable usage)

---

### Target 3: Evaluation (`fuzz_eval`)

**Purpose**: Find runtime performance cliffs and GC pressure

**What It Tests**:
- Bytecode execution (VM dispatch, instruction implementation)
- Garbage collection (allocation pressure, collection frequency)
- Built-in functions (Object, Array, String, Function methods)
- Memory leaks (unreachable objects not collected)

**Expected Findings**:
- GC thrashing on certain allocation patterns
- Infinite loops (fuzzer timeout needed)
- Slow built-in methods (e.g., Array.sort on large arrays)
- Memory leaks on closures or cyclic references

**Corpus**:
- Passing Test262 tests (valid, executable programs)
- Real-world code (React, Lodash, npm packages)
- Mutation: Execution-aware (change loop bounds, add recursion)

---

## CORPUS STRATEGY

### Initial Seeds

**Test262 Subset** (100 tests):
- Location: `/home/eirikr/Github/silksurf/diff-analysis/tools-output/afl-corpus/parser/seeds/`
- Sampled from: `test262/test/language/**/*.js`
- Coverage: ES5-ES15 features (classes, async/await, destructuring, etc.)
- Quality: Official compliance tests (known-good JavaScript)

**QuickJS Test Suite** (TBD):
- Location: `/home/eirikr/Github/silksurf/silksurf-extras/quickjs/tests/`
- Coverage: Additional edge cases not in Test262
- Focus: Performance stress tests (deep recursion, large arrays)

**Real-World Corpus** (TBD):
- Source: Top 100 npm packages (minified + original)
- Examples: React, Lodash, Express, Webpack, Babel
- Purpose: Real-world code patterns (not just spec compliance)

### Mutation Strategy

**AFL++ Default Mutations**:
- Bit flips (brute-force byte mutations)
- Arithmetic mutations (increment/decrement values)
- Block operations (insert, delete, swap chunks)
- Dictionary-based (inject common JavaScript keywords)

**Syntax-Aware Mutations** (Future Enhancement):
- Parse input → mutate AST → serialize back to JavaScript
- Examples:
  - Swap operators (`+` → `-`, `&&` → `||`)
  - Add nesting (wrap expression in function)
  - Change identifiers (rename variables)
  - Insert control flow (add `if`, `while`, `try/catch`)

---

## EXECUTION PLAN

### Phase 1: Build Fuzz Targets (Week 1, Day 1)

- ✅ Create `fuzz_parser.rs`, `fuzz_compiler.rs`, `fuzz_eval.rs`
- ✅ Update `fuzz/Cargo.toml` with dependencies
- ✅ Exclude `fuzz` from workspace in root `Cargo.toml`
- 🔄 Build all three targets with `cargo fuzz build`
- ✅ Create corpus directory structure

### Phase 2: Prepare Corpus (Week 1, Day 1)

- ✅ Sample 100 Test262 tests for parser seeds
- ⏳ Copy passing Test262 tests for compiler/eval seeds
- ⏳ Add QuickJS test suite to corpus
- ⏳ Download and extract top 100 npm packages
- ⏳ Create AFL++ dictionary (JavaScript keywords)

### Phase 3: Run Quick Fuzzing Sessions (Week 1, Day 1)

**Parser Fuzzing** (10 minutes):
```bash
cd /home/eirikr/Github/silksurf/silksurf-extras/boa
timeout 600 cargo fuzz run fuzz_parser \
  --release \
  --jobs $(nproc) \
  -- \
  -max_total_time=600 \
  -timeout=5 \
  -max_len=65536 \
  /home/eirikr/Github/silksurf/diff-analysis/tools-output/afl-corpus/parser/seeds
```

**Compiler Fuzzing** (10 minutes):
```bash
timeout 600 cargo fuzz run fuzz_compiler --release --jobs $(nproc) -- -max_total_time=600 -timeout=10 ...
```

**Eval Fuzzing** (10 minutes):
```bash
timeout 600 cargo fuzz run fuzz_eval --release --jobs $(nproc) -- -max_total_time=600 -timeout=15 ...
```

**Parameters**:
- `-t 5000`: 5s timeout (prevent hangs)
- `-m none`: No memory limit (Boa uses a lot of RAM)
- `--jobs $(nproc)`: Parallel fuzzing (all CPU cores)
- `--release`: Optimized builds (faster execution)

### Phase 4: Analyze Results (Week 1, Day 5)

**Coverage Analysis**:
```bash
cargo fuzz coverage fuzz_parser
llvm-cov report fuzz_parser > coverage-parser.txt
```

**Crash Analysis**:
```bash
ls /home/eirikr/Github/silksurf/diff-analysis/tools-output/afl-corpus/parser/crashes/
# Reproduce each crash:
cargo fuzz run fuzz_parser crashes/crash-abc123
```

**Slowdown Analysis**:
```bash
# Find slow inputs (took >1s):
ls /home/eirikr/Github/silksurf/diff-analysis/tools-output/afl-corpus/parser/hangs/
# Profile each slow input:
perf record --call-graph dwarf -- cargo fuzz run fuzz_parser hangs/hang-xyz789
perf report
```

---

## SUCCESS METRICS

### Quantitative Metrics

| Metric | Parser | Compiler | Eval | Notes |
|--------|--------|----------|------|-------|
| Code Coverage | >90% | >85% | >80% | Lines executed during fuzzing |
| Crashes Found | <10 | <5 | <5 | Panics, segfaults, aborts |
| Hangs Found | <20 | <10 | <50 | Inputs taking >timeout |
| Corpus Size | 1000+ | 500+ | 300+ | Unique inputs found by AFL++ |
| Exec Speed | >500/s | >200/s | >50/s | Executions per second |

### Qualitative Insights

**Categorize Findings**:
1. **Avoidable Issues**: Patterns cleanroom should avoid (e.g., regex catastrophic backtracking)
2. **Architectural Flaws**: Design decisions causing slowdowns (e.g., string interning overhead)
3. **Optimization Opportunities**: Hot paths to optimize in cleanroom (e.g., property lookup)
4. **Edge Cases**: Rare corner cases to handle explicitly (e.g., Unicode normalization)

**Deliverable**: Document for cleanroom team with:
- Top 10 performance cliffs to avoid
- Top 5 architectural patterns to improve
- Critical edge cases requiring explicit handling

---

## RISK MITIGATION

### Risk: Fuzzing Finds Too Many Issues

**Mitigation**:
- Prioritize by severity (crashes > hangs > slowdowns)
- Focus on cleanroom-relevant findings (ignore Boa-specific bugs)
- Defer low-priority edge cases to Phase 2+

### Risk: Low Code Coverage (<90%)

**Mitigation**:
- Add targeted seeds for uncovered paths
- Use AFL++ dictionary with JavaScript keywords
- Run longer campaigns (48hr instead of 24hr)

### Risk: No Significant Findings

**Mitigation**:
- Boa is well-tested (expected outcome)
- Document absence of findings (validates cleanroom viability)
- Focus on profiling instead (perf/flamegraph more valuable)

---

## DELIVERABLES

### Files Created

1. ✅ `fuzz/fuzz_targets/fuzz_parser.rs` - Parser fuzzer
2. ✅ `fuzz/fuzz_targets/fuzz_compiler.rs` - Compiler fuzzer
3. ✅ `fuzz/fuzz_targets/fuzz_eval.rs` - Eval fuzzer
4. ✅ `fuzz/Cargo.toml` - Updated dependencies
5. ✅ Corpus directory structure (`afl-corpus/`)
6. ✅ 100 seed files from Test262

### Reports to Generate

1. ⏳ `AFL-PARSER-REPORT.md` - Parser fuzzing results
2. ⏳ `AFL-COMPILER-REPORT.md` - Compiler fuzzing results
3. ⏳ `AFL-EVAL-REPORT.md` - Eval fuzzing results
4. ⏳ `AFL-INSIGHTS.md` - Consolidated insights for cleanroom

---

## NEXT STEPS

1. ✅ **COMPLETE**: Create fuzz targets and corpus structure
2. 🔄 **IN PROGRESS**: Build fuzz targets with cargo-fuzz
3. ⏳ **NEXT**: Verify targets execute correctly (smoke test)
4. ⏳ **NEXT**: Expand corpus (QuickJS + npm packages)
5. ⏳ **NEXT**: Launch 24hr parser fuzzing campaign
6. ⏳ **NEXT**: Analyze parser results and document findings
7. ⏳ **NEXT**: Launch compiler + eval campaigns
8. ⏳ **NEXT**: Consolidate insights for cleanroom design

---

## TIMELINE

**Week 1, Day 1** (Today):
- ✅ Create fuzz infrastructure
- 🔄 Build fuzz targets
- ⏳ Prepare corpus

**Week 1, Days 2-4**:
- Run 24hr fuzzing campaigns (3 targets)
- Monitor progress, adjust timeouts

**Week 1, Day 5**:
- Analyze results (crashes, hangs, coverage)
- Generate reports and insights

**Week 1, Days 6-7**:
- Performance profiling (perf, flamegraph, heaptrack)
- Consolidate findings into cleanroom design doc

---

## CONCLUSION

AFL++ fuzzing will provide critical intelligence for cleanroom implementation:
- Identify performance cliffs to avoid
- Discover edge cases requiring explicit handling
- Validate Boa's stability (0 panics expected)
- Inform architectural decisions (arena allocation, zero-copy parsing)

**Expected Outcome**: <10 crashes, <50 hangs, >90% coverage, actionable insights for cleanroom design.

**Timeline**: 5 days to complete fuzzing + analysis, ready for cleanroom implementation Week 2.
