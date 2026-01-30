# CSS_INVALID Investigation: Findings & Recommendations

**Prepared for**: SilkSurf Development Team
**Date**: 2026-01-29
**Prepared by**: Claude Code Investigation
**Status**: Complete with Actionable Recommendations

## Executive Summary

**Problem**: `css_select_style()` returns CSS_INVALID (error code 3) when silksurf attempts to compute styles for DOM elements.

**Root Cause**: LibCSS's cascade algorithm requires handlers to provide defaults for all 60+ CSS properties, but silksurf only implements 4. When libcss queries for margin, padding, border, or other unhandled properties via `ua_default_for_property()`, returning CSS_INVALID tells libcss the cascade has failed entirely.

**Assessment**: This is **not a bug** - it's a **design mismatch** between libcss (designed for complete browser NetSurf) and silksurf's minimal CSS handling approach.

**Recommendation**: Build a native CSS cascade engine instead of continuing to force-fit libcss into silksurf's architecture.

---

## Part 1: Root Cause Analysis

### What Happens

1. HTML parser creates DOM element
2. Layout/rendering code calls `silk_css_get_computed_style()`
3. This calls `css_select_style()` from libcss
4. LibCSS's cascade algorithm walks stylesheets to find rules matching the element
5. For each property that isn't in a matching rule, libcss calls `ua_default_for_property()` callback
6. Silksurf's handler has only 4 properties in switch statement
7. For unhandled properties (margin, padding, border, etc.), handler returns CSS_INVALID
8. LibCSS interprets CSS_INVALID as cascade failure
9. `css_select_style()` returns error
10. Silksurf treats this as fatal error, returns -1

### Code Path

**Trigger**: Line 337 in `/home/eirikr/Github/silksurf/src/document/css_engine.c`
```c
css_error err = css_select_style(engine->select_ctx, libdom_node, &unit_ctx, &media, NULL,
                                  silk_css_get_select_handler(), (void *)engine, &results);
```

**Handler implementation**: Lines 641-677 in `/home/eirikr/Github/silksurf/src/document/css_select_handler.c`
```c
static css_error ua_default_for_property(void *pw, uint32_t property, css_hint *hint) {
    switch (property) {
        case CSS_PROP_COLOR:
            hint->data.color = 0xFF000000;
            hint->status = CSS_COLOR_COLOR;
            break;
        case CSS_PROP_DISPLAY:
            hint->status = CSS_DISPLAY_INLINE;
            break;
        case CSS_PROP_FONT_SIZE:
            hint->data.length.value = 16;
            hint->data.length.unit = CSS_UNIT_PX;
            hint->status = CSS_FONT_SIZE_DIMENSION;
            break;
        case CSS_PROP_FONT_FAMILY:
            hint->status = CSS_FONT_FAMILY_SANS_SERIF;
            break;
        default:
            return CSS_INVALID;  // <-- Line 673: Problem here
    }
    return CSS_OK;
}
```

**Error detection**: Lines 348-350 in `/home/eirikr/Github/silksurf/src/document/css_engine.c`
```c
if (err != CSS_OK || !results) {
    fprintf(stderr, "[CSS] ERROR: Style selection failed: %d (results=%p)\n", err, (void *)results);
    return -1;  // Fails here when err == 3 (CSS_INVALID)
}
```

### Why NetSurf Doesn't Have This Problem

NetSurf's reference implementation also returns CSS_INVALID for unhandled properties, but:

1. **Complete UA stylesheet**: NetSurf's UA stylesheet (user agent styles) includes defaults for all properties
2. **Complete handler**: Even though handler returns CSS_INVALID for unhandled properties, the stylesheet has already provided the value
3. **Different error path**: NetSurf's rendering pipeline may have fallback logic for style failures

**Key evidence**: From netsurf-browser/libcss/examples/example1.c:
```c
css_error ua_default_for_property(void *pw, uint32_t property, css_hint *hint) {
    // ... handles only 4 properties (color, font-family, quotes, voice-family) ...
    } else {
        return CSS_INVALID;  // NetSurf ALSO returns CSS_INVALID!
    }
}
```

---

## Part 2: Why This Matters

### LibCSS's Design Assumptions

LibCSS was built for NetSurf with these assumptions:

1. **Handler provides comprehensive defaults**
   - Handler callback should handle all commonly-used properties
   - Or user-agent stylesheet should have rules for all properties

2. **Cascade is atomic**
   - Either all properties compute successfully or cascade fails
   - No partial results allowed
   - One missing property = entire style selection fails

3. **Handler is trusted**
   - LibCSS relies on handler to do the right thing
   - If handler returns CSS_INVALID, cascade is broken
   - No error recovery mechanism

### Why This Breaks SilkSurf

SilkSurf's approach is fundamentally different:

1. **Minimal implementation**: Only handles display properties for now
2. **Not complete**: Doesn't need full CSS support immediately
3. **Modular design**: CSS should be independent from other systems

LibCSS's "all-or-nothing" design conflicts with this modular, incremental approach.

---

## Part 3: Solutions Evaluated

### Solution 1: Expand ua_default_for_property()
**Status**: Possible, but not recommended

**What it entails**:
```c
// Add handlers for all 60+ CSS properties
// Each property needs correct initial value per CSS spec
// Approximately 200+ lines of switch statements

case CSS_PROP_MARGIN_TOP:
case CSS_PROP_MARGIN_RIGHT:
case CSS_PROP_MARGIN_BOTTOM:
case CSS_PROP_MARGIN_LEFT:
    hint->data.length.value = 0;
    hint->data.length.unit = CSS_UNIT_PX;
    hint->status = CSS_MARGIN_SET;
    break;

case CSS_PROP_PADDING_TOP:
// ... repeat for padding_right, padding_bottom, padding_left ...

case CSS_PROP_BORDER_TOP_WIDTH:
// ... repeat for other border properties ...

case CSS_PROP_BACKGROUND_COLOR:
    hint->data.color = 0x00000000;  // transparent
    hint->status = CSS_BACKGROUND_COLOR_COLOR;
    break;

// ... and 40+ more properties ...
```

**Pros**:
- Quick fix (2-3 hours)
- Avoids libcss migration
- Works immediately

**Cons**:
- 200+ lines of boilerplate
- Still doesn't match silksurf's design philosophy
- Perpetuates tight coupling to libcss
- Hard to maintain when CSS spec evolves
- Doesn't give silksurf control over cascade algorithm

**Verdict**: **Not recommended** - solves immediate symptom but not underlying problem

---

### Solution 2: Error Recovery Wrapper
**Status**: Possible, quick band-aid

**What it entails**:
```c
css_error err = css_select_style(...);
if (err == CSS_INVALID) {
    // Fallback: return minimal style
    out_style->display = CSS_DISPLAY_BLOCK;
    out_style->width = -1;   // auto
    out_style->height = -1;  // auto
    out_style->color = 0xFF000000;  // black
    out_style->font_size = 16;
    fprintf(stderr, "[CSS] Style selection failed, using fallback\n");
    return 0;
}
```

**Pros**:
- Very quick (1 hour)
- Allows rendering to proceed
- Transparent to rest of codebase

**Cons**:
- Masks real problem
- Silksurf still can't control cascade
- Still coupled to libcss
- Doesn't scale to real CSS needs

**Verdict**: **Acceptable as temporary fix**, but implement Solution 3 long-term

---

### Solution 3: Native CSS Cascade Engine ⭐ RECOMMENDED
**Status**: Recommended long-term solution

**What it entails**:
1. Build pure CSS cascade engine with no external dependencies
2. Spec-compliant implementation of CSS Cascading and Inheritance Module Level 3
3. Every property has explicit initial value from CSS spec
4. Cascade algorithm is transparent and testable
5. No handler callbacks for property logic
6. Per-property error handling (one property failure doesn't break cascade)

**Implementation phases**:

**Phase 1 (1-2 weeks)**: Foundation
- Define css_property_spec table for core properties
- Implement cascade algorithm
- Basic compute functions

**Phase 2 (1-2 weeks)**: Coverage
- Add all CSS properties to spec table
- Implement compute functions
- Unit conversion (em, rem, %, etc.)

**Phase 3 (1 week)**: Integration
- Hook into DOM tree
- Cache computed styles
- Integrate with layout engine

**Phase 4 (1 week)**: Optimization
- Performance profiling
- Selector matching optimization

**Pros**:
- Aligns with silksurf's cleanroom design philosophy
- Full control over CSS implementation
- Spec-compliant and debuggable
- No external dependencies
- Better error handling (partial results)
- Easier to test and optimize
- Enables custom extensions (CSS variables, etc.)
- Self-documenting code (spec in code)

**Cons**:
- Larger initial effort (4-6 weeks total)
- Must implement all CSS properties eventually

**Verdict**: **Strongly recommended** - best long-term solution

**Timeline**: Can be phased:
- Week 1-2: Basic engine (display, color, font properties)
- Week 3-4: Full property coverage
- Week 5-6: Optimization and integration

During implementation, Solution 2 (error recovery) can be temporary workaround.

---

## Part 4: Detailed Recommendation

### Immediate Action (Next 1-2 Hours)

**Option A**: Add error recovery wrapper
```c
// In css_engine.c, around line 348
if (err != CSS_OK) {
    if (err == CSS_INVALID) {
        // Cascade failed: provide minimal fallback
        fprintf(stderr, "[CSS] WARNING: Style selection returned CSS_INVALID, using defaults\n");
        out_style->display = CSS_DISPLAY_BLOCK;
        out_style->width = -1;
        out_style->height = -1;
        out_style->color = 0xFF000000;
        out_style->font_size = 16;
        return 0;  // Success with fallback
    }
    return -1;
}
```

**Impact**: Allows rendering to proceed with reasonable defaults. Marks as temporary with clear logging.

### Short Term (Next 1-2 Weeks)

**Expand handler** (if absolutely necessary):
- Add handlers for box model properties (margin, padding, border)
- Add handlers for background properties
- Document in CLAUDE.md as temporary workaround

**Pros**: Enables more CSS properties
**Cons**: Still not permanent solution

### Long Term (Weeks 3-6)

**Implement native CSS engine**:
1. Study MODERN_CSS_ENGINE_DESIGN.md (included in this investigation)
2. Build property specification table
3. Implement cascade algorithm
4. Add compute functions for each property
5. Integrate with DOM tree
6. Benchmark and optimize

**Payoff**:
- Self-sufficient CSS implementation
- No libcss dependency
- Full control over cascade behavior
- Better error handling
- Spec-compliant
- Maintainable and extensible

---

## Part 5: Decision Matrix

| Factor | LibCSS + Workaround | LibCSS + Handler Expansion | Native Engine |
|--------|--------------------|-----------------------------|---------------|
| **Time to fix** | 1 hour | 2-3 hours | 4-6 weeks |
| **Code clarity** | Low | Medium | High |
| **Spec compliance** | None | Partial | Full |
| **Error handling** | Bad | Bad | Good |
| **Scalability** | None | Low | High |
| **Dependencies** | libcss | libcss | None |
| **Future maintenance** | High | High | Low |
| **Control** | None | Low | Full |
| **Debugging** | Hard | Hard | Easy |

**Recommendation**: 
- **Immediate** (this week): LibCSS + Error Recovery (Solution 2)
- **Medium term** (if CSS needs expand): LibCSS + Handler Expansion (Solution 1)
- **Long term** (planned architecture): Native Engine (Solution 3)

---

## Part 6: Design Philosophy

### LibCSS Approach
- Handler pattern: "My code talks to libcss via callbacks"
- Cascade inside black box
- Tightly coupled

### SilkSurf Cleanroom Approach
- Specification-driven: "CSS spec → code, directly"
- Transparent cascade algorithm
- Loosely coupled components

**Native CSS engine fits SilkSurf's philosophy better.**

---

## Part 7: Documentation Provided

Three comprehensive documents have been created:

1. **CSS_INVALID_INVESTIGATION.md** (600+ lines)
   - Complete root cause analysis
   - LibCSS design explanation
   - Comparison with NetSurf
   - All evidence and code paths
   - Architecture implications

2. **MODERN_CSS_ENGINE_DESIGN.md** (800+ lines)
   - Complete specification for native CSS engine
   - Data structures and algorithms
   - Property specification system
   - Cascade algorithm pseudocode
   - Integration points
   - Testing strategy
   - Implementation roadmap

3. **CSS_FINDINGS_AND_RECOMMENDATIONS.md** (this document)
   - Executive summary
   - Solution evaluation
   - Decision matrix
   - Specific recommendations with timelines

---

## Part 8: Next Steps

### If choosing Solution 2 (Error Recovery)
1. Implement wrapper in css_engine.c
2. Add logging for transparency
3. Document in CLAUDE.md
4. Plan migration to Solution 3

### If choosing Solution 3 (Native Engine)
1. Read MODERN_CSS_ENGINE_DESIGN.md thoroughly
2. Start with Phase 1 (property specs + basic cascade)
3. Use Solution 2 as temporary fallback during implementation
4. Run tests against real CSS during each phase
5. Integrate with layout/rendering as phases complete

---

## Conclusion

**The CSS_INVALID error is a natural consequence of using libcss minimally.** LibCSS was designed for complete browser implementations; SilkSurf's minimalist approach conflicts with libcss's all-or-nothing cascade design.

**Three solutions exist**, with increasing maturity:
1. Quick band-aid (1 hour) - temporary
2. Handler expansion (2-3 hours) - extends status quo
3. Native engine (4-6 weeks) - long-term optimal

**Recommendation**: Use Solution 2 immediately to unblock current work, then plan for Solution 3 as part of next architectural phase.

All necessary technical specifications and architectural guidance are provided in the included design documents.

---

## References

**Included Documents**:
- CSS_INVALID_INVESTIGATION.md - Detailed root cause analysis
- MODERN_CSS_ENGINE_DESIGN.md - Native engine specification

**External Resources**:
- [NetSurf LibCSS](https://www.netsurf-browser.org/projects/libcss/)
- [CSS Cascading and Inheritance Level 3](https://www.w3.org/TR/css-cascade-3/)
- [CSS Initial Values](https://www.w3.org/TR/css-values-3/#initial-value)
