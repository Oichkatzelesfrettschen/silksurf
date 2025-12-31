# SilkSurf Testing Strategy
## Standardized Test Suites for HTML5 and CSS Compliance
### Date: 2025-12-30
### Purpose: Ensure maximum compliance with web standards

---

## Executive Summary

To achieve our goal of **maximum HTML5/CSS compliance**, we will integrate industry-standard test suites from W3C and WHATWG. These test suites represent the **de-facto standard for web engine compliance testing** used by all major browsers.

**Test Repositories:**
- **HTML5 Tokenizer:** [html5lib/html5lib-tests](https://github.com/html5lib/html5lib-tests) (de-facto standard)
- **CSS:** [w3c/web-platform-tests](https://github.com/w3c/web-platform-tests) (official W3C tests)

---

## HTML5 Tokenizer Tests (html5lib-tests)

### Repository
- **URL:** https://github.com/html5lib/html5lib-tests
- **License:** MIT License
- **Status:** De-facto standard, used by all major browser engines
- **Test Location:** `/tokenizer` directory

### Test Files Available

| File | Purpose | Complexity |
|------|---------|------------|
| `test1.test` | General tokenizer tests (part 1) | Core |
| `test2.test` | General tokenizer tests (part 2) | Core |
| `test3.test` | General tokenizer tests (part 3) | Core |
| `test4.test` | General tokenizer tests (part 4) | Core |
| `entities.test` | Entity parsing tests | High |
| `namedEntities.test` | Named entity recognition | High |
| `numericEntities.test` | Numeric entity handling | Medium |
| `contentModelFlags.test` | Content model flags | Medium |
| `escapeFlag.test` | Escape flag functionality | Medium |
| `unicodeChars.test` | Unicode character processing | High |
| `unicodeCharsProblematic.test` | Problematic Unicode scenarios | Very High |
| `domjs.test` | DOM JavaScript handling | Medium |
| `xmlViolation.test` | XML specification violations | Low |
| `pendingSpecChanges.test` | Anticipated spec updates | Low |

### Test Format (JSON)

```json
{
  "tests": [
    {
      "description": "Simple start tag",
      "input": "<div>",
      "output": [
        ["StartTag", "div", {}]
      ],
      "initialStates": ["Data state"],
      "lastStartTag": null,
      "errors": []
    },
    {
      "description": "Start tag with attribute",
      "input": "<div class=\"foo\">",
      "output": [
        ["StartTag", "div", {"class": "foo"}]
      ],
      "errors": []
    }
  ]
}
```

**Field Descriptions:**
- `description`: Human-readable test description
- `input`: Input string to tokenize (UTF-8, preprocessed per spec)
- `output`: Array of expected tokens
  - Token types: `DOCTYPE`, `StartTag`, `EndTag`, `Comment`, `Character`, `EndOfFile`
  - Format varies by token type
- `initialStates`: Optional initial tokenizer state(s)
- `lastStartTag`: Optional last emitted start tag name
- `errors`: Array of expected parse errors (with line/column)

**Error Format:**
```json
{
  "code": "unexpected-character",
  "line": 1,
  "col": 5
}
```

---

## CSS Tests (web-platform-tests)

### Repository
- **URL:** https://github.com/w3c/web-platform-tests
- **License:** W3C License
- **Status:** Official W3C test suite
- **Test Location:** `/css` subdirectory

### Test Categories

| Category | Priority | Implementation Phase |
|----------|----------|----------------------|
| CSS Syntax | P1 | Phase 4f-4g |
| CSS Selectors | P1 | Phase 4g |
| CSS Cascade | P1 | Phase 4h |
| CSS Box Model | P2 | Phase 5+ |
| CSS Flexbox | P3 | Phase 6+ |
| CSS Grid | P3 | Phase 7+ |

### Test Format
- Multiple formats: HTML reference tests, JavaScript tests, manual tests
- Use [wpt.fyi](http://wpt.fyi/) for test results tracking

---

## Integration Plan

### Phase 1: HTML5 Tokenizer Test Harness (Week 2)

**Goal:** Create test runner for html5lib JSON format

**Tasks:**
1. Download html5lib-tests repository as submodule
2. Create JSON test parser in C
3. Implement test harness that:
   - Loads JSON test files
   - Runs each test against tokenizer
   - Compares output tokens
   - Reports pass/fail with diffs
4. Integration with build system

**Deliverable:** `tests/html5lib_runner.c`

**Success Criteria:**
- Can run all 15 test files
- Generates pass/fail report
- Shows diff for failures

### Phase 2: Baseline Compliance (Week 3-4)

**Goal:** Pass basic tokenizer tests

**Test Priority:**
1. `test1.test` - Basic tags, characters
2. `test2.test` - Attributes, self-closing
3. `entities.test` - Entity basics
4. `test3.test` - Comments, DOCTYPE
5. `test4.test` - Advanced cases

**Target:** 70% pass rate on test1-test4

### Phase 3: Entity Compliance (Week 5-6)

**Goal:** Complete entity support

**Tests:**
- `namedEntities.test` - All 2,231 named entities
- `numericEntities.test` - Decimal/hex references
- `entities.test` - Edge cases

**Target:** 90% pass rate on entity tests

### Phase 4: Full Tokenizer Compliance (Week 7-10)

**Goal:** Maximum compliance

**Tests:**
- All tokenizer tests
- Unicode edge cases
- Content model flags
- Escape flags

**Target:** 85% overall pass rate (per roadmap)

### Phase 5: CSS Test Integration (Week 14-24)

**Goal:** CSS parsing and cascade compliance

**Approach:**
1. Download relevant web-platform-tests
2. Filter to CSS syntax, selectors, cascade tests
3. Create test harness for CSS tests
4. Integrate into CI

**Target:** 80% pass rate on CSS basics

---

## Test Execution Strategy

### Continuous Integration

**On every commit:**
- Run unit tests (our custom tests)
- Run html5lib test1.test (smoke test)
- Report: X/Y tests passing

**Nightly:**
- Run full html5lib suite
- Generate compliance report
- Track progress over time

**Weekly:**
- Run CSS test subset
- Update compliance dashboard

### Compliance Tracking

**Metrics to track:**
- Overall pass rate (%)
- Pass rate by test file
- Pass rate by feature area
- Regression detection
- Progress over time

**Dashboard format:**
```
SilkSurf HTML5 Compliance Report - 2025-12-30
================================================
html5lib Tokenizer Tests: 1,234/1,500 (82.3%)
  test1.test: 145/150 (96.7%) ✓
  test2.test: 134/145 (92.4%) ✓
  test3.test: 98/120 (81.7%)
  test4.test: 87/115 (75.7%)
  entities.test: 245/280 (87.5%)
  namedEntities.test: 1,987/2,231 (89.1%)
  numericEntities.test: 156/178 (87.6%)
  ... [other tests]

Top Failures:
1. Unicode normalization (45 failures)
2. Rare named entities (32 failures)
3. Content model switches (18 failures)

Trend: +2.4% since last week
```

---

## Test Harness Architecture

### Design

```c
/* HTML5lib test harness */

typedef struct {
    char *description;
    char *input;
    silk_html_token_t **expected_output;
    int expected_count;
    char *initial_state;
    char *last_start_tag;
    /* ... error tracking ... */
} html5lib_test_t;

typedef struct {
    char *filename;
    html5lib_test_t *tests;
    int test_count;
} html5lib_test_file_t;

/* Load JSON test file */
html5lib_test_file_t *html5lib_load_tests(const char *path);

/* Run single test */
int html5lib_run_test(
    html5lib_test_t *test,
    silk_html_tokenizer_t *tokenizer
);

/* Run all tests in file */
void html5lib_run_file(
    const char *path,
    int *passed,
    int *failed
);

/* Generate report */
void html5lib_generate_report(
    const char *output_path
);
```

### JSON Parser

Use a lightweight JSON parser:
- Option 1: [cJSON](https://github.com/DaveGamble/cJSON) (MIT license, single-file)
- Option 2: Custom minimal parser (we only need simple JSON)
- Option 3: [jsmn](https://github.com/zserge/jsmn) (MIT license, ~400 LOC)

**Recommendation:** Use cJSON for simplicity.

---

## Acceptance Criteria

### Week 2 (Phase 4c)
- ✅ Test harness implemented
- ✅ Can load and parse JSON test files
- ✅ Can run test1.test
- ⏳ >0% pass rate

### Week 4 (Phase 4c end)
- ⏳ >70% pass rate on test1-test4
- ⏳ Basic entities working

### Week 10 (Phase 4d end)
- ⏳ >85% overall tokenizer pass rate
- ⏳ Full entity support (>90% on entity tests)

### Week 24 (Phase 4h end)
- ⏳ >80% CSS test pass rate
- ⏳ Compliance dashboard operational

---

## Risk Mitigation

### Risk 1: Test Format Changes
- **Mitigation:** Pin html5lib-tests to specific commit
- **Fallback:** Fork repository if needed

### Risk 2: Too Many Failures
- **Mitigation:** Focus on high-value tests first (test1-test4)
- **Fallback:** Document known limitations, defer exotic cases

### Risk 3: Performance
- **Mitigation:** Run subset in CI, full suite nightly
- **Fallback:** Parallelize test execution

---

## References

### HTML5 Testing
- [html5lib-tests repository](https://github.com/html5lib/html5lib-tests)
- [WHATWG HTML Spec](https://html.spec.whatwg.org/)
- [html5lib test format documentation](https://github.com/html5lib/html5lib-tests/blob/master/tokenizer/README.md)

### CSS Testing
- [web-platform-tests repository](https://github.com/w3c/web-platform-tests)
- [W3C CSS Test Suite](https://www.w3.org/Style/CSS/Test/)
- [CSS WG Testing Wiki](https://wiki.csswg.org/test)
- [wpt.fyi - Test Results Dashboard](http://wpt.fyi/)

### Tools
- [cJSON Library](https://github.com/DaveGamble/cJSON)
- [jsmn Parser](https://github.com/zserge/jsmn)

---

**Document Version**: 1.0
**Author**: Claude (SilkSurf Development Team)
**Date**: 2025-12-30
**Status**: Ready for Implementation
