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
 * Cleanroom CSS cascade engine. Does NOT include <libcss/libcss.h>.
 * Defines its own compatible types for CSS values, units, colors.
 */

/* CSS fixed-point arithmetic (libcss compatible layout) */
typedef int32_t css_fixed;
#define CSS_RADIX_POINT 10
#define INTTOFIX(x) ((x) << CSS_RADIX_POINT)
#define FIXTOINT(x) ((x) >> CSS_RADIX_POINT)
#define css_fixed_mul(a, b) (((int64_t)(a) * (b)) >> CSS_RADIX_POINT)
#define css_fixed_div(a, b) (((int64_t)(a) << CSS_RADIX_POINT) / (b))

typedef uint32_t css_color;
#define CSS_COLOR_TRANSPARENT 0x00000000
#define CSS_COLOR_BLACK       0xFF000000
#define CSS_COLOR_WHITE       0xFFFFFFFF

typedef enum {
    CSS_UNIT_PX, CSS_UNIT_EM, CSS_UNIT_REM,
    CSS_UNIT_PERCENT, CSS_UNIT_AUTO, CSS_UNIT_INHERIT, CSS_UNIT_INITIAL,
} css_unit;

typedef union {
    struct { css_fixed value; css_unit unit; } length;
    struct { css_fixed value; } percentage;
    css_color color;
    uint32_t keyword;
    struct { const char *value; size_t length; } string;
    struct { void *items; uint32_t count; } list;
} css_property_value;

typedef enum { CSS_VALUE_SET, CSS_VALUE_INHERIT, CSS_VALUE_INITIAL, CSS_VALUE_UNSET } css_value_status;

typedef enum {
    CSS_ORIGIN_UA, CSS_ORIGIN_AUTHOR, CSS_ORIGIN_AUTHOR_IMPORTANT,
} css_origin;

typedef enum {
    CSS_OK = 0, CSS_NOMEM = 1, CSS_INVALID = 3, CSS_BADPARM = 4,
} css_error;

/* Property IDs */
typedef enum {
    CSS_PROP_COLOR = 0, CSS_PROP_DISPLAY, CSS_PROP_FONT_SIZE, CSS_PROP_FONT_FAMILY,
    CSS_PROP_MARGIN_TOP, CSS_PROP_MARGIN_RIGHT, CSS_PROP_MARGIN_BOTTOM, CSS_PROP_MARGIN_LEFT,
    CSS_PROP_PADDING_TOP, CSS_PROP_PADDING_RIGHT, CSS_PROP_PADDING_BOTTOM, CSS_PROP_PADDING_LEFT,
    CSS_PROP_BORDER_TOP_WIDTH, CSS_PROP_BORDER_RIGHT_WIDTH, CSS_PROP_BORDER_BOTTOM_WIDTH, CSS_PROP_BORDER_LEFT_WIDTH,
    CSS_PROP_WIDTH, CSS_PROP_HEIGHT, CSS_PROP_BACKGROUND_COLOR, CSS_PROP_POSITION,
    CSS_PROP_MAX_WIDTH, CSS_PROP_MAX_HEIGHT, CSS_PROP_MIN_WIDTH, CSS_PROP_MIN_HEIGHT,
    CSS_PROP_FONT_WEIGHT, CSS_PROP_TEXT_ALIGN,
    CSS_PROPERTY_COUNT = 26,
} css_property_id;

typedef struct {
    css_property_value values[CSS_PROPERTY_COUNT];
    uint32_t specificity_used;
    bool is_root;
} css_computed_style;

typedef struct css_cascade_context {
    dom_element *element;
    dom_element *parent;
    css_computed_style *parent_computed;
    struct css_rule *matched_rules;
    uint32_t matched_rule_count;
    uint16_t *specificities;
    css_origin *origins;
} css_cascade_context;

typedef struct css_property_spec {
    uint32_t property_id;
    const char *name;
    bool inherited;
    css_property_value initial_value;
    css_error (*compute)(const css_property_value *raw, const css_cascade_context *ctx, css_property_value *computed);
    bool (*is_valid)(const css_property_value *value);
    void (*debug_print)(const css_property_value *value);
} css_property_spec;

typedef struct css_rule {
    struct { uint32_t id; css_property_value value; } properties[CSS_PROPERTY_COUNT];
    uint32_t property_count;
} css_rule;

/* Public API */
css_error css_cascade_for_element(css_cascade_context *ctx, css_computed_style *out);
css_error css_compute_element_styles(
    dom_element *element, dom_element *parent,
    struct css_rule *matched_rules, uint32_t matched_count,
    uint16_t *specificities, css_origin *origins,
    css_computed_style *parent_computed, css_computed_style *out_computed
);
const css_property_spec *css_get_property_spec(css_property_id prop_id);
void css_convert_to_silk_style(const css_computed_style *computed, void *out_silk_style);

typedef enum { CSS_DISPLAY_BLOCK=0, CSS_DISPLAY_INLINE, CSS_DISPLAY_INLINE_BLOCK,
               CSS_DISPLAY_FLEX, CSS_DISPLAY_NONE, CSS_DISPLAY_TABLE,
               CSS_DISPLAY_TABLE_ROW, CSS_DISPLAY_TABLE_CELL } css_display_keyword;
typedef enum { CSS_POSITION_STATIC=0, CSS_POSITION_ABSOLUTE, CSS_POSITION_RELATIVE, CSS_POSITION_FIXED } css_position_keyword;
typedef enum { CSS_TEXT_ALIGN_LEFT=0, CSS_TEXT_ALIGN_CENTER, CSS_TEXT_ALIGN_RIGHT, CSS_TEXT_ALIGN_JUSTIFY } css_text_align_keyword;

/* Debug */
void css_debug_print_style(const css_computed_style *style);

#endif
