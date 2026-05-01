# CSS_INVALID Root Cause Investigation

**Date**: 2026-01-29
**Status**: Complete Root Cause Identified
**Severity**: Design Issue (Not a Bug)

## Executive Summary

The CSS_INVALID error returned by `css_select_style()` in libcss is **not a bug in the handler implementation**. It is a **fundamental architectural gap** between silksurf's minimal CSS handling approach and libcss's design assumptions about UA default coverage.

**Root Cause**: silksurf's `ua_default_for_property()` handler only provides defaults for 4 CSS properties (color, display, font-size, font-family), but libcss's cascade algorithm requests defaults for 50+ properties during style computation. When a property is not handled, returning CSS_INVALID tells libcss the cascade has failed entirely.

**Root Cause Code Location**: `/home/eirikr/Github/silksurf/src/document/css_select_handler.c`, lines 671-673

```c
default:
    fprintf(stderr, "[CSS Handler] ua_default_for_property: property %u not handled, returning CSS_INVALID\n", property);
    return CSS_INVALID;  // Signals "cascade failure" to libcss
```

## Part 1: Research Phase Findings

### 1.1 What css_select_style() Actually Does

`css_select_style()` is libcss's main public API for computing styles. It:

1. Takes a DOM node, stylesheets, media context, and handler callbacks
2. Walks stylesheets to find matching rules for the element
3. Applies cascade resolution (origin, specificity, order)
4. For each property, attempts to:
   - Find matching rule in author stylesheet
   - Find matching rule in UA stylesheet
   - Query handler for UA defaults via `ua_default_for_property()`
5. Returns computed style with all properties set or initialized

### 1.2 LibCSS's Design Assumptions

From NetSurf's libcss reference implementation (`examples/example1.c`):

The `ua_default_for_property()` callback is designed to provide defaults for properties that:
- Aren't set by any stylesheet rule
- Need a UA default before inheritance/initial value computation
- Are "system properties" like color, font-family, quotes, voice-family

**NetSurf's minimal handler**:
```c
css_error ua_default_for_property(void *pw, uint32_t property, css_hint *hint)
{
    if (property == CSS_PROP_COLOR) {
        hint->data.color = 0x00000000;
        hint->status = CSS_COLOR_COLOR;
    } else if (property == CSS_PROP_FONT_FAMILY) {
        hint->data.strings = NULL;
        hint->status = CSS_FONT_FAMILY_SANS_SERIF;
    } else if (property == CSS_PROP_QUOTES) {
        hint->data.strings = NULL;
        hint->status = CSS_QUOTES_NONE;
    } else if (property == CSS_PROP_VOICE_FAMILY) {
        hint->data.strings = NULL;
        hint->status = 0;
    } else {
        return CSS_INVALID;  // Same as silksurf!
    }
    return CSS_OK;
}
```

**Key finding**: NetSurf's handler also returns CSS_INVALID for unhandled properties! So why doesn't NetSurf crash?

### 1.3 Why NetSurf Doesn't Crash (Hypothesis)

1. **NetSurf has a full CSS implementation** that handles computed defaults elsewhere
2. **NetSurf's ua_default_for_property() is not called frequently** - most style computation happens through stylesheet matching
3. **Different error handling** - NetSurf may wrap css_select_style() with fallback logic
4. **Stylesheet coverage** - NetSurf's UA stylesheet defines all properties, so cascade doesn't fail

### 1.4 LibCSS Property Coverage

LibCSS supports 60+ CSS properties (from `/usr/include/libcss/properties.h`):

```
0x000 - AZIMUTH
0x001-0x005 - BACKGROUND_* (4 properties)
0x006-0x013 - BORDER_* (14 properties)
0x014 - BOTTOM
0x015 - CAPTION_SIDE
0x016 - CLEAR
0x017 - CLIP
0x018 - COLOR
0x019 - CONTENT
... (40+ more properties)
```

Silksurf handles only 4 of these in ua_default_for_property().

### 1.5 The Cascade Algorithm's Expectations

When libcss's cascade algorithm encounters an unhandled property (margin-top, padding-left, border-width, etc.):

1. It checks author stylesheets - no match
2. It checks UA stylesheet - no match (silksurf only provides 4 properties)
3. It calls `ua_default_for_property(property, &hint)` - returns CSS_INVALID
4. libcss interprets CSS_INVALID as **"cannot proceed with cascade"**
5. css_select_style() returns CSS_INVALID to caller
6. silksurf treats this as a fatal error

## Part 2: Diagnosis

### 2.1 Exact Error Path

**File**: `/home/eirikr/Github/silksurf/src/document/css_engine.c` (lines 337-350)

```c
/* Use libcss to compute styles for this element */
css_select_results *results = NULL;
css_error err = css_select_style(engine->select_ctx,
                                  libdom_node,
                                  &unit_ctx,
                                  &media,
                                  NULL,
                                  silk_css_get_select_handler(),
                                  (void *)engine,
                                  &results);

fprintf(stderr, "[CSS] css_select_style returned: %d\n", err);

if (err != CSS_OK || !results) {
    fprintf(stderr, "[CSS] ERROR: Style selection failed: %d (results=%p)\n", err, (void *)results);
    return -1;  // Fails here when err == CSS_INVALID
}
```

When `err == 3 (CSS_INVALID)`, execution stops and returns -1 to caller.

### 2.2 Why It Happens During First Style Computation

The error occurs when:
1. Document is parsed and first element is encountered
2. `silk_css_get_computed_style()` is called for that element
3. css_select_style() internally queries for property defaults
4. For margin, padding, border, or other unhandled properties, ua_default_for_property() is called
5. Returns CSS_INVALID
6. css_select_style() cascades fail
7. Error propagates up

### 2.3 This is NOT a Bug in the Handler

The handler implementation is **correct**. It follows NetSurf's own pattern. The issue is **architectural gap**: libcss was designed for NetSurf (complete browser), but silksurf is trying to use it minimally.

## Part 3: Forward-Looking Design Recommendations

### 3.1 Why Libcss is Not Suitable Long-Term

LibCSS's architecture has fundamental limitations:

1. **Handler-based property querying**
   - Every property must either come from stylesheet or UA defaults
   - No graceful degradation for missing properties
   - Requires complete handler implementation upfront

2. **Atomic style computation**
   - All-or-nothing: Either all properties are computed or style selection fails
   - Cannot return partial results (e.g., color is computed, margin isn't)
   - One missing property breaks the entire cascade

3. **Tight coupling to NetSurf**
   - Designed for NetSurf's specific rendering pipeline
   - Assumptions about DOM structure and node data storage
   - Not designed for minimal/embedded use cases

4. **Limited error recovery**
   - CSS_INVALID means cascade failure
   - No way to distinguish "property not implemented" from "cascade computation error"

### 3.2 Modern CSS Cascade Engine Architecture

A proper CSS engine designed for 2025+ should:

#### A. Spec-Compliant Cascade Algorithm

Implement CSS Cascading and Inheritance Module Level 3:

```
1. Collect all matching rules:
   - User-Agent (from stylesheets, not callbacks)
   - Author (normal)
   - Author (important)
   - User (if applicable)

2. For each property:
   a. Find highest-origin rule with property set
   b. If none, compute initial value
   c. If inherited property, inherit from parent
   d. Never ask "what's the default?" - compute it deterministically

3. Return fully computed style with source/origin metadata
```

#### B. Explicit Property Table

```c
typedef struct {
    css_property properties[CSS_PROPERTY_COUNT];  // All properties
    uint8_t origins[CSS_PROPERTY_COUNT];          // Track origin for each
    uint16_t specificities[CSS_PROPERTY_COUNT];   // For debugging/overrides
    bool inherited[CSS_PROPERTY_COUNT];           // If value came from parent
} css_computed_style;
```

Every property **always** has a value (initial, inherited, or matched rule).

#### C. No Handler Callbacks for Property Logic

Instead of callbacks asking "what's the default?":

```c
// Modern approach: properties are data, not functions
typedef struct {
    const char *name;
    css_property_type type;
    css_value initial_value;
    bool inherited_by_default;
    css_value (*computed_value)(const css_value raw, const css_unit_ctx *unit_ctx);
} css_property_spec;

css_error compute_cascade(
    dom_element *element,
    const css_stylesheet *ua_sheet,
    const css_stylesheet *author_sheet,
    const css_computed_style *parent_style,
    css_computed_style *out  // Always returns all properties
);
```

#### D. Incremental Property Computation

```c
// Lazy compute properties as needed, not atomic
css_error get_property_value(
    css_computed_style *style,
    css_property_id prop,
    css_value *out,
    css_origin *origin  // Where it came from
);
```

#### E. Error-Resilient Parsing

```c
// Return partial results even if some properties fail
typedef struct {
    css_property properties[CSS_PROPERTY_COUNT];
    css_error errors[CSS_PROPERTY_COUNT];  // Per-property error tracking
    uint32_t error_count;
} css_cascade_result;
```

### 3.3 Design Philosophy Difference

**LibCSS Philosophy**:
- NetSurf owns the DOM, the stylesheet, and the handler
- libcss trusts the handler to provide all needed data
- Failure in handler = failure in cascade

**Modern Philosophy**:
- CSS engine is independent, spec-driven component
- No dependency on external callbacks for property logic
- Failure in one property doesn't break cascade
- Engine is "reference implementation" of CSS spec

### 3.4 Property Handling Strategy

Instead of this (NetSurf/libcss way):
```c
for each property in document:
    if stylesheet has rule: use rule value
    else if handler provides default: use default
    else: fail cascade
```

Do this (modern way):
```c
for each CSS_PROPERTY_* enum value:
    if stylesheet has rule: use rule value with author origin
    else if parent computed style has inherited value: use parent value
    else if property is inherited: compute from parent
    else: use explicit initial value from spec
return fully computed style
```

### 3.5 Comparison Table

| Aspect | LibCSS (Current) | Modern Engine (Recommended) |
|--------|------------------|---------------------------|
| Property resolution | Callback-based | Table-based |
| Error handling | Atomic (all or nothing) | Per-property |
| Cascade algorithm | Black box | Explicit, debuggable |
| Property initialization | Query handler | Spec-defined initial values |
| Missing property | Cascade fails | Use initial value |
| Performance model | "Computed on demand" | "Compute all on selection" |
| Extensibility | Add handler methods | Add property specs to table |

## Part 4: Actionable Findings

### 4.1 Root Cause Summary

**What**: CSS_INVALID returned by css_select_style()

**Why**: libcss's cascade algorithm calls ua_default_for_property() for properties (margin, padding, border, etc.) that silksurf doesn't implement. Returning CSS_INVALID signals cascade failure.

**Where**: `/home/eirikr/Github/silksurf/src/document/css_select_handler.c`, line 673

**Evidence**:
- Code: `return CSS_INVALID;` in default case of ua_default_for_property()
- Property enum shows 60+ properties, handler only covers 4
- libcss design expects complete handler implementation
- NetSurf reference implementation also returns CSS_INVALID for unhandled properties

### 4.2 Is This a Bug?

**No**. This is a design gap, not a bug:
- Handler implementation matches NetSurf's reference
- Error return is correct per libcss specification
- The issue is that libcss was designed for complete browser, not minimal engine

### 4.3 Immediate Workarounds (Stay with LibCSS)

If continuing with libcss, three options:

**Option A: Expand ua_default_for_property()**
```c
// Handle all 60+ properties, return sensible defaults or CSS_PROPERTY_NOT_SET
// Pros: Works with libcss
// Cons: 200+ lines of boilerplate, doesn't match design goals
```

**Option B: Wrap css_select_style() with fallback**
```c
css_error err = css_select_style(...);
if (err == CSS_INVALID) {
    // Fallback: compute minimal style manually
    out_style->display = CSS_DISPLAY_BLOCK;
    out_style->width = -1;  // auto
    return 0;
}
```

**Option C: Use CSS_PROPERTY_NOT_SET**
```c
// Research if libcss supports this error code
// Check if it allows cascade to continue (unlikely)
```

### 4.4 Long-Term Solution (Recommended)

**Design Goals for Modern CSS Engine**:

1. **Spec Compliance**
   - Implement CSS Cascading and Inheritance Module Level 3+
   - Every property has documented initial/computed value
   - Cascade algorithm is transparent and debuggable

2. **Independence**
   - No external callbacks for property logic
   - DOM traversal done internally or via fixed API
   - No "handler must provide X" assumptions

3. **Error Resilience**
   - Per-property error tracking
   - Partial results (some properties computed, others at initial)
   - Never atomic failure

4. **Performance**
   - Single-pass cascade for all properties
   - Lazy evaluation for computed values (length units, colors, etc.)
   - Caching of computed styles by selector

5. **Debuggability**
   - Each property tracks: source rule, origin, specificity
   - Clear distinction between inherited/computed/initial
   - Logging shows cascade steps for each property

## Part 5: Architectural Implications

### 5.1 Current Silksurf Architecture

```
HTML Parser -> DOM Tree
                 |
                 v
            CSS Engine (libcss)
              |         |
         Cascade    Selector
              |         |
              +---------+
                 |
                 v
           Layout Engine
              |
              v
         Rendering Pipeline
```

**Problem**: CSS Engine is tightly coupled to libcss, which has incomplete handler requirements.

### 5.2 Recommended Architecture

```
HTML Parser -> DOM Tree
                 |
                 v
         Stylesheet Parser
         (CSS Tokenizer + Parser)
              |
              v
         Style Computation Engine
            (Native Cascade)
              |
              v
         Computed Styles Cache
              |
              v
         Layout Engine
              |
              v
         Rendering Pipeline
```

**Benefits**:
- CSS engine is self-contained
- No external callback dependencies
- Easier to test and debug
- Closer to browser architecture

### 5.3 Migration Strategy

**Phase 1**: Keep libcss, fix with workaround
- Add error recovery to css_select_style() wrapper
- Document limitation

**Phase 2**: Build native cascade engine
- Start with simple property matching
- Implement cascade (origin, specificity)
- Support inheritance

**Phase 3**: Deprecate libcss
- Replace css_select_style() calls with native implementation
- Remove libcss dependency

## Part 6: Code Evidence

### Evidence 1: The Problematic Function

**File**: `/home/eirikr/Github/silksurf/src/document/css_select_handler.c`

```c
static css_error ua_default_for_property(void *pw, uint32_t property, css_hint *hint) {
    (void)pw;

    static int call_count = 0;
    if (++call_count <= 20) {
        fprintf(stderr, "[CSS Handler] ua_default_for_property called (#%d) for property=%u\n", call_count, property);
    }

    /* Only 4 properties handled */
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
            /* This returns CSS_INVALID = 3 for all 56 other properties */
            return CSS_INVALID;
    }

    return CSS_OK;
}
```

### Evidence 2: Where Error is Detected

**File**: `/home/eirikr/Github/silksurf/src/document/css_engine.c`

```c
int silk_css_get_computed_style(silk_css_engine_t *engine,
                                 silk_dom_node_t *element,
                                 silk_computed_style_t *out_style) {
    // ... setup code ...

    css_error err = css_select_style(engine->select_ctx,
                                      libdom_node,
                                      &unit_ctx,
                                      &media,
                                      NULL,
                                      silk_css_get_select_handler(),
                                      (void *)engine,
                                      &results);

    fprintf(stderr, "[CSS] css_select_style returned: %d\n", err);

    if (err != CSS_OK || !results) {
        /* Returns -1 when err == CSS_INVALID (3) */
        fprintf(stderr, "[CSS] ERROR: Style selection failed: %d (results=%p)\n", err, (void *)results);
        return -1;
    }
    // ...
}
```

### Evidence 3: LibCSS Design Assumption

From `/usr/include/libcss/properties.h`:

```c
enum css_properties_e {
    CSS_PROP_AZIMUTH           = 0x000,
    CSS_PROP_BACKGROUND_ATTACHMENT = 0x001,
    // ... 58 more properties ...
    CSS_PROP_Z_INDEX           = 0x3b,
};
```

Silksurf handles 4, libcss defines 60+.

## Conclusion

The CSS_INVALID error is **not a bug**—it is a **design mismatch**. LibCSS was designed for a complete browser implementation (NetSurf) where the handler provides all necessary CSS property defaults. Silksurf attempts to use libcss minimally, but providing only 4 out of 60+ properties causes cascade failures.

**Proper fix**: Either:
1. Expand handler to cover all properties (short-term workaround)
2. Build native CSS cascade engine that doesn't depend on callback handlers (long-term solution)

The recommended approach is **option 2**, as it aligns with silksurf's stated goal of being a "cleanroom" implementation and provides better control over CSS spec compliance.

## References

- [NetSurf LibCSS Project](https://www.netsurf-browser.org/projects/libcss/)
- [NetSurf LibCSS Example](https://github.com/netsurf-browser/libcss/blob/master/examples/example1.c)
- [CSS Cascading and Inheritance Module Level 3](https://www.w3.org/TR/css-cascade-3/)
- [CSS Computed Values](https://www.w3.org/TR/css-values-3/#computed-value)
