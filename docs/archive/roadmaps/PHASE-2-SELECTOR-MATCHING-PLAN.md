# Phase 2: Selector Matching Integration - PLAN

**Created**: 2026-01-29
**Status**: Planning
**Dependencies**: Phase 1 Complete (Native CSS Cascade Engine)

---

## Objective

Integrate native CSS cascade engine with selector matching to complete the CSS pipeline:
- **Input**: DOM element + stylesheets
- **Process**: Match rules → Apply cascade → Compute final values
- **Output**: Computed style

Replace LibCSS's integrated cascade with our native implementation while keeping selector matching.

---

## Current Architecture

```
LibCSS Flow (Current):
┌─ Selector Matching ──┐
│  (Find matching rules) │
└───────────────────────┘
           ↓
┌─ LibCSS Cascade ──┐  ← We're replacing THIS
│  (Apply cascade)   │
└────────────────────┘
           ↓
┌─ Computed Values ──┐
│  (Extract results)  │
└────────────────────┘
```

```
Native Flow (Phase 2):
┌─ Selector Matching ──────┐
│  (Find matching rules)    │  ← Keep LibCSS
└──────────────────────────┘
           ↓
┌─ Native Cascade ────────┐
│  (Apply cascade)         │  ← Replace with our engine
│  ✓ Per-property errors   │
│  ✓ Spec-compliant        │
└─────────────────────────┘
           ↓
┌─ Public API Converter ──┐
│  (silk_computed_style_t) │  ← Already implemented (Phase 1)
└────────────────────────┘
```

---

## Task Breakdown

### Phase 2.1: Rule Extraction (Days 1-2)

**Goal**: Extract matched rules from LibCSS without triggering cascade failure

**Current Problem**:
- `css_select_style()` fails with CSS_INVALID when `ua_default_for_property()` returns error
- Cascade fails before we can get matched rules

**Solution Approaches**:

**Option A: Use LibCSS Pre-Match Hook (Recommended)**
- LibCSS allows querying for matched rules before cascade
- Use `css_select_ctx_get_rules()` or similar
- Extract rules + specificity without triggering cascade
- Avoids segfault, gets what we need

**Option B: Implement Selector Matching**
- Parse selectors from stylesheets
- Implement CSS selector matching algorithm
- More work, but removes LibCSS dependency
- Better long-term (Phase 3)

**Option C: Patch LibCSS Handler**
- Expand `ua_default_for_property()` to handle all properties
- Prevents CSS_INVALID error
- Quick but not optimal

**Recommendation**: Option A (least disruption, fastest)

### Phase 2.2: Rule Conversion (Days 2-3)

**Goal**: Convert LibCSS rules to native css_rule format

**Input**: LibCSS matched rules + specificity + origin
**Output**: css_rule[] array ready for native cascade

```c
// Target conversion
struct css_rule native_rules[matched_count];

for (each matched libcss rule) {
    native_rules[i].properties = extract_properties(libcss_rule);
    native_rules[i].property_count = count_properties(libcss_rule);
}
```

**Properties to Extract**:
- Property ID (maps libcss property enum to our CSS_PROP_* enum)
- Property value (extract length, color, keyword, etc.)
- Track specificity (already in LibCSS results)
- Track origin (author, UA, !important)

### Phase 2.3: Cascade Integration (Days 3-5)

**Goal**: Use native cascade instead of LibCSS cascade

**Current Flow**:
```c
err = css_select_style(...);  // ← Triggers cascade
// Extract from libcss results
```

**New Flow**:
```c
// 1. Get matched rules from libcss (no cascade)
matched_rules = get_matched_rules_from_libcss(...);

// 2. Convert to native format
native_rules = convert_to_native_format(matched_rules);

// 3. Run native cascade
css_cascade_context ctx = {
    .matched_rules = native_rules,
    .matched_rule_count = count,
    .specificities = specificities,
    .origins = origins,
    .parent = parent_node,
    .parent_computed = parent_style,
};
css_cascade_for_element(&ctx, &computed_native);

// 4. Convert to public API format
css_convert_to_silk_style(&computed_native, out_style);
```

### Phase 2.4: Testing & Validation (Days 5-6)

**Goals**:
- Fix css_cascade test (remove segfault)
- Validate selector matching still works
- Test cascade integration end-to-end
- Performance benchmarking

**Test Cases**:
1. Tag selectors (div, p, span, etc.)
2. Class selectors (.header, .content)
3. ID selectors (#main, #sidebar)
4. Attribute selectors ([type="text"])
5. Pseudo-classes (:hover, :first-child)
6. Complex selectors (div > p.main)
7. Multiple matching rules (cascade order)
8. !important rules
9. Specificity ordering

**Target**: 10/10 tests passing (currently 9/10)

---

## Implementation Details

### LibCSS Integration Points

**What We Keep**:
- `css_stylesheet` parsing (libcss is excellent at this)
- `css_select_ctx` for stylesheet storage
- Handler callbacks for DOM traversal (node_name, node_classes, etc.)

**What We Replace**:
- `css_select_style()` → custom wrapper with fallback
- Internal cascade algorithm → `css_cascade_for_element()`
- Computed style format → our `css_computed_style` + converter

### Property Mapping

Create mapping table between LibCSS and Native property IDs:

```c
// libcss enum → our enum
typedef struct {
    uint32_t libcss_id;
    css_property_id native_id;
} property_mapping_t;

static property_mapping_t property_map[] = {
    {CSS_PROP_COLOR, CSS_PROP_COLOR},
    {CSS_PROP_DISPLAY, CSS_PROP_DISPLAY},
    {CSS_PROP_FONT_SIZE, CSS_PROP_FONT_SIZE},
    // ... 23 more mappings
};
```

### Value Extraction

Handle different LibCSS value types:

```c
// LibCSS result format varies by property
switch (libcss_property_type) {
    case CSS_COLOR_COLOR:
        css_color color = css_computed_color(computed, &color_val);
        native_value.color = color_val;
        break;

    case CSS_DIMENSION:
        css_fixed val = 0;
        css_unit unit = CSS_UNIT_PX;
        css_computed_width(computed, &val, &unit);
        native_value.length.value = val;
        native_value.length.unit = map_unit(unit);
        break;

    case CSS_KEYWORD:
        uint8_t keyword = css_computed_display(computed, NULL);
        native_value.keyword = map_keyword(keyword);
        break;
}
```

---

## Risks & Mitigations

### Risk 1: LibCSS API Changes
- **Impact**: Matched rules extraction API might not exist
- **Mitigation**: Check LibCSS source for rule access API
- **Fallback**: Implement selector matching ourselves

### Risk 2: Property Mapping Gaps
- **Impact**: Some properties don't map correctly
- **Mitigation**: Create comprehensive mapping table, test each property
- **Fallback**: Use default values for unmapped properties

### Risk 3: Specificity Calculation Mismatch
- **Impact**: Our cascade orders rules differently than expected
- **Mitigation**: Validate specificity calculation matches CSS spec
- **Fallback**: Log specificity values, debug with test cases

### Risk 4: Unit Conversion Issues
- **Impact**: em, rem, % values don't compute correctly
- **Mitigation**: Test unit conversion thoroughly in Phase 2.4
- **Fallback**: Implement unit resolution tests in test_css_cascade_native

---

## Success Criteria

1. **Functional**
   - [ ] 10/10 tests passing (including css_cascade)
   - [ ] No segfaults
   - [ ] Cascade works for all test cases

2. **Performance**
   - [ ] Style computation < 1ms per element
   - [ ] No memory leaks
   - [ ] Comparable to LibCSS performance

3. **Code Quality**
   - [ ] 0 compiler warnings
   - [ ] Proper error handling
   - [ ] Clear integration points

4. **Architecture**
   - [ ] Native cascade properly decoupled from selector matching
   - [ ] Easy to replace LibCSS selectors in Phase 3
   - [ ] Public API unchanged

---

## Timeline

| Phase | Tasks | Duration |
|-------|-------|----------|
| 2.1 | Rule extraction, API exploration | Days 1-2 |
| 2.2 | Rule conversion, value mapping | Days 2-3 |
| 2.3 | Cascade integration, testing | Days 3-5 |
| 2.4 | Full validation, performance test | Days 5-6 |
| **Total** | | **6 days** |

---

## Files to Create/Modify

### New Files
- `src/document/css_selector_matching.h` - Rules extraction API
- `src/document/css_selector_matching.c` - LibCSS integration wrapper
- `src/document/css_property_mapping.h` - Property ID mapping table

### Modified Files
- `src/document/css_engine.c` - Integration point
- `tests/test_css_cascade.c` - Should pass with new integration
- `CMakeLists.txt` - May need updates for new files

---

## Phase 3 Preview

Once Phase 2 is complete (native cascade with LibCSS selectors):

**Phase 3 Options**:
1. **Keep LibCSS Selectors**: Stop here, keep working system
2. **Replace Selectors**: Implement native selector matching
3. **Optimize**: Add selector caching, CSS rule indexing

### Phase 3 Estimate: 2-3 weeks
- Selector matching algorithm: 1 week
- Integration: 3-5 days
- Performance optimization: 3-5 days

---

## Notes for Implementation

### Debugging Tips
1. Add logging to trace rule extraction
2. Compare LibCSS vs native cascade results on same element
3. Use test_css_cascade_native tests as regression suite
4. Validate specificity calculation matches CSS spec

### Key Concepts
- **Specificity**: (inline, IDs, classes+attributes, elements)
- **Origin**: UA < Author < Author !important
- **Cascade Order**: Later rules win unless origin/specificity differs
- **Inheritance**: Some properties inherit from parent

### References
- CSS Cascading and Inheritance Level 3: https://www.w3.org/TR/css-cascade-3/
- LibCSS API: https://www.netsurf-browser.org/projects/libcss/
- Selector Specificity: https://www.w3.org/TR/selectors-3/#specificity

---

## Conclusion

Phase 2 bridges the gap between selector matching (LibCSS) and cascade (native engine). Once complete, SilkSurf will have a modern CSS pipeline with per-property error handling and full spec compliance.

The architecture allows Phase 3 to remove LibCSS selectors without disturbing the cascade implementation.
