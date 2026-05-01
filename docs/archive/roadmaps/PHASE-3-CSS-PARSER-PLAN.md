# Phase 3: Native CSS Parser Implementation - Detailed Plan

**Status**: Planning (Ready for Execution)
**Estimated Duration**: 2-3 weeks
**Timeline**: After Phase 2.3 validation completes
**Dependencies**: Phase 2 (selector matching + cascade engine)

---

## Executive Summary

Phase 3 completes the native CSS pipeline by implementing a CSS parser that:
- Parses CSS stylesheets into rule structures
- Feeds parsed rules to selector matching + cascade engine
- Removes LibCSS dependency from critical path
- Achieves full control over CSS processing

**Outcome**: Complete native CSS engine (parse → match → cascade → compute)

---

## Architecture Overview

### Current State (Phase 2)
```
CSS Text → LibCSS Parser → [Opaque rules] → css_select_style() → Computed Style
                                                   ↓
                            (Our selector handler callbacks)
                                                   ↓
                            (Our cascade engine - not used yet)
```

### After Phase 3
```
CSS Text → Native Parser → Rule Array → Selector Matching → Native Cascade → Computed Style
                                              ↓                     ↓
                         (Our css_selector_match.c)   (Our css_cascade.c)
```

---

## Phase 3 Design Decisions

### 3.1: Parser Strategy

**Decision**: Build lean CSS 2.1 + Level 3 Selectors parser (~1500 lines)
- Sufficient for browser styling needs
- Modern CSS features via fallback/ignored
- Maintainable and auditable

**Why not use existing parser?**
- LibCSS: Already integrated, but opaque ruleset
- HappyCSS: Incomplete, unmaintained
- Writing our own: Clear semantics, full control

**Parser Output Format**:
```c
typedef struct {
    char *selector_text;           /* e.g., "div.class > p" */
    css_declaration *declarations; /* Array of property:value pairs */
    uint32_t decl_count;
    css_origin origin;             /* UA, Author, Author!important */
} css_rule_t;

typedef struct {
    css_rule_t *rules;
    uint32_t rule_count;
} css_stylesheet_t;
```

### 3.2: Tokenizer vs Direct Parser

**Decision**: Two-stage approach (Tokenizer → Parser)
- **Stage 1**: Tokenizer converts CSS text → tokens (IDENT, NUMBER, STRING, symbols)
- **Stage 2**: Recursive descent parser consumes tokens → rules

**Rationale**:
- Separation of concerns
- Easier to test and debug
- Tokenizer can be reused for other formats
- Aligns with browser engine design

### 3.3: Selector Complexity

**Scope for Phase 3**:
- Simple selectors: `div`, `.class`, `#id`, `[attr]`, `*` ✓
- Combinators: `div > p`, `div p`, `div + p` ✓
- Compound selectors: `div.class#id` ✓
- Pseudo-classes: `:hover`, `:focus` (parse, defer matching to Phase 4)
- **NOT in scope**: Pseudo-elements (::before), complex attribute selectors ([attr^="val"])

**Deferred to Phase 4**:
- Pseudo-element matching
- Complex selectors (`:not()`, `:has()`)
- Media queries (@media)
- Keyframes (@keyframes)

### 3.4: Property Parsing

**Scope**: Parse all CSS 2.1 properties + common CSS 3 properties

**Strategy**:
1. Generic property parser: reads property name + values until `;`
2. Property-specific validation: happens during cascade (css_cascade.c compute functions)
3. Store raw values as strings initially, compute during cascade

**Example**:
```
Input:  "color: red;"
Output: { name: "color", value: "red", important: false }
        (Validation deferred to css_cascade.c compute_color())
```

---

## Phase 3 Implementation Tasks

### Task 3.1: CSS Tokenizer (~300 lines, 2-3 days)

**File**: `src/css/css_tokenizer.c` (new)

**Tokens to Support**:
```c
typedef enum {
    CSS_TOKEN_IDENT,         /* div, color, auto */
    CSS_TOKEN_FUNCTION,      /* rgb(), url() */
    CSS_TOKEN_STRING,        /* "value" or 'value' */
    CSS_TOKEN_NUMBER,        /* 123, 45.6 */
    CSS_TOKEN_DIMENSION,     /* 10px, 2em, 50% */
    CSS_TOKEN_PERCENTAGE,    /* 50% */
    CSS_TOKEN_HASH,          /* #abc or #abcdef */
    CSS_TOKEN_DELIM,         /* +, >, ~, |, ^ */
    CSS_TOKEN_WHITESPACE,
    CSS_TOKEN_COMMENT,       /* /* ... */ */
    CSS_TOKEN_COLON,         /* : */
    CSS_TOKEN_SEMICOLON,     /* ; */
    CSS_TOKEN_LBRACE,        /* { */
    CSS_TOKEN_RBRACE,        /* } */
    CSS_TOKEN_LBRACKET,      /* [ */
    CSS_TOKEN_RBRACKET,      /* ] */
    CSS_TOKEN_LPAREN,        /* ( */
    CSS_TOKEN_RPAREN,        /* ) */
    CSS_TOKEN_COMMA,         /* , */
    CSS_TOKEN_EOF,
} css_token_type_t;
```

**Key Functions**:
- `css_tokenize(const char *input)` → `css_token_t *tokens`
- `css_token_next()` → returns next non-whitespace token
- `css_token_peek()` → returns next without consuming
- Error reporting with line:column position

**Tests**:
- Tokenize simple rule: `div { color: red; }`
- Tokenize complex selectors: `.class > p + span`
- Tokenize functions: `rgb(255, 0, 0)`
- Tokenize units: `10px`, `2em`, `50%`
- Handle comments, whitespace

---

### Task 3.2: Selector Parser (~400 lines, 3-4 days)

**File**: `src/css/css_selector_parser.c` (new)

**Functionality**:
- Parse selector string → `css_selector_t` chain (linked list)
- Support all combinator types
- Calculate specificity

**Example Parsing**:
```
Input:  "div.class#id > p:hover"
Output: [div] → [.class] → [#id] → CHILD_COMBINATOR → [p] → [:hover]
```

**Algorithm**:
```
parse_selector_list():
  while tokens remain:
    parse_compound_selector()
    if next is combinator (>, +, ~, space):
      create combinator node
      continue
    else:
      return selector chain

parse_compound_selector():
  parse simple selector (div, .class, #id, [attr])
  while next is part of compound:
    parse next simple selector
  return compound selector

calculate_specificity():
  for each selector in chain:
    if type == ID: ids++
    if type == CLASS or ATTRIBUTE: classes++
    if type == ELEMENT: elements++
  return (ids, classes, elements)
```

**Integration with existing code**:
- Use `css_selector_t` struct from `css_selector_match.h`
- Replace simple parser in `css_selector_match.c` with this full parser
- Validate against `test_css_selector_matching.c` tests

**Tests**:
- Parse type selector: `div`
- Parse class selector: `.highlight`
- Parse ID selector: `#main`
- Parse compound: `div.class#id`
- Parse combinators: `div > p`, `div p`, `div + p`
- Calculate specificity correctly
- Handle pseudo-classes: `:hover`, `:focus`

---

### Task 3.3: Declaration Parser (~250 lines, 2-3 days)

**File**: `src/css/css_declaration_parser.c` (new)

**Functionality**:
- Parse CSS declarations: `property: value1 value2; /* !important */`
- Extract property name, values, !important flag
- Create declaration structures

**Data Structures**:
```c
typedef struct {
    char *name;           /* e.g., "color" */
    char *value;          /* e.g., "red" or "rgb(255,0,0)" */
    bool important;       /* true if !important */
} css_declaration_t;

typedef struct {
    css_declaration_t *decls;
    uint32_t decl_count;
} css_declaration_block_t;
```

**Parsing Strategy**:
1. Read property name (IDENT before `:`)
2. Read values (everything before `;` or `!important`)
3. Check for `!important` flag
4. Validate property name against known CSS properties
5. Store value as-is (validation deferred to cascade)

**Example**:
```
Input:  "color: red; margin: 10px 20px; font-weight: bold !important;"
Output: [
  { name: "color", value: "red", important: false },
  { name: "margin", value: "10px 20px", important: false },
  { name: "font-weight", value: "bold", important: true }
]
```

**Tests**:
- Parse simple property: `color: red;`
- Parse multi-value property: `margin: 10px 20px 30px 40px;`
- Parse function value: `background-image: url(image.png);`
- Parse !important flag
- Handle comments
- Validate property names

---

### Task 3.4: Rule Parser (~300 lines, 3-4 days)

**File**: `src/css/css_rule_parser.c` (new)

**Functionality**:
- Parse complete CSS rules: `selector { property: value; }`
- Handle rule types: style rules, @import, @media (basic)
- Create rule structures

**Algorithm**:
```
parse_stylesheet():
  while tokens remain:
    if next is @:
      parse at-rule (@import, @media, etc.)
    else:
      parse style rule

parse_style_rule():
  selectors = parse_selector_list()
  expect '{'
  declarations = parse_declaration_block()
  expect '}'
  return rule

parse_at_rule():
  if @import:
    parse_import_rule() → add to imports
  if @media:
    parse_media_rule() → parse rules inside media block
  if @font-face:
    parse_font_face_rule()
  else:
    skip_unknown_at_rule()
```

**Data Structures**:
```c
typedef enum {
    CSS_RULE_STYLE,      /* Normal style rule */
    CSS_RULE_MEDIA,      /* @media block */
    CSS_RULE_IMPORT,     /* @import url */
    CSS_RULE_FONT_FACE,  /* @font-face */
    CSS_RULE_UNKNOWN,    /* Unknown @-rule */
} css_rule_type_t;

typedef struct {
    css_rule_type_t type;

    union {
        struct {
            char **selectors;              /* Array of selector strings */
            uint32_t selector_count;
            css_declaration_t *declarations;
            uint32_t decl_count;
        } style;

        struct {
            char *media_query;             /* e.g., "screen and (max-width: 800px)" */
            css_rule_t *rules;             /* Rules within @media */
            uint32_t rule_count;
        } media;

        struct {
            char *url;                     /* URL to import */
        } import;
    } data;
} css_rule_t;
```

**Tests**:
- Parse simple rule: `div { color: red; }`
- Parse multiple selectors: `h1, h2, h3 { margin: 0; }`
- Parse multiple declarations
- Parse @media queries (basic)
- Parse @import rules
- Handle nested rules in @media blocks

---

### Task 3.5: Full Parser Integration (~200 lines, 2-3 days)

**File**: `src/css/css_parser.c` (new, replaces libcss parsing)

**Functionality**:
- Public API: `css_stylesheet_t *css_parse_stylesheet(const char *text)`
- Integrate tokenizer → selector → declaration → rule parsing
- Error handling and recovery
- Line/column tracking for diagnostics

**Public API**:
```c
typedef struct {
    css_rule_t *rules;
    uint32_t rule_count;
    char **errors;           /* Parsing errors (continue on errors) */
    uint32_t error_count;
} css_stylesheet_t;

/* Main API */
css_stylesheet_t *css_parse_stylesheet(const char *css_text);
void css_stylesheet_free(css_stylesheet_t *sheet);

/* Utilities */
void css_stylesheet_debug_print(const css_stylesheet_t *sheet);
uint32_t css_stylesheet_rule_count(const css_stylesheet_t *sheet);
```

**Error Handling**:
- Graceful degradation: skip invalid rules, continue parsing
- Collect errors but don't fail stylesheet parsing
- Print warnings to debug output
- Never crash on malformed CSS

**Integration Points**:
- Replace libcss stylesheet creation in `css_engine.c`
- Use parsed rules with selector matching + cascade
- Feed matching rules to `css_cascade_for_element()`

**Tests**:
- Parse complete HTML page stylesheet
- Parse multiple rules with various properties
- Recover from invalid rules (skip and continue)
- Debug output verification

---

### Task 3.6: Selector Matching Integration (~300 lines, 3-4 days)

**File**: Modify `src/document/css_engine.c` (Phase 2.2 integration)

**Functionality**:
- Load parsed stylesheet rules
- For each DOM element:
  1. Iterate stylesheet rules
  2. Match selector against element using `css_selector_match.c`
  3. Collect matched rules with specificity
  4. Feed to native cascade engine

**New Algorithm** (replacing libcss):
```c
void compute_element_styles_native(
    silk_css_engine *engine,
    silk_dom_node_t *element,
    silk_computed_style_t *out_style
) {
    /* Iterate stylesheets */
    for (int s = 0; s < engine->sheet_count; s++) {
        css_stylesheet_t *sheet = engine->sheets[s];

        /* Iterate rules */
        for (uint32_t r = 0; r < sheet->rule_count; r++) {
            css_rule_t *rule = &sheet->rules[r];
            if (rule->type != CSS_RULE_STYLE) continue;

            /* Parse selectors */
            for (uint32_t sel = 0; sel < rule->selector_count; sel++) {
                css_rule_selector_t *parsed = css_selector_parse(
                    rule->selectors[sel]);

                /* Match selector */
                if (css_selector_matches(parsed, element, NULL)) {
                    /* Add matched rule to cascade context */
                    css_cascade_add_rule(cascade_ctx, rule, parsed->specificity);
                }
                css_selector_free(parsed);
            }
        }
    }

    /* Run native cascade */
    css_cascade_for_element(cascade_ctx, out_style);
}
```

**Challenge**: Performance optimization
- Don't re-parse selectors on every element
- Cache parsed selectors in stylesheet
- Consider selector indexing (Phase 3.7)

**Tests**:
- Simple rule matching and cascade
- Multiple matching rules (test specificity)
- Cascade with different origins (UA, Author, Author!important)

---

### Task 3.7: Selector Indexing & Optimization (~200 lines, 2-3 days)

**File**: `src/css/css_selector_index.c` (new)

**Problem**: Iterating all rules for every element is O(rules × elements)

**Solution**: Selector indexing
- Index by tag name: `div { ... }` → indexed under "div"
- Index by class: `.header { ... }` → indexed under ".header"
- Index by ID: `#main { ... }` → indexed under "#main"
- Fallback: Universal rules `*` or complex selectors

**Lookup Algorithm**:
```c
css_rule_t **find_matching_rules(
    css_stylesheet_t *sheet,
    dom_element *element,
    uint32_t *out_count
) {
    /* Get element characteristics */
    const char *tag = dom_element_tag_name(element);
    const char *id = dom_element_id(element);
    const char **classes = dom_element_classes(element);

    /* Lookup: ID rules */
    css_rule_t **rules = css_index_lookup_id(sheet, id);

    /* Lookup: class rules (each class) */
    for (int i = 0; classes[i]; i++) {
        css_rule_t **class_rules = css_index_lookup_class(sheet, classes[i]);
        append_rules(rules, class_rules);
    }

    /* Lookup: tag rules */
    css_rule_t **tag_rules = css_index_lookup_tag(sheet, tag);
    append_rules(rules, tag_rules);

    /* Lookup: universal rules */
    css_rule_t **univ_rules = css_index_lookup_universal(sheet);
    append_rules(rules, univ_rules);

    return rules;
}
```

**Data Structure**:
```c
typedef struct {
    /* Hash tables for fast lookup */
    hash_table_t *by_id;        /* ID → rules */
    hash_table_t *by_class;     /* Class → rules */
    hash_table_t *by_tag;       /* Tag name → rules */
    css_rule_t *universal;      /* Rules with * or complex selectors */
    uint32_t universal_count;
} css_selector_index_t;
```

**Performance Gain**: ~100x faster for large stylesheets (1000+ rules)

**Tests**:
- Index creation and correctness
- Lookup by ID, class, tag
- Fallback to universal rules
- Performance benchmark: 1000 rules, 100 elements

---

### Task 3.8: Error Handling & Recovery (~150 lines, 2 days)

**File**: `src/css/css_error.c` (new)

**Functionality**:
- Robust error reporting with line:column
- Graceful recovery (skip invalid rules)
- Debug diagnostics
- Performance under malformed CSS

**Error Types**:
```c
typedef enum {
    CSS_ERR_NONE,
    CSS_ERR_UNEXPECTED_TOKEN,
    CSS_ERR_MISSING_BRACE,
    CSS_ERR_INVALID_PROPERTY,
    CSS_ERR_INVALID_SELECTOR,
    CSS_ERR_UNTERMINATED_STRING,
    CSS_ERR_UNEXPECTED_EOF,
} css_error_type_t;
```

**Example Recovery**:
```
Input:  "div { color: red; } xxx invalid rule { } p { font-size: 20px; }"

Parse: [✓ div rule] [✗ skip "xxx invalid"] [✓ p rule]

Output: 2 rules successfully parsed, 1 error reported
```

**Tests**:
- Malformed selectors (recovery)
- Invalid declarations (recovery)
- Missing braces (recovery)
- Unterminated strings (recovery)
- Mixed valid/invalid rules

---

### Task 3.9: Unit Testing Suite (~500 lines, 3-4 days)

**File**: `tests/test_css_parser.c` (new, comprehensive)

**Test Coverage**:
1. Tokenizer tests (50+ cases)
2. Selector parser tests (50+ cases)
3. Declaration parser tests (40+ cases)
4. Rule parser tests (40+ cases)
5. Integration tests (30+ cases)
6. Error recovery tests (30+ cases)

**Key Test Scenarios**:
- Parse real-world stylesheets
- Performance under 10KB+ CSS
- Error recovery for malformed CSS
- Compatibility with CSS 2.1 + common CSS 3

**Expected Results**:
- 180+ unit tests, 100% pass
- <1ms parse time for 10KB CSS
- <2MB memory for typical page CSS

---

### Task 3.10: Integration Testing (~300 lines, 3-4 days)

**Files**:
- `tests/test_css_parser_integration.c` (new)
- Update `tests/test_e2e_rendering.c`

**Integration Scenarios**:
1. Parse stylesheet → Match selectors → Cascade → Compute styles
2. Real HTML + CSS (from test.html)
3. Performance: full page CSS pipeline
4. Memory usage: track allocations

**Test Case**:
```html
<html>
<head>
<style>
  body { margin: 0; background: white; }
  h1 { color: blue; font-size: 24px; }
  .highlight { background: yellow; }
  #main { width: 80%; }
</style>
</head>
<body>
  <h1>Title</h1>
  <p class="highlight" id="main">Content</p>
</body>
</html>
```

**Expected**:
- h1: color=blue, font-size=24px
- p: background=yellow, width=80%
- All styles computed in <1ms

---

## Phase 3 Timeline

### Week 1: Tokenizer + Selector Parser
- Task 3.1: CSS Tokenizer (2-3 days)
- Task 3.2: Selector Parser (3-4 days)
- Subtotal: ~1 week

### Week 2: Declaration + Rule Parser
- Task 3.3: Declaration Parser (2-3 days)
- Task 3.4: Rule Parser (3-4 days)
- Subtotal: ~1 week

### Week 3: Integration + Optimization
- Task 3.5: Full Parser Integration (2-3 days)
- Task 3.6: Selector Matching Integration (3-4 days)
- Task 3.7: Selector Indexing (2-3 days)
- Task 3.8: Error Handling (2 days)
- Task 3.9-3.10: Testing & Integration (3-4 days)
- Subtotal: ~1 week

**Total Duration**: 2-3 weeks
**Parallel Work**: Testing can start during Week 2

---

## Success Criteria

### Functional
- [ ] Parse 100% of CSS 2.1 properties (allow unknown properties)
- [ ] Support selectors: type, class, ID, attribute, combinators, pseudo-classes
- [ ] Graceful error recovery (malformed CSS continues parsing)
- [ ] Selector indexing working (100x performance vs. linear scan)
- [ ] Full native pipeline: Parse → Match → Cascade → Compute

### Performance
- [ ] Parse 10KB CSS in <1ms
- [ ] Match selectors for 1000 elements in <5ms
- [ ] Memory: <2MB for typical page CSS
- [ ] 60 FPS maintained during interactive CSS changes

### Quality
- [ ] 180+ unit tests, 100% pass
- [ ] 0 compiler warnings
- [ ] 0 memory leaks (Valgrind clean)
- [ ] Code review: alignment with project standards

### Documentation
- [ ] `docs/CSS-PARSER-ARCHITECTURE.md` (500+ lines)
- [ ] `docs/CSS-PARSING-EXAMPLES.md` (algorithm walkthroughs)
- [ ] Inline code comments explaining complex logic

---

## Risk Mitigation

### Risk 1: Parser Complexity
- **Mitigation**: Start with CSS 2.1 only, defer advanced features
- **Fallback**: Keep libcss as backup until parser fully tested

### Risk 2: Performance Not Meeting Goals
- **Mitigation**: Profile early, optimize hotspots, implement indexing
- **Fallback**: Accept linear selector matching if necessary

### Risk 3: Compatibility Issues
- **Mitigation**: Test against real-world stylesheets, add recovery
- **Fallback**: Unknown properties skipped gracefully

### Risk 4: Integration Complexity
- **Mitigation**: Modular design, test each component independently
- **Fallback**: Hybrid approach (parser + libcss cascade) if needed

---

## Architectural Benefits After Phase 3

1. **Full Control**: Every CSS processing step under our control
2. **Performance**: Custom optimizations (indexing, caching)
3. **Debuggability**: Can trace entire CSS pipeline
4. **Maintainability**: No dependency on libcss internals
5. **Flexibility**: Easy to add custom features (variables, extensions)
6. **Security**: No external parsing overhead

---

## Dependencies & Prerequisites

### Before Phase 3 Starts
- [ ] Phase 2.3 validation complete (selector matching + cascade tested)
- [ ] No breaking changes to css_cascade.c or css_selector_match.c
- [ ] Agreement on CSS 2.1 + Selectors Level 3 scope

### External Dependencies
- **None**: No new libraries required
- C standard library only (stdlib, string, ctype)

---

## Post-Phase 3: Phase 4 Roadmap

After Phase 3 complete, Phase 4 builds on native CSS engine:

### Phase 4.1: Advanced Selectors
- Pseudo-element matching (::before, ::after)
- Complex selectors (:not(), :has())
- :nth-child() family

### Phase 4.2: Advanced CSS
- CSS variables (--custom-property)
- Calc expressions
- CSS Grid support

### Phase 4.3: Performance
- Incremental CSS computation
- Style change invalidation
- CSS animation optimization

### Phase 4.4: Compatibility
- CSS transforms
- Media queries (full support)
- Viewport units (vw, vh)

---

## Conclusion

Phase 3 delivers a complete, native CSS parsing and styling engine that:
- Removes LibCSS dependency from critical path
- Provides full control and debuggability
- Maintains high performance through indexing
- Enables future CSS feature extensions
- Aligns with project's cleanroom architecture vision

**Key Insight**: By deferring advanced features to Phase 4, Phase 3 stays focused and achievable in 2-3 weeks.
