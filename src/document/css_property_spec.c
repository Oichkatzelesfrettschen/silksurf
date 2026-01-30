#include "css_cascade.h"
#include <stdio.h>
#include <string.h>

/* ============================================================================
 * Property Compute Functions
 * ============================================================================
 *
 * Each property has a compute function that:
 * 1. Takes the raw value from stylesheet
 * 2. Converts units if needed
 * 3. Returns the final computed value
 * 4. Never returns an error that breaks cascade (per-property error handling)
 */

/* Color: straightforward, no unit conversion */
static css_error compute_color(
    const css_property_value *raw,
    const css_cascade_context *ctx,
    css_property_value *computed
) {
    (void)ctx;  /* Unused */
    *computed = *raw;
    return CSS_OK;
}

static bool validate_color(const css_property_value *value) {
    (void)value;
    return true;
}

static void print_color(const css_property_value *value) {
    printf("color: #%08X", value->color);
}

/* Display: keyword property */
static css_error compute_display(
    const css_property_value *raw,
    const css_cascade_context *ctx,
    css_property_value *computed
) {
    (void)ctx;
    *computed = *raw;
    return CSS_OK;
}

static bool validate_display(const css_property_value *value) {
    return value->keyword <= CSS_DISPLAY_TABLE_CELL;
}

static void print_display(const css_property_value *value) {
    const char *names[] = {
        "block", "inline", "inline-block", "flex", "none", "table", "table-row", "table-cell"
    };
    if (value->keyword < 8) {
        printf("display: %s", names[value->keyword]);
    }
}

/* Font Size: handles em, rem, px, % */
static css_error compute_font_size(
    const css_property_value *raw,
    const css_cascade_context *ctx,
    css_property_value *computed
) {
    *computed = *raw;

    /* If unit is already px or absolute, pass through */
    if (raw->length.unit == CSS_UNIT_PX || raw->length.unit == CSS_UNIT_AUTO) {
        return CSS_OK;
    }

    /* em: relative to parent font-size */
    if (raw->length.unit == CSS_UNIT_EM) {
        if (ctx->parent_computed) {
            css_fixed parent_size = ctx->parent_computed->values[CSS_PROP_FONT_SIZE].length.value;
            computed->length.value = css_fixed_mul(raw->length.value, parent_size);
            computed->length.unit = CSS_UNIT_PX;
        } else {
            /* No parent: 1em = 16px */
            computed->length.value = css_fixed_mul(raw->length.value, INTTOFIX(16));
            computed->length.unit = CSS_UNIT_PX;
        }
        return CSS_OK;
    }

    /* percentage: relative to parent font-size */
    if (raw->length.unit == CSS_UNIT_PERCENT) {
        if (ctx->parent_computed) {
            css_fixed parent_size = ctx->parent_computed->values[CSS_PROP_FONT_SIZE].length.value;
            computed->length.value = css_fixed_mul(raw->length.value, parent_size);
            computed->length.unit = CSS_UNIT_PX;
        } else {
            /* No parent: 100% = 16px */
            computed->length.value = css_fixed_mul(raw->length.value, INTTOFIX(16));
            computed->length.unit = CSS_UNIT_PX;
        }
        return CSS_OK;
    }

    /* rem: relative to root font-size (not implemented in Phase 1) */
    if (raw->length.unit == CSS_UNIT_REM) {
        /* For now, treat as 16px base */
        computed->length.value = css_fixed_mul(raw->length.value, INTTOFIX(16));
        computed->length.unit = CSS_UNIT_PX;
        return CSS_OK;
    }

    return CSS_OK;
}

static bool validate_font_size(const css_property_value *value) {
    return value->length.value >= 0;
}

static void print_font_size(const css_property_value *value) {
    printf("font-size: %d", FIXTOINT(value->length.value));
    if (value->length.unit == CSS_UNIT_PX) printf("px");
}

/* Font Family: keyword property (sans-serif, serif, monospace) */
static css_error compute_font_family(
    const css_property_value *raw,
    const css_cascade_context *ctx,
    css_property_value *computed
) {
    (void)ctx;
    *computed = *raw;
    return CSS_OK;
}

static bool validate_font_family(const css_property_value *value) {
    (void)value;
    return true;
}

static void print_font_family(const css_property_value *value) {
    const char *names[] = { "serif", "sans-serif", "monospace" };
    if (value->keyword < 3) {
        printf("font-family: %s", names[value->keyword]);
    }
}

/* Margin: handles length, %, auto */
static css_error compute_margin(
    const css_property_value *raw,
    const css_cascade_context *ctx,
    css_property_value *computed
) {
    *computed = *raw;

    /* auto: layout engine will compute actual value */
    if (raw->length.unit == CSS_UNIT_AUTO) {
        return CSS_OK;
    }

    /* px: pass through */
    if (raw->length.unit == CSS_UNIT_PX) {
        return CSS_OK;
    }

    /* percentage: resolve against parent width */
    if (raw->length.unit == CSS_UNIT_PERCENT) {
        /* Leave as percentage for layout engine to compute */
        return CSS_OK;
    }

    /* em: relative to font-size */
    if (raw->length.unit == CSS_UNIT_EM) {
        if (ctx && ctx->parent_computed) {
            css_fixed font_size = ctx->parent_computed->values[CSS_PROP_FONT_SIZE].length.value;
            computed->length.value = css_fixed_mul(raw->length.value, font_size);
            computed->length.unit = CSS_UNIT_PX;
        }
        return CSS_OK;
    }

    return CSS_OK;
}

static bool validate_margin(const css_property_value *value) {
    (void)value;
    return true;  /* All values valid */
}

static void print_margin(const css_property_value *value) {
    if (value->length.unit == CSS_UNIT_AUTO) {
        printf("margin: auto");
    } else if (value->length.unit == CSS_UNIT_PERCENT) {
        printf("margin: %d%%", FIXTOINT(value->length.value));
    } else {
        printf("margin: %dpx", FIXTOINT(value->length.value));
    }
}

/* Padding: similar to margin but no auto */
static css_error compute_padding(
    const css_property_value *raw,
    const css_cascade_context *ctx,
    css_property_value *computed
) {
    /* Padding can't be auto or negative */
    if (raw->length.unit == CSS_UNIT_AUTO) {
        computed->length.value = 0;
        computed->length.unit = CSS_UNIT_PX;
        return CSS_OK;
    }

    *computed = *raw;

    /* Handle em units */
    if (raw->length.unit == CSS_UNIT_EM) {
        if (ctx && ctx->parent_computed) {
            css_fixed font_size = ctx->parent_computed->values[CSS_PROP_FONT_SIZE].length.value;
            computed->length.value = css_fixed_mul(raw->length.value, font_size);
            computed->length.unit = CSS_UNIT_PX;
        }
    }

    return CSS_OK;
}

static bool validate_padding(const css_property_value *value) {
    return value->length.value >= 0;
}

static void print_padding(const css_property_value *value) {
    printf("padding: %dpx", FIXTOINT(value->length.value));
}

/* Border Width */
static css_error compute_border_width(
    const css_property_value *raw,
    const css_cascade_context *ctx,
    css_property_value *computed
) {
    (void)ctx;
    *computed = *raw;
    return CSS_OK;
}

static bool validate_border_width(const css_property_value *value) {
    return value->length.value >= 0;
}

static void print_border_width(const css_property_value *value) {
    printf("border-width: %dpx", FIXTOINT(value->length.value));
}

/* Width */
static css_error compute_width(
    const css_property_value *raw,
    const css_cascade_context *ctx,
    css_property_value *computed
) {
    (void)ctx;
    *computed = *raw;
    return CSS_OK;
}

static bool validate_width(const css_property_value *value) {
    (void)value;
    return true;
}

static void print_width(const css_property_value *value) {
    if (value->length.unit == CSS_UNIT_AUTO) {
        printf("width: auto");
    } else {
        printf("width: %d%s", FIXTOINT(value->length.value),
               value->length.unit == CSS_UNIT_PX ? "px" : "%");
    }
}

/* Height */
static css_error compute_height(
    const css_property_value *raw,
    const css_cascade_context *ctx,
    css_property_value *computed
) {
    (void)ctx;
    *computed = *raw;
    return CSS_OK;
}

static bool validate_height(const css_property_value *value) {
    (void)value;
    return true;
}

static void print_height(const css_property_value *value) {
    if (value->length.unit == CSS_UNIT_AUTO) {
        printf("height: auto");
    } else {
        printf("height: %dpx", FIXTOINT(value->length.value));
    }
}

/* Background Color */
static css_error compute_background_color(
    const css_property_value *raw,
    const css_cascade_context *ctx,
    css_property_value *computed
) {
    (void)ctx;
    *computed = *raw;
    return CSS_OK;
}

static bool validate_background_color(const css_property_value *value) {
    (void)value;
    return true;
}

static void print_background_color(const css_property_value *value) {
    printf("background-color: #%08X", value->color);
}

/* Position */
static css_error compute_position(
    const css_property_value *raw,
    const css_cascade_context *ctx,
    css_property_value *computed
) {
    (void)ctx;
    *computed = *raw;
    return CSS_OK;
}

static bool validate_position(const css_property_value *value) {
    return value->keyword <= CSS_POSITION_FIXED;
}

static void print_position(const css_property_value *value) {
    const char *names[] = { "static", "absolute", "relative", "fixed" };
    if (value->keyword < 4) {
        printf("position: %s", names[value->keyword]);
    }
}

/* ============================================================================
 * Property Specification Table
 * ============================================================================
 *
 * Metadata for each CSS property: name, initial value, inheritance,
 * compute function, validation, debug output
 */

static const css_property_spec css_properties[CSS_PROPERTY_COUNT] = {
    /* COLOR: Inherited */
    [CSS_PROP_COLOR] = {
        .property_id = CSS_PROP_COLOR,
        .name = "color",
        .inherited = true,
        .initial_value = {.color = CSS_COLOR_BLACK},
        .compute = compute_color,
        .is_valid = validate_color,
        .debug_print = print_color,
    },

    /* DISPLAY: Not inherited */
    [CSS_PROP_DISPLAY] = {
        .property_id = CSS_PROP_DISPLAY,
        .name = "display",
        .inherited = false,
        .initial_value = {.keyword = CSS_DISPLAY_INLINE},
        .compute = compute_display,
        .is_valid = validate_display,
        .debug_print = print_display,
    },

    /* FONT_SIZE: Inherited */
    [CSS_PROP_FONT_SIZE] = {
        .property_id = CSS_PROP_FONT_SIZE,
        .name = "font-size",
        .inherited = true,
        .initial_value = {.length = {INTTOFIX(16), CSS_UNIT_PX}},
        .compute = compute_font_size,
        .is_valid = validate_font_size,
        .debug_print = print_font_size,
    },

    /* FONT_FAMILY: Inherited */
    [CSS_PROP_FONT_FAMILY] = {
        .property_id = CSS_PROP_FONT_FAMILY,
        .name = "font-family",
        .inherited = true,
        .initial_value = {.keyword = 1},  /* sans-serif */
        .compute = compute_font_family,
        .is_valid = validate_font_family,
        .debug_print = print_font_family,
    },

    /* MARGIN: Not inherited */
    [CSS_PROP_MARGIN_TOP] = {
        .property_id = CSS_PROP_MARGIN_TOP,
        .name = "margin-top",
        .inherited = false,
        .initial_value = {.length = {0, CSS_UNIT_PX}},
        .compute = compute_margin,
        .is_valid = validate_margin,
        .debug_print = print_margin,
    },

    [CSS_PROP_MARGIN_RIGHT] = {
        .property_id = CSS_PROP_MARGIN_RIGHT,
        .name = "margin-right",
        .inherited = false,
        .initial_value = {.length = {0, CSS_UNIT_PX}},
        .compute = compute_margin,
        .is_valid = validate_margin,
        .debug_print = print_margin,
    },

    [CSS_PROP_MARGIN_BOTTOM] = {
        .property_id = CSS_PROP_MARGIN_BOTTOM,
        .name = "margin-bottom",
        .inherited = false,
        .initial_value = {.length = {0, CSS_UNIT_PX}},
        .compute = compute_margin,
        .is_valid = validate_margin,
        .debug_print = print_margin,
    },

    [CSS_PROP_MARGIN_LEFT] = {
        .property_id = CSS_PROP_MARGIN_LEFT,
        .name = "margin-left",
        .inherited = false,
        .initial_value = {.length = {0, CSS_UNIT_PX}},
        .compute = compute_margin,
        .is_valid = validate_margin,
        .debug_print = print_margin,
    },

    /* PADDING: Not inherited */
    [CSS_PROP_PADDING_TOP] = {
        .property_id = CSS_PROP_PADDING_TOP,
        .name = "padding-top",
        .inherited = false,
        .initial_value = {.length = {0, CSS_UNIT_PX}},
        .compute = compute_padding,
        .is_valid = validate_padding,
        .debug_print = print_padding,
    },

    [CSS_PROP_PADDING_RIGHT] = {
        .property_id = CSS_PROP_PADDING_RIGHT,
        .name = "padding-right",
        .inherited = false,
        .initial_value = {.length = {0, CSS_UNIT_PX}},
        .compute = compute_padding,
        .is_valid = validate_padding,
        .debug_print = print_padding,
    },

    [CSS_PROP_PADDING_BOTTOM] = {
        .property_id = CSS_PROP_PADDING_BOTTOM,
        .name = "padding-bottom",
        .inherited = false,
        .initial_value = {.length = {0, CSS_UNIT_PX}},
        .compute = compute_padding,
        .is_valid = validate_padding,
        .debug_print = print_padding,
    },

    [CSS_PROP_PADDING_LEFT] = {
        .property_id = CSS_PROP_PADDING_LEFT,
        .name = "padding-left",
        .inherited = false,
        .initial_value = {.length = {0, CSS_UNIT_PX}},
        .compute = compute_padding,
        .is_valid = validate_padding,
        .debug_print = print_padding,
    },

    /* BORDER WIDTH: Not inherited */
    [CSS_PROP_BORDER_TOP_WIDTH] = {
        .property_id = CSS_PROP_BORDER_TOP_WIDTH,
        .name = "border-top-width",
        .inherited = false,
        .initial_value = {.length = {0, CSS_UNIT_PX}},
        .compute = compute_border_width,
        .is_valid = validate_border_width,
        .debug_print = print_border_width,
    },

    [CSS_PROP_BORDER_RIGHT_WIDTH] = {
        .property_id = CSS_PROP_BORDER_RIGHT_WIDTH,
        .name = "border-right-width",
        .inherited = false,
        .initial_value = {.length = {0, CSS_UNIT_PX}},
        .compute = compute_border_width,
        .is_valid = validate_border_width,
        .debug_print = print_border_width,
    },

    [CSS_PROP_BORDER_BOTTOM_WIDTH] = {
        .property_id = CSS_PROP_BORDER_BOTTOM_WIDTH,
        .name = "border-bottom-width",
        .inherited = false,
        .initial_value = {.length = {0, CSS_UNIT_PX}},
        .compute = compute_border_width,
        .is_valid = validate_border_width,
        .debug_print = print_border_width,
    },

    [CSS_PROP_BORDER_LEFT_WIDTH] = {
        .property_id = CSS_PROP_BORDER_LEFT_WIDTH,
        .name = "border-left-width",
        .inherited = false,
        .initial_value = {.length = {0, CSS_UNIT_PX}},
        .compute = compute_border_width,
        .is_valid = validate_border_width,
        .debug_print = print_border_width,
    },

    /* WIDTH: Not inherited */
    [CSS_PROP_WIDTH] = {
        .property_id = CSS_PROP_WIDTH,
        .name = "width",
        .inherited = false,
        .initial_value = {.length = {0, CSS_UNIT_AUTO}},
        .compute = compute_width,
        .is_valid = validate_width,
        .debug_print = print_width,
    },

    /* HEIGHT: Not inherited */
    [CSS_PROP_HEIGHT] = {
        .property_id = CSS_PROP_HEIGHT,
        .name = "height",
        .inherited = false,
        .initial_value = {.length = {0, CSS_UNIT_AUTO}},
        .compute = compute_height,
        .is_valid = validate_height,
        .debug_print = print_height,
    },

    /* BACKGROUND_COLOR: Not inherited */
    [CSS_PROP_BACKGROUND_COLOR] = {
        .property_id = CSS_PROP_BACKGROUND_COLOR,
        .name = "background-color",
        .inherited = false,
        .initial_value = {.color = CSS_COLOR_TRANSPARENT},
        .compute = compute_background_color,
        .is_valid = validate_background_color,
        .debug_print = print_background_color,
    },

    /* POSITION: Not inherited */
    [CSS_PROP_POSITION] = {
        .property_id = CSS_PROP_POSITION,
        .name = "position",
        .inherited = false,
        .initial_value = {.keyword = CSS_POSITION_STATIC},
        .compute = compute_position,
        .is_valid = validate_position,
        .debug_print = print_position,
    },

    /* Additional properties for later (slots 20-25) */
    /* These are placeholders - will be filled in Phase 2 */
};

/* ============================================================================
 * Public API
 * ============================================================================ */

const css_property_spec *css_get_property_spec(css_property_id prop_id) {
    if (prop_id >= CSS_PROPERTY_COUNT) {
        return NULL;
    }
    return &css_properties[prop_id];
}
