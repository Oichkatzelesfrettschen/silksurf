# SilkSurf Architecture Analysis
## Browser Engine Comparative Study
### Date: 2025-12-30
### Purpose: Design self-contained HTML5/CSS/DOM implementation

---

## Executive Summary

After analyzing four major browser implementations (Ladybird, Servo, NetSurf, Sciter), we have identified optimal patterns for implementing a **self-contained, cleanroom** HTML5 parser, CSS engine, and DOM for SilkSurf.

**Critical Requirement**: SilkSurf MUST be independent of libdom and libcss. All crashes in Phase 4a/4b were caused by external library dependencies. We need a pure, self-contained C implementation.

---

## Browser Analysis Results

### 1. Ladybird (C++ - Modern, Standards-Compliant)

**Architecture:**
- **Total Size**: ~400,000 LOC for HTML parsing
- **Language**: Modern C++ with GC integration
- **License**: BSD-2-Clause

**HTML Parser Structure:**
```
LibWeb/HTML/Parser/
├── HTMLTokenizer.cpp (~120k LOC, 70+ states)
├── HTMLParser.cpp (~265k LOC, 21 insertion modes)
├── HTMLToken.h (6 token types)
├── StackOfOpenElements
├── ListOfActiveFormattingElements
└── Entities.json (146k - HTML entity table)
```

**Key Patterns:**
1. **Tokenizer-Parser Split**: Clean separation between lexical analysis and tree construction
2. **State Machines**:
   - Tokenizer: 70+ states for character-by-character processing
   - Parser: 21 insertion modes following HTML5 spec
3. **Data Structures**:
   - Stack of open elements (tag matching)
   - List of active formatting elements (for `<b>`, `<i>`, etc.)
   - Adoption agency algorithm for misnested tags
4. **Token Types**:
   - DOCTYPE, StartTag, EndTag, Comment, Character, EndOfFile
   - Position tracking for error reporting
   - Attribute normalization
5. **GC Integration**: Uses LibGC throughout (not applicable to SilkSurf)

**Strengths:**
- Comprehensive HTML5 compliance
- Well-tested in production
- Clear separation of concerns

**Weaknesses for SilkSurf:**
- Massive codebase (~400k LOC)
- C++ dependencies (GC, FlyString, etc.)
- Too complex for cleanroom reimplementation

---

### 2. Servo (Rust - Parallel, Modern)

**Architecture:**
- **Language**: Rust
- **CSS Implementation**: ~4,470 LOC in DOM layer
- **License**: MPL 2.0

**CSS Structure:**
```
components/script/dom/css/
├── css.rs
├── cssconditionrule.rs
├── cssmediarule.rs
├── cssstylerule.rs
├── cssstylesheet.rs
├── cssstyledeclaration.rs
└── [25 total CSS DOM modules]
```

**Layout Structure:**
```
components/layout/
├── stylesheets/ (UA, quirks, presentational hints)
├── flexbox/
├── flow/
├── fragment_tree/
└── table/
```

**Key Patterns:**
1. **Modular Design**: Separate CSS DOM from layout engine
2. **External Dependencies**: Uses stylo/style crate (Gecko's style system)
3. **Parallel Architecture**: Lock-free data structures, work-stealing
4. **Rust Safety**: Memory safety without GC overhead

**Strengths:**
- Modern parallel design
- Memory safe
- Modular architecture

**Weaknesses for SilkSurf:**
- Rust language barrier
- External style crate dependency
- Designed for multi-threading (SilkSurf is single-threaded)

---

### 3. NetSurf (C - Lightweight, Cleanroom)

**Architecture:**
- **Language**: Pure C
- **HTML Handler**: ~28,784 LOC
- **License**: GPL

**HTML Structure:**
```
content/handlers/html/
├── html.c (main handler)
├── box_construct.c (box tree construction)
├── box_normalise.c (box tree normalization)
├── css.c (stylesheet management)
├── layout.c (layout engine)
├── form.c, forms.c (form handling)
└── table.c (table layout)
```

**Key Patterns:**
1. **Box Model**: Builds intermediate box tree from DOM
2. **Modular Handlers**: Separate content type handlers (HTML, CSS, images)
3. **External Libraries**: Uses libdom for DOM, libcss for CSS parsing
4. **Lightweight**: Designed for embedded systems
5. **C Structs**: Simple C data structures, manual memory management

**Strengths:**
- Pure C implementation
- Lightweight and portable
- Well-suited for embedded
- Clear module boundaries

**Weaknesses for SilkSurf:**
- Still depends on libdom/libcss (CRITICAL ISSUE)
- Box model adds complexity
- Limited HTML5 support

---

### 4. Sciter (C++ - Embeddable)

**Notes:**
- Archived repository (read-only)
- Proprietary licensing model
- Embeddable HTML/CSS engine
- Not fully open-source
- Did not analyze in depth due to licensing concerns

---

## Critical Insights for SilkSurf

### 1. The External Library Problem

**All current approaches have a fatal flaw for SilkSurf:**
- **Ladybird**: Depends on LibGC, AK (SerenityOS foundation)
- **Servo**: Depends on stylo/style crate
- **NetSurf**: Depends on libdom and libcss (EXACTLY our problem in Phase 4a/4b)

**Our crashes in Phase 4b were caused by:**
- libcss expecting raw libdom node pointers
- Handler callback mismatches
- Reference counting bugs across library boundaries
- NULL function pointers in callback structures

**Conclusion**: We MUST implement everything self-contained.

---

### 2. Optimal Architecture for SilkSurf

Based on analysis, the optimal design combines:
- **Ladybird's** tokenizer-parser split pattern
- **NetSurf's** lightweight C approach
- **Servo's** modular design philosophy
- **Custom implementation**: No external DOM/CSS libraries

---

## Recommended SilkSurf Architecture

### Phase 1: Self-Contained HTML5 Parser

**Size Target**: 15,000 - 25,000 LOC (much smaller than Ladybird's 400k)

**Architecture:**
```
src/document/
├── html_tokenizer.c (~3,000 LOC)
│   ├── Character-by-character state machine
│   ├── 12 core states (simplified from Ladybird's 70+)
│   └── Entity decoding (inline, not external JSON)
│
├── html_token.c (~500 LOC)
│   ├── Token types: DOCTYPE, StartTag, EndTag, Comment, Character, EOF
│   └── Attribute storage
│
├── html_parser.c (~8,000 LOC)
│   ├── Tree construction state machine
│   ├── 8-10 insertion modes (simplified from Ladybird's 21)
│   ├── Open element stack
│   ├── Formatting element list
│   └── Minimal adoption agency algorithm
│
├── dom_core.c (~5,000 LOC)
│   ├── Node types: Element, Text, Comment, Document
│   ├── Tree operations: append, insert, remove
│   ├── Attribute management
│   └── Reference counting (arena-based)
│
└── html_entities.c (~2,000 LOC)
    └── Common entity table (not full HTML5 set)
```

**Key Design Decisions:**

1. **Simplified State Machines**:
   - Tokenizer: 12 states instead of 70+
   - Parser: 8-10 insertion modes instead of 21
   - Focus on common HTML, skip exotic edge cases

2. **Arena Allocation**:
   - All DOM nodes allocated from arena
   - No malloc/free for individual nodes
   - Reference counting for cross-arena references

3. **No External Dependencies**:
   - Self-contained entity table
   - No libdom, no libcss
   - Pure C99, POSIX-compatible

4. **Token Stream Interface**:
   ```c
   typedef enum {
       HTML_TOKEN_DOCTYPE,
       HTML_TOKEN_START_TAG,
       HTML_TOKEN_END_TAG,
       HTML_TOKEN_COMMENT,
       HTML_TOKEN_CHARACTER,
       HTML_TOKEN_EOF
   } silk_html_token_type_t;

   typedef struct {
       silk_html_token_type_t type;
       char *tag_name;
       silk_html_attribute_t *attributes;
       int attribute_count;
       uint32_t code_point;  /* for CHARACTER tokens */
   } silk_html_token_t;
   ```

---

### Phase 2: Self-Contained CSS Engine

**Size Target**: 8,000 - 12,000 LOC

**Architecture:**
```
src/document/
├── css_tokenizer.c (~2,000 LOC)
│   ├── CSS syntax tokenization
│   ├── String handling, comments, whitespace
│   └── Number and unit parsing
│
├── css_parser.c (~4,000 LOC)
│   ├── Selector parsing
│   ├── Property value parsing
│   ├── Media query support (basic)
│   └── Stylesheet construction
│
├── css_cascade.c (~3,000 LOC)
│   ├── Specificity calculation
│   ├── Origin cascade (UA → Author → Inline)
│   ├── Inheritance
│   └── Computed value resolution
│
└── css_select.c (~2,000 LOC)
    ├── Selector matching against DOM
    ├── Type, class, ID, attribute selectors
    └── Pseudo-classes (:hover, :focus, etc.)
```

**Key Design Decisions:**

1. **Minimal Selector Support**:
   - Type selectors: `div`, `p`, `span`
   - Class selectors: `.classname`
   - ID selectors: `#idname`
   - Attribute selectors: `[attr=value]`
   - Descendant: `div p`
   - Child: `div > p`
   - Skip complex pseudo-selectors for MVP

2. **Essential Properties Only**:
   - Display (block, inline, none)
   - Box model (width, height, margin, padding, border)
   - Colors (color, background-color)
   - Typography (font-size, font-family, text-align)
   - Positioning (position, top, left, etc.)
   - Skip animations, transforms, filters for MVP

3. **Inline Cascade Implementation**:
   ```c
   typedef struct {
       uint32_t origin;      /* UA=0, Author=1, Inline=2 */
       uint32_t specificity; /* (a,b,c,d) packed into uint32 */
       uint32_t order;       /* Source order */
   } silk_css_cascade_key_t;

   /* Winner = max(origin, then specificity, then order) */
   ```

---

### Phase 3: Self-Contained DOM

**Size Target**: 6,000 - 8,000 LOC

**Architecture:**
```
src/document/
├── dom_node.c (~2,500 LOC)
│   ├── Node base class
│   ├── Element node
│   ├── Text node
│   ├── Comment node
│   └── Tree traversal
│
├── dom_element.c (~2,000 LOC)
│   ├── Attribute management
│   ├── Class list
│   ├── ID lookup
│   └── Tag name matching
│
├── dom_document.c (~1,500 LOC)
│   ├── Document root
│   ├── Element factory
│   ├── Node lookup (getElementById, etc.)
│   └── Tree modification
│
└── dom_string.c (~1,000 LOC)
    ├── String pooling
    ├── Case-insensitive comparison
    └── Arena-allocated strings
```

**Key Design Decisions:**

1. **No External libdom**:
   - Implement minimal W3C DOM API
   - Focus on what layout engine needs
   - Skip exotic DOM features

2. **Arena-Based Memory**:
   - All nodes from arena
   - Automatic cleanup on arena destroy
   - Reference counting only for cross-arena refs

3. **Direct Integration with Parser**:
   ```c
   /* Parser directly creates DOM nodes */
   silk_dom_element_t *element = silk_dom_create_element(
       doc, token->tag_name);

   /* No libdom wrapper needed */
   silk_dom_append_child(current, element);
   ```

---

## Implementation Phases

### Phase 4c: HTML5 Tokenizer (Week 1-2)
- [ ] Implement character stream reader
- [ ] Build state machine (12 states)
- [ ] Add entity decoding
- [ ] Write tokenizer tests

### Phase 4d: HTML5 Parser (Week 3-4)
- [ ] Implement insertion mode state machine
- [ ] Build open element stack
- [ ] Add formatting element list
- [ ] Implement tree construction algorithms
- [ ] Write parser tests

### Phase 4e: Self-Contained DOM (Week 5-6)
- [ ] Implement node types (Element, Text, Comment)
- [ ] Build tree operations
- [ ] Add attribute management
- [ ] Implement document methods
- [ ] Write DOM tests

### Phase 4f: CSS Tokenizer (Week 7-8)
- [ ] Implement CSS syntax tokenizer
- [ ] Add number and unit parsing
- [ ] Handle strings and comments
- [ ] Write CSS tokenizer tests

### Phase 4g: CSS Parser (Week 9-10)
- [ ] Implement selector parser
- [ ] Add property value parser
- [ ] Build stylesheet construction
- [ ] Write CSS parser tests

### Phase 4h: CSS Cascade (Week 11-12)
- [ ] Implement specificity calculation
- [ ] Add cascade algorithm (origin, specificity, order)
- [ ] Implement inheritance
- [ ] Write cascade tests

### Phase 4i: Integration & Testing (Week 13-14)
- [ ] Integrate tokenizer → parser → DOM
- [ ] Integrate CSS parsing → cascade → computed styles
- [ ] Full system testing
- [ ] Performance optimization
- [ ] Memory leak detection

---

## Code Size Comparison

| Component | Ladybird | SilkSurf Target | Reduction |
|-----------|----------|-----------------|-----------|
| HTML Tokenizer | ~120,000 | ~3,000 | 97.5% |
| HTML Parser | ~265,000 | ~8,000 | 97.0% |
| DOM | (in LibWeb) | ~6,000 | N/A |
| CSS Cascade | (in LibWeb) | ~8,000 | N/A |
| **TOTAL** | **~400,000** | **~25,000** | **93.75%** |

**How we achieve 93% reduction:**
1. Simplified state machines (fewer states/modes)
2. Focus on common HTML/CSS features
3. Skip exotic edge cases
4. No unicode normalization
5. No complex selector support
6. Inline entity tables instead of external files
7. No GC overhead
8. Pure C, no C++ template expansion

---

## Key Principles

### 1. Self-Containment is Non-Negotiable
- **NO libdom**: Implement our own DOM
- **NO libcss**: Implement our own CSS parser/cascade
- **NO external libraries**: Except standard C library

### 2. Simplicity Over Completeness
- Support 80% of web content with 20% of complexity
- Skip exotic features
- Focus on correctness for common cases

### 3. Arena Allocation
- All parsing allocations from arena
- Fast allocation, fast cleanup
- No per-node malloc/free

### 4. Clean Interfaces
- Clear separation: Tokenizer → Parser → DOM
- CSS Parser → Cascade → Computed Styles
- Testable components

### 5. Incremental Implementation
- Build vertically: One feature end-to-end
- Test continuously
- Integrate frequently

---

## Testing Strategy

### Unit Tests
- Tokenizer: Character stream → Tokens
- Parser: Tokens → DOM tree
- CSS Parser: CSS text → Stylesheet
- Cascade: Stylesheets → Computed styles

### Integration Tests
- HTML → DOM tree construction
- CSS → Styled DOM
- Full document rendering pipeline

### Compliance Tests
- HTML5 parsing test suite (subset)
- CSS test suite (subset)
- Real-world HTML samples

---

## Success Criteria

### Phase 4 Complete When:
1. ✅ Can parse simple HTML documents into DOM tree
2. ✅ Can parse simple CSS stylesheets
3. ✅ Can compute styles for DOM elements
4. ✅ No crashes (unlike Phase 4a/4b with libdom/libcss)
5. ✅ All unit tests pass
6. ✅ Can render test_document_full.html correctly
7. ✅ Memory clean (no leaks detected by valgrind)
8. ✅ Performance acceptable (< 50ms for typical page)

---

## References

### Analyzed Repositories
- Ladybird: `~/Github/ladybird/Libraries/LibWeb/`
- Servo: `~/Github/servo/components/`
- NetSurf: `~/Github/netsurf/content/handlers/`
- Sciter: `~/Github/sciter-sdk/` (limited analysis)

### Specifications
- HTML5: https://html.spec.whatwg.org/
- CSS Syntax: https://drafts.csswg.org/css-syntax/
- CSS Cascade: https://drafts.csswg.org/css-cascade/
- DOM: https://dom.spec.whatwg.org/

### Key Findings Documents
- Ladybird parser states: `ENUMERATE_TOKENIZER_STATES` macro
- Ladybird insertion modes: `ENUMERATE_INSERTION_MODES` macro
- NetSurf box model: `content/handlers/html/box_construct.c`

---

## Conclusion

The path forward is clear: **Implement a self-contained, cleanroom HTML5/CSS/DOM engine** for SilkSurf. By combining the architectural patterns from Ladybird's comprehensive design with NetSurf's lightweight C approach, and avoiding ALL external library dependencies, we can build a robust, crash-free rendering engine in approximately 25,000 LOC of pure C.

The previous Phase 4a/4b failures taught us that external libraries (libdom, libcss) introduce unacceptable complexity and fragility. A self-contained implementation will be:
- **More reliable**: No library boundary bugs
- **More maintainable**: All code under our control
- **More portable**: Pure C, minimal dependencies
- **Easier to debug**: No black-box library behavior

**Next step**: Begin Phase 4c - HTML5 Tokenizer implementation.

---

**Document Version**: 1.0
**Author**: Claude (SilkSurf Development Team)
**Date**: 2025-12-30
**Status**: Approved for Implementation
