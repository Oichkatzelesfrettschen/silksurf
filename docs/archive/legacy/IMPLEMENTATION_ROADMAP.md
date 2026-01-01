# SilkSurf Implementation Roadmap

> Legacy roadmap (C baseline). The Rust cleanroom implementation supersedes
> this plan. Current Rust milestones are tracked in `docs/ENGINE_PERF_ROADMAP.md`,
> `docs/JS_ENGINE_PERF_ROADMAP.md`, and `docs/ENGINE_HOTPATHS.md`.
## Self-Contained HTML5/CSS/DOM Engine
### Date: 2025-12-30
### Scope: Maximum Standards Compliance with Comprehensive Testing

---

## Revised Scope Based on Requirements

### User Requirements (from 2025-12-30 scoping session):
1. ✅ **Maximum compliance** - implement as much of HTML5/CSS specs as possible
2. ✅ **Hybrid approach** - core components sequentially, features vertically
3. ✅ **Comprehensive testing** - unit + integration + compliance tests
4. ✅ **Lenient/recovery mode** - match browser error recovery behavior

### Impact on Original Architecture Estimate:

| Component | Original Est. | Revised Est. | Reason |
|-----------|---------------|--------------|---------|
| HTML Tokenizer | 3,000 | 10,000-15,000 | Need 60-70 states, not 12. Full error recovery. |
| HTML Parser | 8,000 | 20,000-30,000 | Need 18-21 modes, not 8-10. Adoption agency, quirks mode. |
| DOM | 6,000 | 8,000-10,000 | More complete W3C DOM API for compliance. |
| CSS Tokenizer | 2,000 | 3,000-4,000 | Full CSS syntax, all token types. |
| CSS Parser | 4,000 | 8,000-10,000 | All selector types, all properties, media queries. |
| CSS Cascade | 3,000 | 5,000-6,000 | Full cascade algorithm, all edge cases. |
| Test Suite | 5,000 | 15,000-20,000 | Unit + integration + compliance tests. |
| **TOTAL** | **~25,000** | **~70,000-95,000** | **3-4x increase for full compliance** |

**Still 75-80% smaller than Ladybird's 400,000 LOC!**

---

## Revised Timeline

### Overview: 20-24 Weeks (5-6 Months)

Instead of 14 weeks for simplified implementation, full compliance needs:
- **Phase 4c (Tokenizer)**: 3-4 weeks → Full HTML5 tokenizer with all states
- **Phase 4d (Parser)**: 5-6 weeks → Full HTML5 parser with all insertion modes
- **Phase 4e (DOM)**: 2-3 weeks → Complete W3C DOM implementation
- **Phase 4f (CSS Tokenizer)**: 2 weeks → Full CSS syntax tokenizer
- **Phase 4g (CSS Parser)**: 4-5 weeks → Complete CSS parser with all selectors/properties
- **Phase 4h (CSS Cascade)**: 3-4 weeks → Full cascade algorithm
- **Phase 4i (Integration)**: 2-3 weeks → Full system integration and testing

---

## Phase 4c: HTML5 Tokenizer (Weeks 1-4)

### Scope: ~10,000-15,000 LOC
### Goal: Full HTML5 tokenization state machine with error recovery

### 4c.1: Tokenizer Foundation (Week 1)
**LOC Target**: ~2,000

- [ ] Create `src/document/html_tokenizer.h` with state enum
- [ ] Create `src/document/html_tokenizer.c` skeleton
- [ ] Implement UTF-8 input stream reader
- [ ] Add character reference decoder stub
- [ ] Write unit tests for input stream
- [ ] Test: Verify input stream handles UTF-8 correctly

**Files**:
```c
// html_tokenizer.h
typedef enum {
    HTML_TOK_DATA,
    HTML_TOK_RCDATA,
    HTML_TOK_RAWTEXT,
    HTML_TOK_SCRIPT_DATA,
    HTML_TOK_PLAINTEXT,
    // ... 60+ more states ...
} silk_html_tokenizer_state_t;

typedef struct {
    silk_arena_t *arena;
    const char *input;
    size_t input_len;
    size_t pos;
    silk_html_tokenizer_state_t state;
    silk_html_token_t *current_token;
} silk_html_tokenizer_t;
```

### 4c.2: Core Tokenizer States (Week 2)
**LOC Target**: ~4,000

- [ ] Implement Data state
- [ ] Implement Tag open state
- [ ] Implement Tag name state
- [ ] Implement End tag open state
- [ ] Implement Before/after attribute name states
- [ ] Implement Attribute value states (quoted/unquoted)
- [ ] Implement Self-closing start tag state
- [ ] Write unit tests for each state
- [ ] Test: Tokenize simple tags `<div>`, `<p class="foo">`, `<br/>`

### 4c.3: Special Content States (Week 2-3)
**LOC Target**: ~3,000

- [ ] Implement RCDATA states (for `<textarea>`, `<title>`)
- [ ] Implement RAWTEXT states (for `<style>`, `<script>`)
- [ ] Implement Script data states (with escape sequences)
- [ ] Implement Comment states
- [ ] Implement DOCTYPE states
- [ ] Implement CDATA section states
- [ ] Write unit tests for special content
- [ ] Test: Tokenize `<script>`, `<style>`, `<!-- comments -->`

### 4c.4: Character References (Week 3)
**LOC Target**: ~2,000

- [ ] Create `src/document/html_entities.h` with entity table
- [ ] Implement named character reference matching
- [ ] Implement numeric character references (decimal/hex)
- [ ] Implement ambiguous ampersand handling
- [ ] Write comprehensive entity tests
- [ ] Test: Decode `&lt;`, `&amp;`, `&#123;`, `&#x41;`

**Entity Table**:
```c
// html_entities.h
typedef struct {
    const char *name;
    uint32_t codepoint;
} silk_html_entity_t;

// Common entities (can expand to full HTML5 set)
static const silk_html_entity_t entities[] = {
    {"lt", 0x003C},
    {"gt", 0x003E},
    {"amp", 0x0026},
    {"quot", 0x0022},
    {"nbsp", 0x00A0},
    // ... ~2,000 more entities ...
};
```

### 4c.5: Error Recovery & Edge Cases (Week 4)
**LOC Target**: ~2,000

- [ ] Implement error token emission
- [ ] Add error recovery for malformed tags
- [ ] Handle EOF in various states
- [ ] Implement state reconsumption logic
- [ ] Add comprehensive error recovery tests
- [ ] Test: Malformed HTML `<div<p>`, `<div attr`, `<div attr="unclosed`

### 4c.6: Tokenizer Integration Tests (Week 4)
**LOC Target**: ~1,000 (test code)

- [ ] Create `tests/test_html_tokenizer.c`
- [ ] Add tests for complete HTML documents
- [ ] Add tests for real-world malformed HTML
- [ ] Run HTML5 tokenizer compliance tests (subset)
- [ ] Benchmark tokenizer performance
- [ ] Test: Full test_document_simple.html tokenization

**Success Criteria for Phase 4c**:
- ✅ All 60-70 tokenizer states implemented
- ✅ Character reference decoding works for common entities
- ✅ Error recovery matches HTML5 spec for common cases
- ✅ 95%+ unit test coverage
- ✅ Can tokenize real-world HTML without crashes
- ✅ Performance: < 5ms for 10KB document

---

## Phase 4d: HTML5 Parser (Weeks 5-10)

### Scope: ~20,000-30,000 LOC
### Goal: Full HTML5 tree construction with all insertion modes

### 4d.1: Parser Foundation (Week 5)
**LOC Target**: ~3,000

- [ ] Create `src/document/html_parser.h` with insertion mode enum
- [ ] Create `src/document/html_parser.c` skeleton
- [ ] Implement stack of open elements
- [ ] Implement list of active formatting elements
- [ ] Create initial insertion mode
- [ ] Write unit tests for data structures
- [ ] Test: Stack push/pop, formatting list operations

**Core Data Structures**:
```c
// html_parser.h
typedef enum {
    HTML_IM_INITIAL,
    HTML_IM_BEFORE_HTML,
    HTML_IM_BEFORE_HEAD,
    HTML_IM_IN_HEAD,
    HTML_IM_IN_HEAD_NOSCRIPT,
    HTML_IM_AFTER_HEAD,
    HTML_IM_IN_BODY,
    HTML_IM_TEXT,
    HTML_IM_IN_TABLE,
    HTML_IM_IN_TABLE_TEXT,
    HTML_IM_IN_CAPTION,
    HTML_IM_IN_COLUMN_GROUP,
    HTML_IM_IN_TABLE_BODY,
    HTML_IM_IN_ROW,
    HTML_IM_IN_CELL,
    HTML_IM_IN_TEMPLATE,
    HTML_IM_AFTER_BODY,
    HTML_IM_IN_FRAMESET,
    HTML_IM_AFTER_FRAMESET,
    HTML_IM_AFTER_AFTER_BODY,
    HTML_IM_AFTER_AFTER_FRAMESET
} silk_html_insertion_mode_t;

typedef struct {
    silk_arena_t *arena;
    silk_html_tokenizer_t *tokenizer;
    silk_dom_document_t *document;

    silk_html_insertion_mode_t mode;
    silk_html_insertion_mode_t original_mode;

    /* Stack of open elements */
    silk_dom_element_t **element_stack;
    int element_stack_size;
    int element_stack_capacity;

    /* List of active formatting elements */
    silk_dom_element_t **formatting_list;
    int formatting_list_size;

    /* Template insertion mode stack */
    silk_html_insertion_mode_t *template_modes;
    int template_mode_count;

    silk_dom_element_t *head_element;
    silk_dom_element_t *form_element;

    bool foster_parenting;
    bool frameset_ok;
    bool scripting_enabled;
} silk_html_parser_t;
```

### 4d.2: Basic Insertion Modes (Week 5-6)
**LOC Target**: ~5,000

- [ ] Implement "initial" mode
- [ ] Implement "before html" mode
- [ ] Implement "before head" mode
- [ ] Implement "in head" mode
- [ ] Implement "after head" mode
- [ ] Implement "in body" mode (basic)
- [ ] Write unit tests for each mode
- [ ] Test: Parse `<!DOCTYPE html><html><head></head><body></body></html>`

### 4d.3: Body Insertion Mode (Week 6-7)
**LOC Target**: ~6,000

- [ ] Complete "in body" mode (all cases)
- [ ] Implement element creation for all HTML elements
- [ ] Handle text node insertion
- [ ] Implement comment insertion
- [ ] Handle whitespace collapsing
- [ ] Write comprehensive "in body" tests
- [ ] Test: Parse complex nested HTML with divs, spans, paragraphs

### 4d.4: Table Insertion Modes (Week 7-8)
**LOC Target**: ~4,000

- [ ] Implement "in table" mode
- [ ] Implement "in table text" mode
- [ ] Implement "in caption" mode
- [ ] Implement "in column group" mode
- [ ] Implement "in table body" mode
- [ ] Implement "in row" mode
- [ ] Implement "in cell" mode
- [ ] Implement foster parenting algorithm
- [ ] Write table parsing tests
- [ ] Test: Parse complex tables with nested content

### 4d.5: Advanced Modes & Algorithms (Week 8-9)
**LOC Target**: ~5,000

- [ ] Implement "in template" mode
- [ ] Implement template mode stack
- [ ] Implement adoption agency algorithm
- [ ] Implement "reconstruct active formatting elements"
- [ ] Implement "generate implied end tags"
- [ ] Implement "reset insertion mode appropriately"
- [ ] Write tests for misnested tags
- [ ] Test: Parse `<b><i></b></i>` (adoption agency)

**Adoption Agency Algorithm** (most complex part of HTML5 parsing):
```c
// Handles misnested formatting elements like <b><i></b></i>
void run_adoption_agency_algorithm(silk_html_parser_t *parser,
                                     const char *tag_name) {
    // Step 1: Find formatting element in active list
    // Step 2: Find furthest block
    // Step 3: Create bookmark
    // Step 4-8: Complex DOM tree manipulation
    // ... (implements full HTML5 spec algorithm)
}
```

### 4d.6: Error Recovery & Quirks (Week 9)
**LOC Target**: ~3,000

- [ ] Implement parser error reporting
- [ ] Add error recovery for malformed nesting
- [ ] Implement quirks mode detection
- [ ] Add limited quirks mode handling
- [ ] Handle unexpected end tags
- [ ] Write error recovery tests
- [ ] Test: Parse real-world broken HTML

### 4d.7: Parser Integration Tests (Week 10)
**LOC Target**: ~2,000 (test code)

- [ ] Create `tests/test_html_parser.c`
- [ ] Add tests for complete HTML documents
- [ ] Run HTML5 parser compliance tests (subset)
- [ ] Test with real-world HTML samples
- [ ] Benchmark parser performance
- [ ] Test: Parse Wikipedia homepage HTML

**Success Criteria for Phase 4d**:
- ✅ All 21 insertion modes implemented
- ✅ Adoption agency algorithm works correctly
- ✅ Can parse complex real-world HTML
- ✅ 90%+ unit test coverage
- ✅ Passes core HTML5 parser compliance tests
- ✅ Performance: < 20ms for 50KB document

---

## Phase 4e: Self-Contained DOM (Weeks 11-13)

### Scope: ~8,000-10,000 LOC
### Goal: Complete W3C DOM Level 2 Core implementation

### 4e.1: DOM Node Types (Week 11)
**LOC Target**: ~2,500

- [ ] Create `src/document/dom_node.h` with node types
- [ ] Implement base Node structure
- [ ] Implement Element node
- [ ] Implement Text node
- [ ] Implement Comment node
- [ ] Implement Document node
- [ ] Implement DocumentType node
- [ ] Write node creation tests
- [ ] Test: Create all node types

**DOM Node Structure**:
```c
// dom_node.h
typedef enum {
    SILK_DOM_ELEMENT_NODE = 1,
    SILK_DOM_TEXT_NODE = 3,
    SILK_DOM_COMMENT_NODE = 8,
    SILK_DOM_DOCUMENT_NODE = 9,
    SILK_DOM_DOCUMENT_TYPE_NODE = 10
} silk_dom_node_type_t;

typedef struct silk_dom_node {
    silk_dom_node_type_t type;
    char *node_name;
    char *node_value;

    struct silk_dom_node *parent;
    struct silk_dom_node *first_child;
    struct silk_dom_node *last_child;
    struct silk_dom_node *previous_sibling;
    struct silk_dom_node *next_sibling;

    silk_dom_document_t *owner_document;
    silk_arena_t *arena;

    /* For Element nodes */
    silk_dom_attribute_t *attributes;
    int attribute_count;
} silk_dom_node_t;
```

### 4e.2: Tree Operations (Week 11-12)
**LOC Target**: ~2,000

- [ ] Implement appendChild
- [ ] Implement insertBefore
- [ ] Implement removeChild
- [ ] Implement replaceChild
- [ ] Implement cloneNode
- [ ] Implement tree traversal helpers
- [ ] Write tree operation tests
- [ ] Test: Build and manipulate complex DOM trees

### 4e.3: Element & Attributes (Week 12)
**LOC Target**: ~2,000

- [ ] Implement getAttribute
- [ ] Implement setAttribute
- [ ] Implement removeAttribute
- [ ] Implement hasAttribute
- [ ] Implement getElementsByTagName
- [ ] Implement getElementById
- [ ] Implement getElementsByClassName
- [ ] Write attribute tests
- [ ] Test: Element lookup and attribute manipulation

### 4e.4: Document Methods (Week 12-13)
**LOC Target**: ~1,500

- [ ] Implement createElement
- [ ] Implement createTextNode
- [ ] Implement createComment
- [ ] Implement querySelector (basic)
- [ ] Implement querySelectorAll (basic)
- [ ] Write document method tests
- [ ] Test: Document manipulation operations

### 4e.5: DOM Integration & Tests (Week 13)
**LOC Target**: ~2,000 (test code)

- [ ] Create `tests/test_dom.c`
- [ ] Add DOM manipulation tests
- [ ] Add W3C DOM compliance tests (subset)
- [ ] Test DOM with parser integration
- [ ] Benchmark DOM operations
- [ ] Test: Full HTML parse → DOM tree → query

**Success Criteria for Phase 4e**:
- ✅ All core node types implemented
- ✅ Tree operations work correctly
- ✅ Element/attribute methods functional
- ✅ 95%+ unit test coverage
- ✅ Passes core W3C DOM compliance tests
- ✅ Performance: < 1ms for 1000 node tree manipulation

---

## Phase 4f: CSS Tokenizer (Weeks 14-15)

### Scope: ~3,000-4,000 LOC
### Goal: Full CSS syntax tokenization

### 4f.1: CSS Token Types (Week 14)
**LOC Target**: ~1,000

- [ ] Create `src/document/css_tokenizer.h`
- [ ] Define CSS token types (ident, function, string, number, etc.)
- [ ] Implement input stream
- [ ] Create token structure
- [ ] Write basic tokenizer tests
- [ ] Test: Tokenize simple CSS strings

**CSS Token Types**:
```c
typedef enum {
    CSS_TOK_IDENT,
    CSS_TOK_FUNCTION,
    CSS_TOK_AT_KEYWORD,
    CSS_TOK_HASH,
    CSS_TOK_STRING,
    CSS_TOK_NUMBER,
    CSS_TOK_PERCENTAGE,
    CSS_TOK_DIMENSION,
    CSS_TOK_URL,
    CSS_TOK_UNICODE_RANGE,
    CSS_TOK_WHITESPACE,
    CSS_TOK_CDO,         /* <!-- */
    CSS_TOK_CDC,         /* --> */
    CSS_TOK_COLON,
    CSS_TOK_SEMICOLON,
    CSS_TOK_COMMA,
    CSS_TOK_DELIM,
    CSS_TOK_EOF
} silk_css_token_type_t;
```

### 4f.2: CSS Tokenizer Implementation (Week 14-15)
**LOC Target**: ~2,000

- [ ] Implement identifier tokenization
- [ ] Implement number tokenization
- [ ] Implement string tokenization (single/double quotes)
- [ ] Implement URL tokenization
- [ ] Implement hash tokenization
- [ ] Implement whitespace handling
- [ ] Implement comment handling
- [ ] Write comprehensive tokenizer tests
- [ ] Test: Tokenize complex CSS

### 4f.3: CSS Tokenizer Tests (Week 15)
**LOC Target**: ~1,000 (test code)

- [ ] Create `tests/test_css_tokenizer.c`
- [ ] Add unit tests for all token types
- [ ] Test CSS 3 syntax features
- [ ] Test error recovery
- [ ] Run CSS tokenizer compliance tests
- [ ] Test: Tokenize Bootstrap CSS

**Success Criteria for Phase 4f**:
- ✅ All CSS token types implemented
- ✅ Handles CSS 3 syntax correctly
- ✅ Error recovery works
- ✅ 95%+ test coverage
- ✅ Performance: < 5ms for 100KB CSS file

---

## Phase 4g: CSS Parser (Weeks 16-20)

### Scope: ~8,000-10,000 LOC
### Goal: Complete CSS parser with all selector/property types

### 4g.1: CSS Parser Foundation (Week 16)
**LOC Target**: ~2,000

- [ ] Create `src/document/css_parser.h`
- [ ] Implement stylesheet structure
- [ ] Implement rule list structure
- [ ] Create parser state machine
- [ ] Write basic parser tests
- [ ] Test: Parse empty stylesheet

**CSS Structures**:
```c
typedef struct {
    silk_css_selector_t *selectors;
    int selector_count;
    silk_css_declaration_t *declarations;
    int declaration_count;
} silk_css_style_rule_t;

typedef struct {
    silk_css_style_rule_t *rules;
    int rule_count;
    silk_arena_t *arena;
} silk_css_stylesheet_t;
```

### 4g.2: Selector Parsing (Week 16-17)
**LOC Target**: ~3,000

- [ ] Implement type selector parsing
- [ ] Implement class selector parsing
- [ ] Implement ID selector parsing
- [ ] Implement attribute selector parsing
- [ ] Implement pseudo-class parsing
- [ ] Implement combinator parsing (descendant, child, adjacent, etc.)
- [ ] Implement selector list parsing
- [ ] Write selector parsing tests
- [ ] Test: Parse complex selectors `div.class#id[attr]:hover > span`

### 4g.3: Property Value Parsing (Week 17-19)
**LOC Target**: ~4,000

- [ ] Implement color value parsing (named, hex, rgb, rgba, hsl)
- [ ] Implement length value parsing (px, em, rem, %, etc.)
- [ ] Implement keyword value parsing
- [ ] Implement shorthand property expansion
- [ ] Implement calc() parsing
- [ ] Implement CSS variable parsing (--custom-prop)
- [ ] Implement font-family parsing
- [ ] Write property value tests
- [ ] Test: Parse all CSS property types

### 4g.4: At-Rules & Media Queries (Week 19)
**LOC Target**: ~2,000

- [ ] Implement @media rule parsing
- [ ] Implement @import rule parsing
- [ ] Implement @font-face rule parsing
- [ ] Implement media query parsing
- [ ] Write at-rule tests
- [ ] Test: Parse responsive CSS with media queries

### 4g.5: CSS Parser Integration Tests (Week 20)
**LOC Target**: ~1,000 (test code)

- [ ] Create `tests/test_css_parser.c`
- [ ] Add tests for complete stylesheets
- [ ] Run CSS parser compliance tests
- [ ] Test with real-world CSS (Bootstrap, Tailwind)
- [ ] Benchmark parser performance
- [ ] Test: Parse large CSS frameworks

**Success Criteria for Phase 4g**:
- ✅ All selector types implemented
- ✅ All common CSS properties parsed
- ✅ Media queries work
- ✅ 90%+ test coverage
- ✅ Passes core CSS parser compliance tests
- ✅ Performance: < 20ms for 500KB CSS file

---

## Phase 4h: CSS Cascade (Weeks 21-24)

### Scope: ~5,000-6,000 LOC
### Goal: Full CSS cascade algorithm with specificity and inheritance

### 4h.1: Specificity Calculation (Week 21)
**LOC Target**: ~1,500

- [ ] Create `src/document/css_cascade.h`
- [ ] Implement specificity calculation for all selector types
- [ ] Implement specificity comparison
- [ ] Handle !important declarations
- [ ] Write specificity tests
- [ ] Test: Verify specificity order for complex selectors

**Specificity Algorithm**:
```c
typedef struct {
    int a;  /* inline styles */
    int b;  /* IDs */
    int c;  /* classes, attributes, pseudo-classes */
    int d;  /* elements, pseudo-elements */
} silk_css_specificity_t;

int compare_specificity(silk_css_specificity_t *s1,
                         silk_css_specificity_t *s2) {
    if (s1->a != s2->a) return s1->a - s2->a;
    if (s1->b != s2->b) return s1->b - s2->b;
    if (s1->c != s2->c) return s1->c - s2->c;
    return s1->d - s2->d;
}
```

### 4h.2: Cascade Algorithm (Week 21-22)
**LOC Target**: ~2,000

- [ ] Implement origin cascade (UA → Author → Inline)
- [ ] Implement specificity cascade
- [ ] Implement source order cascade
- [ ] Handle !important cascade inversion
- [ ] Implement cascade layer support
- [ ] Write cascade tests
- [ ] Test: Verify cascade order for conflicting rules

### 4h.3: Inheritance (Week 22-23)
**LOC Target**: ~1,500

- [ ] Implement inherited property list
- [ ] Implement inheritance algorithm
- [ ] Handle inherit/initial/unset keywords
- [ ] Implement CSS variable inheritance
- [ ] Write inheritance tests
- [ ] Test: Verify inheritance for all properties

### 4h.4: Computed Values (Week 23)
**LOC Target**: ~1,500

- [ ] Implement computed value resolution
- [ ] Handle relative units (em, %, etc.)
- [ ] Implement color computation
- [ ] Implement calc() evaluation
- [ ] Write computed value tests
- [ ] Test: Verify computed values match spec

### 4h.5: Cascade Integration Tests (Week 24)
**LOC Target**: ~1,000 (test code)

- [ ] Create `tests/test_css_cascade.c`
- [ ] Add cascade compliance tests
- [ ] Test with complex stylesheets
- [ ] Benchmark cascade performance
- [ ] Test: Full HTML+CSS → computed styles pipeline

**Success Criteria for Phase 4h**:
- ✅ Specificity calculated correctly for all selectors
- ✅ Cascade algorithm matches CSS spec
- ✅ Inheritance works for all properties
- ✅ Computed values correct
- ✅ 95%+ test coverage
- ✅ Performance: < 10ms for 1000 elements with complex CSS

---

## Phase 4i: Integration & Final Testing (Weeks 25-26)

### Scope: ~5,000 LOC (mostly test code)
### Goal: End-to-end system integration and validation

### 4i.1: System Integration (Week 25)
- [ ] Integrate tokenizer → parser → DOM pipeline
- [ ] Integrate CSS parser → cascade → computed styles
- [ ] Connect HTML parser with CSS engine
- [ ] Test full document rendering pipeline
- [ ] Fix integration bugs

### 4i.2: Compliance Testing (Week 25-26)
- [ ] Run HTML5 compliance test suite
- [ ] Run CSS compliance test suite
- [ ] Run W3C DOM compliance tests
- [ ] Achieve 85%+ pass rate on compliance tests
- [ ] Document known failures

### 4i.3: Real-World Testing (Week 26)
- [ ] Test with Wikipedia pages
- [ ] Test with GitHub pages
- [ ] Test with news sites
- [ ] Test with web apps
- [ ] Fix critical bugs

### 4i.4: Performance Optimization (Week 26)
- [ ] Profile tokenizer performance
- [ ] Profile parser performance
- [ ] Profile cascade performance
- [ ] Optimize hot paths
- [ ] Achieve performance targets

### 4i.5: Memory & Stability (Week 26)
- [ ] Run valgrind on all tests
- [ ] Fix all memory leaks
- [ ] Test with large documents (>1MB HTML)
- [ ] Test with complex stylesheets (>1MB CSS)
- [ ] Verify stability under stress

**Final Success Criteria**:
- ✅ 85%+ HTML5 compliance test pass rate
- ✅ 85%+ CSS compliance test pass rate
- ✅ 90%+ W3C DOM compliance
- ✅ No memory leaks (valgrind clean)
- ✅ Can parse Wikipedia homepage without errors
- ✅ Performance targets met for all components
- ✅ No crashes on real-world HTML/CSS

---

## Risk Analysis & Mitigation

### High-Risk Areas

#### Risk 1: Scope Creep
**Probability**: HIGH
**Impact**: HIGH
**Mitigation**:
- Stick to roadmap strictly
- Defer non-essential features to post-MVP
- Regular scope reviews every 2 weeks
- User approval required for scope changes

#### Risk 2: Adoption Agency Algorithm Complexity
**Probability**: MEDIUM
**Impact**: HIGH
**Mitigation**:
- Study Ladybird implementation thoroughly
- Implement with comprehensive tests
- Start with simplified version, iterate
- Budget extra time (Week 8-9)

#### Risk 3: CSS Cascade Edge Cases
**Probability**: MEDIUM
**Impact**: MEDIUM
**Mitigation**:
- Follow spec precisely
- Use compliance tests to catch issues
- Study real browser implementations
- Defer exotic features if needed

#### Risk 4: Performance Targets
**Probability**: MEDIUM
**Impact**: MEDIUM
**Mitigation**:
- Profile early and often
- Optimize data structures upfront
- Use arena allocation for speed
- Defer optimization if needed for MVP

#### Risk 5: Testing Burden
**Probability**: HIGH
**Impact**: MEDIUM
**Mitigation**:
- Write tests alongside implementation
- Automate test execution
- Focus on high-value tests first
- Accept 85% pass rate, not 100%

---

## Development Workflow

### Daily Process:
1. Update TODO list at start of day
2. Implement features from roadmap
3. Write unit tests for new code
4. Run all tests before committing
5. Update TODO list at end of day

### Weekly Process:
1. Review progress against roadmap
2. Update LOC estimates based on actuals
3. Identify blockers or risks
4. Adjust schedule if needed
5. Communicate status

### Testing Cadence:
- **Unit tests**: Every commit
- **Integration tests**: Every feature completion
- **Compliance tests**: Weekly
- **Memory tests**: Weekly (valgrind)
- **Performance tests**: Bi-weekly

### Code Quality Gates:
- No compiler warnings (-Wall -Wextra -Werror)
- All unit tests pass
- No memory leaks (valgrind clean)
- Code review (self-review checklist)
- Documentation updated

---

## Success Metrics

### Code Quality:
- ✅ 90%+ unit test coverage
- ✅ 85%+ compliance test pass rate
- ✅ Zero memory leaks
- ✅ Zero compiler warnings
- ✅ All public APIs documented

### Performance:
- ✅ Tokenizer: < 5ms for 10KB HTML
- ✅ Parser: < 20ms for 50KB HTML
- ✅ CSS Parser: < 20ms for 500KB CSS
- ✅ Cascade: < 10ms for 1000 elements
- ✅ Total: < 50ms for typical web page

### Compliance:
- ✅ 85%+ HTML5 parsing tests
- ✅ 85%+ CSS parsing tests
- ✅ 90%+ W3C DOM tests
- ✅ Can render real-world sites correctly

### Maintainability:
- ✅ Clear module boundaries
- ✅ Comprehensive documentation
- ✅ Self-contained (no external libs)
- ✅ Readable code (< 100 LOC per function avg)

---

## Contingency Plans

### If Behind Schedule (>2 weeks):
1. Defer non-critical features
2. Reduce compliance test coverage target (75% instead of 85%)
3. Simplify error recovery (fail fast instead of recovery)
4. Skip exotic CSS features (variables, calc, etc.)

### If Ahead of Schedule:
1. Add more compliance tests
2. Optimize performance further
3. Add nice-to-have features
4. Improve documentation

### If Critical Blocker:
1. Document blocker clearly
2. Assess impact on timeline
3. Identify workarounds
4. Escalate to user if needed

---

## Next Steps

1. ✅ **User approval**: Review and approve this roadmap
2. ✅ **Environment setup**: Ensure build system ready
3. ✅ **Begin Phase 4c.1**: Start HTML5 tokenizer foundation (Week 1)

---

**Document Version**: 1.0
**Author**: Claude (SilkSurf Development Team)
**Date**: 2025-12-30
**Status**: Pending User Approval
**Estimated Completion**: June 2026 (26 weeks from now)
