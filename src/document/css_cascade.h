#ifndef SILKSURF_CSS_CASCADE_H
#define SILKSURF_CSS_CASCADE_H

#include <stdint.h>
#include <stdbool.h>
#include <stddef.h>
#include "dom/dom.h"

/* ============================================================================
 * CSS CASCADE ENGINE - Native Implementation for SilkSurf
 * ============================================================================
 *
 * This module provides a cleanroom CSS cascade engine that:
 * - Computes all CSS properties to their final values
 * - Never fails cascade due to missing properties (per-property error handling)
 * - Maintains transparency about property value origins
 * - Implements CSS Cascading and Inheritance Module Level 3
 *
 * Design Philosophy:
 * - Cascade is a pure data transformation (no external callbacks)
 * - Every property has explicit initial value from CSS spec
 * - Error handling is per-property (one failure doesn't break entire cascade)
 * - Full transparency: trace where each property value came from
 *
 * See MODERN_CSS_ENGINE_DESIGN.md for complete specification
 */

/* ============================================================================
 * Part 1: CSS Value Types
 * ============================================================================ */

/* CSS fixed-point arithmetic (libcss compatible) */
typedef int32_t css_fixed;
#define CSS_RADIX_POINT 10
#define INTTOFIX(x) ((x) << CSS_RADIX_POINT)
#define FIXTOINT(x) ((x) >> CSS_RADIX_POINT)
#define css_fixed_mul(a, b) (((int64_t)(a) * (b)) >> CSS_RADIX_POINT)
#define css_fixed_div(a, b) (((int64_t)(a) << CSS_RADIX_POINT) / (b))

/* CSS color: ARGB format (0xAARRGGBB) */
typedef uint32_t css_color;
#define CSS_COLOR_TRANSPARENT 0x00000000
#define CSS_COLOR_BLACK       0xFF000000
#define CSS_COLOR_WHITE       0xFFFFFFFF

/* CSS units */
typedef enum {
    CSS_UNIT_PX,          /* Pixels */
    CSS_UNIT_EM,          /* Relative to font-size */
    CSS_UNIT_REM,         /* Relative to root font-size */
    CSS_UNIT_PERCENT,     /* Percentage */
    CSS_UNIT_AUTO,        /* Auto (computed at layout time) */
    CSS_UNIT_INHERIT,     /* Inherit from parent */
    CSS_UNIT_INITIAL,     /* Use initial value */
} css_unit;

/* Property value representation - all values fit in this union */
typedef union {
    struct {
        css_fixed value;
        css_unit unit;
    } length;

    struct {
        css_fixed value;
    } percentage;

    css_color color;

    uint32_t keyword;     /* For keywords: display, position, text-align, etc. */

    struct {
        const char *value;
        size_t length;
    } string;

    struct {
        void *items;
        uint32_t count;
    } list;
} css_property_value;

/* Status of a computed property value */
typedef enum {
    CSS_VALUE_SET,        /* Property has computed value from stylesheet */
    CSS_VALUE_INHERIT,    /* Inherited from parent */
    CSS_VALUE_INITIAL,    /* Using initial value from spec */
    CSS_VALUE_UNSET,      /* No value set (should not occur in final) */
} css_value_status;

/* Origin of property value (for cascade ordering) */
typedef enum {
    CSS_ORIGIN_UA,                /* User-Agent stylesheet (lowest priority) */
    CSS_ORIGIN_AUTHOR,            /* Author stylesheet (normal) */
    CSS_ORIGIN_AUTHOR_IMPORTANT,  /* Author !important (highest priority) */
} css_origin;

/* CSS Error codes */
typedef enum {
    CSS_OK = 0,
    CSS_NOMEM = 1,
    CSS_INVALID = 3,
    CSS_BADPARM = 4,
} css_error;

/* ============================================================================
 * Part 2a: CSS Property IDs (must come before css_computed_style)
 * ============================================================================ */

typedef enum {
    CSS_PROP_COLOR = 0,
    CSS_PROP_DISPLAY = 1,
    CSS_PROP_FONT_SIZE = 2,
    CSS_PROP_FONT_FAMILY = 3,
    CSS_PROP_MARGIN_TOP = 4,
    CSS_PROP_MARGIN_RIGHT = 5,
    CSS_PROP_MARGIN_BOTTOM = 6,
    CSS_PROP_MARGIN_LEFT = 7,
    CSS_PROP_PADDING_TOP = 8,
    CSS_PROP_PADDING_RIGHT = 9,
    CSS_PROP_PADDING_BOTTOM = 10,
    CSS_PROP_PADDING_LEFT = 11,
    CSS_PROP_BORDER_TOP_WIDTH = 12,
    CSS_PROP_BORDER_RIGHT_WIDTH = 13,
    CSS_PROP_BORDER_BOTTOM_WIDTH = 14,
    CSS_PROP_BORDER_LEFT_WIDTH = 15,
    CSS_PROP_WIDTH = 16,
    CSS_PROP_HEIGHT = 17,
    CSS_PROP_BACKGROUND_COLOR = 18,
    CSS_PROP_POSITION = 19,

    /* Extended properties for later phases */
    CSS_PROP_MAX_WIDTH = 20,
    CSS_PROP_MAX_HEIGHT = 21,
    CSS_PROP_MIN_WIDTH = 22,
    CSS_PROP_MIN_HEIGHT = 23,
    CSS_PROP_FONT_WEIGHT = 24,
    CSS_PROP_TEXT_ALIGN = 25,

    CSS_PROPERTY_COUNT = 26,  /* Total properties implemented */
} css_property_id;

/* ============================================================================
 * Part 2b: CSS Computed Style Structure
 * ============================================================================
 *
 * All computed CSS properties for an element in one structure.
 * Uses flat array for easy indexed access in cascade algorithm.
 */

typedef struct {
    /* Flat array of property values (indexed by css_property_id) */
    css_property_value values[CSS_PROPERTY_COUNT];

    /* Metadata */
    uint32_t specificity_used;    /* Specificity of rule that won cascade */
    bool is_root;                 /* Whether this is root element */

} css_computed_style;

/* ============================================================================
 * Part 3: Cascade Context (before Property Specification, needed by function pointers)
 * ============================================================================
 *
 * Information needed to run cascade algorithm for one element
 */

typedef struct css_cascade_context {
    dom_element *element;
    dom_element *parent;
    css_computed_style *parent_computed;

    /* Pre-matched rules (from selector matching phase) */
    struct css_rule *matched_rules;
    uint32_t matched_rule_count;
    uint16_t *specificities;
    css_origin *origins;

} css_cascade_context;

/* ============================================================================
 * Part 2c: Property Specification
 * ============================================================================
 *
 * Metadata for each CSS property: initial value, inheritance, compute function
 */

typedef struct css_property_spec {
    uint32_t property_id;
    const char *name;

    bool inherited;
    css_property_value initial_value;

    /* Compute function: convert stylesheet value to final value */
    css_error (*compute)(
        const css_property_value *raw,
        const css_cascade_context *ctx,
        css_property_value *computed
    );

    /* Validation function */
    bool (*is_valid)(const css_property_value *value);

    /* Debug output */
    void (*debug_print)(const css_property_value *value);

} css_property_spec;

/* ============================================================================
 * Part 5: CSS Rules (from stylesheet after parsing)
 * ============================================================================ */

typedef struct css_rule {
    struct {
        uint32_t id;
        css_property_value value;
    } properties[CSS_PROPERTY_COUNT];

    uint32_t property_count;
} css_rule;

/* ============================================================================
 * Part 6: Public API
 * ============================================================================ */

/* Main cascade algorithm */
css_error css_cascade_for_element(
    css_cascade_context *ctx,
    css_computed_style *out
);

/* Compute styles for element with pre-matched rules */
css_error css_compute_element_styles(
    dom_element *element,
    dom_element *parent,
    struct css_rule *matched_rules,
    uint32_t matched_count,
    uint16_t *specificities,
    css_origin *origins,
    css_computed_style *parent_computed,
    css_computed_style *out_computed
);

/* Get the property specification table */
const css_property_spec *css_get_property_spec(css_property_id prop_id);

/* ============================================================================
 * Part 6b: Conversion to Public API
 * ============================================================================ */

/* Forward declaration: silk_computed_style_t is an opaque type defined in css_parser.h
 * This function converts native css_computed_style to the public API format */
void css_convert_to_silk_style(
    const css_computed_style *computed,
    void *out_silk_style  /* Actually silk_computed_style_t * - cast needed */
);

/* ============================================================================
 * Part 7: Display Keywords (from CSS spec)
 * ============================================================================ */

typedef enum {
    CSS_DISPLAY_BLOCK = 0,
    CSS_DISPLAY_INLINE = 1,
    CSS_DISPLAY_INLINE_BLOCK = 2,
    CSS_DISPLAY_FLEX = 3,
    CSS_DISPLAY_NONE = 4,
    CSS_DISPLAY_TABLE = 5,
    CSS_DISPLAY_TABLE_ROW = 6,
    CSS_DISPLAY_TABLE_CELL = 7,
} css_display_keyword;

typedef enum {
    CSS_POSITION_STATIC = 0,
    CSS_POSITION_ABSOLUTE = 1,
    CSS_POSITION_RELATIVE = 2,
    CSS_POSITION_FIXED = 3,
} css_position_keyword;

typedef enum {
    CSS_TEXT_ALIGN_LEFT = 0,
    CSS_TEXT_ALIGN_CENTER = 1,
    CSS_TEXT_ALIGN_RIGHT = 2,
    CSS_TEXT_ALIGN_JUSTIFY = 3,
} css_text_align_keyword;

#endif /* SILKSURF_CSS_CASCADE_H */
