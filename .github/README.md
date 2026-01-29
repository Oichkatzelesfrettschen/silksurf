# GitHub Actions CI/CD

This directory contains automated workflows for continuous integration and quality assurance.

## Workflow: `ci.yml`

Comprehensive CI pipeline running on every push to `main`/`develop` and all pull requests.

### Build Jobs (Parallel)

**build-debug**: Standard debug build with assertions and debug symbols
- Configuration: `-DCMAKE_BUILD_TYPE=Debug`
- Artifacts: test executables, silksurf binary (1 day retention)

**build-release**: Optimized production build
- Configuration: `-DCMAKE_BUILD_TYPE=Release -O3 -march=native`
- Artifacts: stripped silksurf binary (7 day retention)
- Checks: binary size validation (target: <2MB)

**build-asan**: AddressSanitizer build for memory error detection
- Configuration: `-DSANITIZER=address`
- Detects: buffer overflows, use-after-free, memory leaks
- Artifacts: test executables with ASAN instrumentation

**build-ubsan**: UndefinedBehaviorSanitizer build
- Configuration: `-DSANITIZER=undefined`
- Detects: integer overflow, null pointer dereference, misaligned access
- Artifacts: test executables with UBSAN instrumentation

### Test Jobs (Sequential after builds)

**test**: Full test suite with CTest
- Runs: `ctest --test-dir build --output-on-failure`
- Reports: test results in GitHub Actions summary
- Tests: parser_basic, dom_parsing, css_engine, css_cascade, simd_detection

**test-asan**: Memory safety tests with AddressSanitizer
- Runs C-only tests (excluding Rust engine)
- Validates: no buffer overflows, no use-after-free
- Expected: libdom leaks documented, SilkSurf code clean

**test-ubsan**: Undefined behavior tests
- Runs C-only tests with UBSAN instrumentation
- Validates: no integer overflow, no null dereferences
- Expected: zero UBSAN violations

### Lint Jobs (Parallel)

**lint-clang-tidy**: Static analysis with clang-tidy
- Checks: bugprone, clang-analyzer, readability, performance
- Configuration: `-warnings-as-errors='*'` (fail on any warning)
- Scope: all `src/*.c` files except fuzz harnesses

**lint-cppcheck**: Additional static analysis
- Checks: warning, style, performance, portability
- Configuration: `--error-exitcode=1` (fail on errors)
- Scope: entire `src/` directory

### Fuzzing Job

**fuzz**: AFL++ fuzzing campaign (5 minutes per commit)
- Configuration: `-DENABLE_FUZZING=ON` with afl-clang-fast
- Target: `silksurf_fuzz` (HTML parser fuzzing)
- Timeout: 300 seconds (smoke test)
- Failure condition: any crashes discovered
- Artifacts: crash inputs uploaded on failure

Production readiness target: 24 hours with zero crashes

### Performance Job

**performance**: Performance metrics and regression tracking
- Binary size validation: target <2MB (currently ~2MB stripped)
- SIMD detection verification: confirms SSE2/AVX2 support
- Future: startup time, memory usage, FPS benchmarks

### Status Check

**ci-success**: Aggregate status job
- Depends on: all build, test, lint, fuzz, performance jobs
- Fails if: any dependency job fails
- Reports: success summary to GitHub Actions

## Local Testing

Run CI checks locally before pushing:

```bash
# Build all configurations
cmake -B build -DCMAKE_BUILD_TYPE=Debug && cmake --build build
cmake -B build-release -DCMAKE_BUILD_TYPE=Release && cmake --build build-release
cmake -B build-asan -DSANITIZER=address && cmake --build build-asan
cmake -B build-fuzz -DENABLE_FUZZING=ON && cmake --build build-fuzz

# Run tests
ctest --test-dir build --output-on-failure

# Run sanitizer tests
./build-asan/test_dom_parsing
./build-asan/test_css_engine

# Run linters
clang-tidy -p build src/**/*.c
cppcheck --enable=all src/

# Run fuzzing (5 min smoke test)
AFL_I_DONT_CARE_ABOUT_MISSING_CRASHES=1 timeout 300 \
  afl-fuzz -i fuzz_corpus/html/input -o /tmp/fuzz -m none \
  -- ./build-fuzz/silksurf_fuzz || true
```

## Quality Gates

All jobs must pass before merging:
- ✅ Zero compiler warnings (`-Werror` enforced)
- ✅ Zero memory leaks (ASAN clean, expected libdom leaks documented)
- ✅ Zero undefined behavior (UBSAN clean)
- ✅ Zero static analysis errors (clang-tidy, cppcheck)
- ✅ Zero fuzzing crashes (5 min smoke test)
- ✅ 100% critical test passing (parser, DOM, CSS, SIMD)

## Future Enhancements

Planned additions to CI pipeline:

1. **Coverage tracking**: Generate and upload code coverage reports
2. **Test262 integration**: JavaScript engine conformance testing
3. **Performance benchmarks**: Track startup time, memory, FPS over commits
4. **Nightly fuzzing**: 24-hour AFL++ campaigns on schedule
5. **Release automation**: Automatic binary builds and GitHub releases
6. **Documentation builds**: Render and deploy API docs

## Troubleshooting

**Sanitizer builds fail on Rust code**: This is expected due to Rust nightly
unsafe attribute errors (Task #33). Build only C targets with:
```bash
cmake --build build-asan --target test_dom_parsing test_css_engine
```

**Fuzzing crashes on core_pattern**: Set environment variable:
```bash
export AFL_I_DONT_CARE_ABOUT_MISSING_CRASHES=1
```

**clang-tidy warnings differ from local**: Ensure clang-tidy version matches CI
(Ubuntu 24.04 uses clang-tidy-18).

## References

- GitHub Actions docs: https://docs.github.com/actions
- AFL++ docs: https://aflplus.plus/docs/
- AddressSanitizer: https://github.com/google/sanitizers/wiki/AddressSanitizer
- clang-tidy checks: https://clang.llvm.org/extra/clang-tidy/checks/
