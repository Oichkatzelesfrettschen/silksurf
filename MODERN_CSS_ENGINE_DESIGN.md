# Modern CSS Cascade Engine Design for SilkSurf

**Author**: Claude Code
**Date**: 2026-01-29
**Status**: Specification (Post-LibCSS Migration)

## Overview

This document specifies a cleanroom CSS cascade engine designed for SilkSurf, free from LibCSS's callback-based architecture and handler dependencies. The engine implements CSS Cascading and Inheritance Module Level 3 with explicit property initialization and error resilience.

## Design Philosophy

**Core Principle**: A CSS engine should compute **all properties to their final values**, never fail cascade due to missing properties, and maintain complete transparency about where each property value came from (stylesheet rule, inheritance, or initial value).

**Non-Goals**:
- Support for CSS3+ advanced selectors (out of scope, selector matching is separate)
- Dynamic property updates (computed styles are immutable after cascade)
- CSS variables/custom properties (Phase 2)
- Media queries (out of scope, handled elsewhere)

## Part 1: Data Structures

### 1.1 Property Value Representation

```c
/* Every property has one of these value types */
typedef union {
    struct {
        css_fixed value;
        css_unit unit;
    } length;
    struct {
        css_fixed value;
    } percentage;
    css_color color;
    uint32_t keyword;          /* For keywords like 'block', 'auto', 'inherit' */
    struct {
        const char *value;
        size_t length;
    } string;
    struct {
        void *items;           /* Property-specific item list */
        uint32_t count;
    } list;
} css_property_value;

/* Status tracks computed type (for multi-valued properties like margin) */
typedef enum {
    CSS_VALUE_SET,             /* Property has computed value */
    CSS_VALUE_INHERIT,         /* Inherited from parent */
    CSS_VALUE_INITIAL,         /* Using initial value (not inherited) */
    CSS_VALUE_UNSET,           /* No value set (should not occur in final) */
} css_value_status;

/* Origin tracks where value came from */
typedef enum {
    CSS_ORIGIN_UA,             /* User-Agent stylesheet */
    CSS_ORIGIN_AUTHOR,         /* Author stylesheet */
    CSS_ORIGIN_AUTHOR_IMPORTANT, /* Author stylesheet with !important */
} css_origin;
```

### 1.2 Fully Computed Style

```c
/* All 60+ CSS properties in one structure */
typedef struct {
    /* Layout properties */
    struct {
        uint8_t display;       /* CSS_DISPLAY_BLOCK, INLINE, etc. */
        css_origin display_origin;
        uint8_t position;      /* CSS_POSITION_STATIC, ABSOLUTE, etc. */
        css_origin position_origin;
    } layout;

    /* Box model properties */
    struct {
        css_property_value margin_top, margin_right, margin_bottom, margin_left;
        css_property_value padding_top, padding_right, padding_bottom, padding_left;
        css_property_value border_top_width, border_right_width, etc;
        css_origin margin_origins[4];
        css_origin padding_origins[4];
        css_origin border_origins[4];
    } box;

    /* Text properties */
    struct {
        css_property_value color;
        css_property_value font_size;
        css_property_value font_family;
        uint8_t font_weight;
        uint8_t text_align;
        css_origin color_origin;
        css_origin font_origins[3];
        css_origin text_align_origin;
    } text;

    /* Background properties */
    struct {
        css_property_value background_color;
        css_property_value background_image;
        css_origin bg_color_origin;
        css_origin bg_image_origin;
    } background;

    /* ... other property groups ... */

    /* Metadata */
    uint32_t specificity_used;
    bool is_root;              /* Whether this is root element */
} css_computed_style;
```

### 1.3 Cascade Context

```c
/* Information needed for cascade algorithm */
typedef struct {
    dom_element *element;      /* Target element */
    dom_element *parent;       /* Parent element (for inheritance) */
    css_computed_style *parent_computed;  /* Parent's computed style */

    /* Stylesheet rules (pre-matched) */
    const css_rule *matched_rules;
    uint32_t matched_rule_count;
    uint16_t *specificities;   /* Parallel array: specificity for each rule */
    css_origin *origins;       /* Parallel array: origin for each rule */

    /* Unit context for relative units */
    css_unit_ctx *unit_ctx;
} css_cascade_context;
```

## Part 2: Property Specification Table

### 2.1 Property Metadata

Every CSS property needs explicit metadata:

```c
/* Specification for one CSS property */
typedef struct {
    uint32_t property_id;      /* CSS_PROP_MARGIN_TOP, etc. */
    const char *name;          /* "margin-top" */

    /* Spec-defined attributes */
    bool inherited;            /* Is this property inherited? */
    css_property_value initial_value;  /* Initial value from CSS spec */

    /* Computation function */
    css_error (*compute)(
        const css_property_value *raw,     /* Value from stylesheet */
        const css_cascade_context *ctx,
        css_property_value *computed       /* Output computed value */
    );

    /* Validation function */
    bool (*is_valid)(const css_property_value *value);

    /* Display function for debugging */
    void (*debug_print)(const css_property_value *value);
} css_property_spec;

/* Global property table */
extern css_property_spec css_properties[CSS_PROPERTY_COUNT];
```

### 2.2 Property Specifications (Examples)

```c
/* color: [ <color> | inherit | initial ] */
css_property_spec css_properties[CSS_PROP_COLOR] = {
    .property_id = CSS_PROP_COLOR,
    .name = "color",
    .inherited = true,         /* Inherited property */
    .initial_value = {.color = 0xFF000000},  /* black */
    .compute = compute_color,
    .is_valid = validate_color,
    .debug_print = print_color,
};

/* margin-top: [ <length> | <percentage> | auto | inherit | initial ] */
css_property_spec css_properties[CSS_PROP_MARGIN_TOP] = {
    .property_id = CSS_PROP_MARGIN_TOP,
    .name = "margin-top",
    .inherited = false,        /* Not inherited */
    .initial_value = {.length = {0, CSS_UNIT_PX}},
    .compute = compute_margin,
    .is_valid = validate_margin,
    .debug_print = print_margin,
};

/* display: [ block | inline | none | ... | inherit | initial ] */
css_property_spec css_properties[CSS_PROP_DISPLAY] = {
    .property_id = CSS_PROP_DISPLAY,
    .name = "display",
    .inherited = false,
    .initial_value = {.keyword = CSS_DISPLAY_INLINE},
    .compute = compute_display,
    .is_valid = validate_display,
    .debug_print = print_display,
};
```

## Part 3: Cascade Algorithm

### 3.1 High-Level Algorithm

```c
css_error css_cascade_for_element(
    css_cascade_context *ctx,
    css_computed_style *out
)
{
    /* Initialize with all initial values */
    for (uint32_t i = 0; i < CSS_PROPERTY_COUNT; i++) {
        out->values[i] = css_properties[i].initial_value;
        out->origins[i] = CSS_ORIGIN_UA;
    }

    /* Apply matching rules in cascade order:
       1. User-Agent stylesheets (lowest specificity)
       2. Author normal rules
       3. Author !important rules (highest specificity)
    */
    for (uint32_t rule_idx = 0; rule_idx < ctx->matched_rule_count; rule_idx++) {
        const css_rule *rule = &ctx->matched_rules[rule_idx];
        uint16_t specificity = ctx->specificities[rule_idx];
        css_origin origin = ctx->origins[rule_idx];

        /* For each property declared in this rule */
        for (uint32_t prop_idx = 0; prop_idx < rule->property_count; prop_idx++) {
            uint32_t prop_id = rule->properties[prop_idx].id;
            css_property_value value = rule->properties[prop_idx].value;

            /* Apply cascade: origin + specificity determines if we override */
            if (should_override(origin, specificity, out->origins[prop_id])) {
                out->values[prop_id] = value;
                out->origins[prop_id] = origin;
            }
        }
    }

    /* Apply inheritance for inherited properties */
    if (ctx->parent_computed) {
        for (uint32_t i = 0; i < CSS_PROPERTY_COUNT; i++) {
            if (css_properties[i].inherited && out->origins[i] == CSS_ORIGIN_UA) {
                /* No rule matched, inherit from parent */
                out->values[i] = ctx->parent_computed->values[i];
            }
        }
    }

    /* Compute final values (unit conversion, keyword resolution, etc.) */
    for (uint32_t i = 0; i < CSS_PROPERTY_COUNT; i++) {
        css_error err = css_properties[i].compute(&out->values[i], ctx, &out->values[i]);
        if (err != CSS_OK) {
            /* Per-property error handling - log but don't fail */
            fprintf(stderr, "[CSS] Warning: Failed to compute %s: %d\n",
                    css_properties[i].name, err);
        }
    }

    return CSS_OK;  /* Always returns CSS_OK, partial results are fine */
}
```

### 3.2 Cascade Decision Logic

```c
/* Determine if rule should override current value */
static bool should_override(
    css_origin new_origin,
    uint16_t new_specificity,
    css_origin old_origin
)
{
    /* Origin order: UA < Author < Important Author */
    static const int origin_rank[] = {
        [CSS_ORIGIN_UA] = 0,
        [CSS_ORIGIN_AUTHOR] = 1,
        [CSS_ORIGIN_AUTHOR_IMPORTANT] = 2,
    };

    int new_rank = origin_rank[new_origin];
    int old_rank = origin_rank[old_origin];

    /* Higher origin always wins */
    if (new_rank > old_rank) return true;
    if (new_rank < old_rank) return false;

    /* Same origin: higher specificity wins */
    /* (specificity stored with rule for comparison) */
    return true;  /* Simple case: last rule with same origin wins */
}
```

### 3.3 Error Resilience

Key design decision: **Never fail cascade due to missing property**.

```c
/* Per-property result tracking */
typedef struct {
    css_property_value value;
    css_error computation_status;  /* CSS_OK or error from compute() */
    css_origin origin;
    uint16_t specificity;
} css_cascade_property_result;

typedef struct {
    css_cascade_property_result properties[CSS_PROPERTY_COUNT];
    uint32_t error_count;       /* Number of properties with errors */
    uint32_t warning_count;
} css_cascade_full_result;

/* Return detailed results instead of atomic success/failure */
css_error css_cascade_for_element_detailed(
    css_cascade_context *ctx,
    css_cascade_full_result *out
)
{
    /* Cascade algorithm proceeds regardless of per-property errors */

    for (uint32_t i = 0; i < CSS_PROPERTY_COUNT; i++) {
        out->properties[i].computation_status =
            css_properties[i].compute(&value, ctx, &out->properties[i].value);

        if (out->properties[i].computation_status != CSS_OK) {
            out->error_count++;
        }
    }

    return CSS_OK;  /* Overall result is success even if some properties failed */
}
```

## Part 4: Integration with DOM

### 4.1 Style Computation API

```c
/* Main public API */
css_error css_compute_element_styles(
    dom_element *element,
    dom_element *parent,
    const css_stylesheet *ua_sheet,
    const css_stylesheet *author_sheet,
    const css_unit_ctx *unit_ctx,
    css_computed_style *parent_computed,
    css_computed_style *out_computed
)
{
    /* Step 1: Match element against stylesheets */
    css_rule *matched_rules = NULL;
    uint32_t matched_count = 0;
    uint16_t *specificities = NULL;
    css_origin *origins = NULL;

    css_match_rules(element, ua_sheet, author_sheet,
                    &matched_rules, &matched_count,
                    &specificities, &origins);

    /* Step 2: Build cascade context */
    css_cascade_context ctx = {
        .element = element,
        .parent = parent,
        .parent_computed = parent_computed,
        .matched_rules = matched_rules,
        .matched_rule_count = matched_count,
        .specificities = specificities,
        .origins = origins,
        .unit_ctx = unit_ctx,
    };

    /* Step 3: Run cascade algorithm */
    css_error err = css_cascade_for_element(&ctx, out_computed);

    /* Cleanup */
    free(matched_rules);
    free(specificities);
    free(origins);

    return err;  /* Always CSS_OK unless malloc fails */
}
```

### 4.2 Cached Style Computation

```c
/* Cache styles to avoid recomputation */
typedef struct {
    hash_table *style_cache;  /* key: (element, parent_style) -> computed_style */
    size_t max_entries;
} css_style_cache;

css_error css_get_cached_style(
    dom_element *element,
    css_computed_style *parent_computed,
    css_style_cache *cache,
    css_computed_style *out
)
{
    /* Hash key: element pointer + parent hash */
    uint64_t key = hash_combine(
        (uintptr_t)element,
        hash_style(parent_computed)
    );

    if (hash_table_get(cache->style_cache, key, out)) {
        return CSS_OK;  /* Cache hit */
    }

    /* Cache miss: compute and store */
    css_error err = css_compute_element_styles(...);
    if (err == CSS_OK) {
        hash_table_set(cache->style_cache, key, out);
    }

    return err;
}
```

## Part 5: Selector Matching

### 5.1 Selector Matching API

Note: Selector matching is separate from cascade. This engine assumes pre-matched rules.

```c
/* Separate selector matching module provides matched rules */
typedef struct {
    uint32_t rule_index;
    uint16_t specificity;
    css_origin origin;
    bool matches;
} css_match_result;

css_error css_match_selectors(
    dom_element *element,
    const css_stylesheet *sheet,
    css_match_result **out_matches,
    uint32_t *out_count
)
{
    /* Implementation: walk stylesheet rules, test each selector */
    /* Return list of matching rules with specificities */
}
```

### 5.2 Why Separation Matters

**Benefits of separating selector matching from cascade**:
1. Selector matching can be optimized independently (indexing, caching)
2. Cascade algorithm is pure data transformation (no selector logic)
3. Easier to test: mock selector results, test cascade in isolation
4. Easier to replace: can use fast selector engine later

## Part 6: Computation Functions

### 6.1 Example: Compute Color

```c
static css_error compute_color(
    const css_property_value *raw,
    const css_cascade_context *ctx,
    css_property_value *computed
)
{
    /* Color is straightforward: no unit conversion needed */
    *computed = *raw;

    if (!validate_color(computed)) {
        return CSS_BADPARM;
    }

    return CSS_OK;
}
```

### 6.2 Example: Compute Margin (Length or Auto)

```c
static css_error compute_margin(
    const css_property_value *raw,
    const css_cascade_context *ctx,
    css_property_value *computed
)
{
    /* Margin can be:
       - <length>: absolute or relative units
       - <percentage>: relative to parent width
       - auto: computed at layout time
    */

    if (raw->length.unit == CSS_UNIT_AUTO) {
        /* auto is computed value; layout engine replaces with real value */
        computed->length.value = 0;
        computed->length.unit = CSS_UNIT_AUTO;
        return CSS_OK;
    }

    if (raw->length.unit == CSS_UNIT_PERCENTAGE) {
        /* Percentage: resolve against parent width */
        if (!ctx->parent_computed) {
            /* No parent: treat as 0 */
            computed->length.value = 0;
            computed->length.unit = CSS_UNIT_PX;
            return CSS_OK;
        }

        /* Could leave as percentage for layout engine, or compute here */
        /* For now: store as percentage, layout engine handles */
        *computed = *raw;
        return CSS_OK;
    }

    if (raw->length.unit == CSS_UNIT_EM) {
        /* Relative to font-size */
        css_fixed font_size = ctx->parent_computed->text.font_size.length.value;
        computed->length.value = css_fixed_mul(raw->length.value, font_size);
        computed->length.unit = CSS_UNIT_PX;
        return CSS_OK;
    }

    /* Absolute units: pass through */
    *computed = *raw;
    return CSS_OK;
}
```

### 6.3 Example: Compute Display

```c
static css_error compute_display(
    const css_property_value *raw,
    const css_cascade_context *ctx,
    css_property_value *computed
)
{
    uint8_t display = raw->keyword;

    /* display value is mostly computed as-is, with some exceptions:
       - display: inline <internal-display> -> compute based on parent
       - Blockification rules (display: inline inside display: flex -> compute as block)
    */

    *computed = *raw;

    /* Could add blockification logic here if needed */

    return CSS_OK;
}
```

## Part 7: Integration Points

### 7.1 With HTML Parser

```c
/* After element is created in DOM */
dom_element *elem = dom_create_element("div");

/* Apply styles immediately (or defer to layout phase) */
css_computed_style style;
css_error err = css_compute_element_styles(
    elem,
    parent_element,
    ua_stylesheet,
    author_stylesheet,
    &unit_ctx,
    parent_style,
    &style
);

if (err != CSS_OK) {
    fprintf(stderr, "[CSS] Warning: Style computation for element failed\n");
    /* Still have default values from initial values */
}

/* Store computed style on element (or layout tree node) */
dom_set_element_style(elem, &style);
```

### 7.2 With Layout Engine

```c
/* Layout engine calls this to get style for positioning */
const css_computed_style *style = dom_get_element_style(element);

/* Use style values for layout decisions */
int width = (style->layout.width.length.unit == CSS_UNIT_PX)
    ? style->layout.width.length.value
    : -1;  /* auto: layout engine computes */

int height = (style->layout.height.length.unit == CSS_UNIT_PX)
    ? style->layout.height.length.value
    : -1;  /* auto */

int margin_top = style->box.margin_top.length.value;
/* etc. */
```

### 7.3 With Rendering

```c
/* Rendering engine queries computed styles */
const css_computed_style *style = dom_get_element_style(element);

/* Text rendering */
render_text(text, style->text.color.color, style->text.font_size.length.value);

/* Background rendering */
if (style->background.background_color.color != TRANSPARENT) {
    render_background_rect(rect, style->background.background_color.color);
}
```

## Part 8: Testing Strategy

### 8.1 Unit Tests

```c
/* Test cascade algorithm in isolation */

void test_cascade_origin_override() {
    css_cascade_context ctx = {0};

    /* UA rule sets color to black */
    ctx.matched_rules[0] = (css_rule){
        .properties[0] = {.id = CSS_PROP_COLOR, .value = {.color = BLACK}},
        .property_count = 1,
    };
    ctx.origins[0] = CSS_ORIGIN_UA;

    /* Author rule sets color to red */
    ctx.matched_rules[1] = (css_rule){
        .properties[0] = {.id = CSS_PROP_COLOR, .value = {.color = RED}},
        .property_count = 1,
    };
    ctx.origins[1] = CSS_ORIGIN_AUTHOR;

    ctx.matched_rule_count = 2;
    ctx.matched_count = 2;

    css_computed_style result = {0};
    css_cascade_for_element(&ctx, &result);

    /* Author overrides UA: should be RED */
    assert(result.values[CSS_PROP_COLOR].color == RED);
}

void test_inheritance() {
    css_computed_style parent = {0};
    parent.values[CSS_PROP_COLOR].color = BLUE;

    css_cascade_context ctx = {
        .parent_computed = &parent,
        .matched_rule_count = 0,  /* No matching rules */
    };

    css_computed_style result = {0};
    css_cascade_for_element(&ctx, &result);

    /* color is inherited, should take parent's value */
    assert(result.values[CSS_PROP_COLOR].color == BLUE);
}
```

### 8.2 Integration Tests

```c
/* Test against real stylesheets */

void test_margin_padding_cascade() {
    /* Parse stylesheet: "div { margin: 10px; padding: 5px; }" */
    css_stylesheet *sheet = parse_css("div { margin: 10px; padding: 5px; }");

    /* Create DOM element */
    dom_element *div = dom_create_element("div");

    /* Compute styles */
    css_computed_style style = {0};
    css_compute_element_styles(
        div, NULL, ua_sheet, sheet, &unit_ctx, NULL, &style
    );

    /* Verify computed values */
    assert(style.box.margin_top.length.value == 10);
    assert(style.box.margin_top.length.unit == CSS_UNIT_PX);
    assert(style.box.padding_top.length.value == 5);
    assert(style.box.padding_top.length.unit == CSS_UNIT_PX);
}
```

## Part 9: Implementation Roadmap

**Phase 1: Core Infrastructure**
- [ ] Define css_property_spec table for 20 core properties
- [ ] Implement css_cascade_for_element() with basic algorithm
- [ ] Basic compute functions (color, display, font-size)

**Phase 2: Property Coverage**
- [ ] Add remaining properties to spec table
- [ ] Implement compute functions for each property
- [ ] Unit conversion (em, rem, %, etc.)

**Phase 3: Optimization**
- [ ] Implement style caching
- [ ] Selector matching optimization
- [ ] Performance profiling

**Phase 4: Advanced Features**
- [ ] CSS variables (custom properties)
- [ ] Media query support
- [ ] Animation/transition hooks

## Conclusion

This design provides a clean separation of concerns:

1. **Selector Matching** (separate module) -> provides matched rules
2. **Cascade Algorithm** (this engine) -> applies rules and inheritance
3. **Computation** (per-property functions) -> converts to final values
4. **Layout** (separate engine) -> uses computed styles for positioning
5. **Rendering** (separate engine) -> uses styles for visual output

Each component can be tested, optimized, and replaced independently without affecting others. The cascade algorithm itself is transparent, deterministic, and spec-compliant.

**Key Advantages**:
- No external callbacks for property logic
- Per-property error handling (one property failure doesn't break entire cascade)
- Explicit property initialization (no "handler didn't provide value" errors)
- Spec-based property definitions (self-documenting)
- Full cascade transparency (trace how any property computed)
