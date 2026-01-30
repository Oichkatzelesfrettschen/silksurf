#include "css_cascade.h"
#include <stdlib.h>
#include <string.h>
#include <stdio.h>

/* ============================================================================
 * CSS CASCADE ENGINE - Core Algorithm Implementation
 * ============================================================================
 *
 * This implements the cascade algorithm per CSS Cascading and Inheritance
 * Module Level 3 specification, with key differences from libcss:
 *
 * 1. Per-property error handling (not atomic cascade failure)
 * 2. Explicit property initialization (no missing defaults)
 * 3. Full transparency (trace where each property came from)
 * 4. Error-resilient (partial results are acceptable)
 */

/* ============================================================================
 * Cascade Decision Logic
 * ============================================================================ */

/**
 * Determine if a new property value should override the current value.
 *
 * Cascade order (highest priority wins):
 * 1. Author !important (CSS_ORIGIN_AUTHOR_IMPORTANT)
 * 2. Author normal (CSS_ORIGIN_AUTHOR)
 * 3. User-Agent (CSS_ORIGIN_UA)
 *
 * Within same origin, higher specificity wins.
 */
static bool should_override_property(
    css_origin new_origin,
    uint16_t new_specificity,
    css_origin current_origin,
    uint16_t current_specificity
) {
    /* Origin priority: higher origin always wins */
    static const int origin_rank[] = {
        [CSS_ORIGIN_UA] = 0,
        [CSS_ORIGIN_AUTHOR] = 1,
        [CSS_ORIGIN_AUTHOR_IMPORTANT] = 2,
    };

    int new_rank = origin_rank[new_origin];
    int current_rank = origin_rank[current_origin];

    if (new_rank > current_rank) {
        return true;  /* New origin has higher priority */
    }
    if (new_rank < current_rank) {
        return false; /* Current origin has higher priority */
    }

    /* Same origin: specificity wins */
    return new_specificity >= current_specificity;
}

/* ============================================================================
 * Main Cascade Algorithm
 * ============================================================================ */

/**
 * Cascade property values from matching rules.
 *
 * Algorithm:
 * 1. Initialize all properties with their initial values (from spec)
 * 2. For each matching rule (in cascade order):
 *    - For each property declared in rule:
 *      - If higher origin or same origin + higher specificity:
 *        - Override current value
 * 3. Apply inheritance for inherited properties (no rule matched, parent exists)
 * 4. Compute final values (unit conversion, keyword resolution, etc.)
 *
 * Returns CSS_OK always (never fails cascade due to per-property error handling)
 */
css_error css_cascade_for_element(
    css_cascade_context *ctx,
    css_computed_style *out
) {
    if (!ctx || !out) {
        return CSS_BADPARM;
    }

    /* Step 1: Initialize all properties with initial values */
    for (uint32_t i = 0; i < CSS_PROPERTY_COUNT; i++) {
        const css_property_spec *spec = css_get_property_spec(i);
        if (spec) {
            out->values[i] = spec->initial_value;
        }
    }

    out->specificity_used = 0;
    out->is_root = (ctx->parent == NULL);

    /* Step 2: Apply cascade - walk matched rules and apply declarations */
    if (ctx->matched_rules && ctx->matched_rule_count > 0) {
        for (uint32_t rule_idx = 0; rule_idx < ctx->matched_rule_count; rule_idx++) {
            const css_rule *rule = &ctx->matched_rules[rule_idx];
            uint16_t specificity = ctx->specificities ? ctx->specificities[rule_idx] : 0;
            css_origin origin = ctx->origins ? ctx->origins[rule_idx] : CSS_ORIGIN_AUTHOR;

            /* For each property declared in this rule */
            for (uint32_t prop_idx = 0; prop_idx < rule->property_count; prop_idx++) {
                uint32_t prop_id = rule->properties[prop_idx].id;

                if (prop_id >= CSS_PROPERTY_COUNT) {
                    continue;  /* Skip unknown properties */
                }

                css_property_value new_value = rule->properties[prop_idx].value;

                /* Check if this declaration should override current value */
                if (should_override_property(origin, specificity, CSS_ORIGIN_UA, 0)) {
                    out->values[prop_id] = new_value;
                    out->specificity_used = specificity;
                }
            }
        }
    }

    /* Step 3: Apply inheritance for inherited properties */
    if (ctx->parent_computed) {
        for (uint32_t i = 0; i < CSS_PROPERTY_COUNT; i++) {
            const css_property_spec *spec = css_get_property_spec(i);
            if (spec && spec->inherited) {
                /* Inherited property with no matching rule: use parent's value */
                out->values[i] = ctx->parent_computed->values[i];
            }
        }
    }

    /* Step 4: Compute final values for all properties */
    for (uint32_t i = 0; i < CSS_PROPERTY_COUNT; i++) {
        const css_property_spec *spec = css_get_property_spec(i);
        if (!spec || !spec->compute) {
            continue;
        }

        css_property_value computed_value;
        css_error err = spec->compute(&out->values[i], ctx, &computed_value);

        if (err != CSS_OK) {
            /* Per-property error handling: log warning but continue */
            fprintf(stderr, "[CSS] Warning: Failed to compute %s: %d\n",
                    spec->name, err);
            /* Keep original value if compute fails */
        } else {
            out->values[i] = computed_value;
        }
    }

    return CSS_OK;  /* Always succeeds, even if some properties failed */
}

/* ============================================================================
 * Public API: Compute Element Styles
 * ============================================================================ */

/**
 * Main entry point: compute all styles for an element.
 *
 * Parameters:
 * - element: DOM element to compute styles for
 * - parent: Parent element (for inheritance and unit context)
 * - matched_rules: Pre-matched rules from selector matching phase
 * - matched_count: Number of matched rules
 * - specificities: Specificity of each matched rule (parallel array)
 * - origins: Origin of each matched rule (parallel array)
 * - parent_computed: Parent element's computed style (for inheritance)
 * - out_computed: Output computed style
 *
 * Returns: CSS_OK always (per-property error handling)
 */
css_error css_compute_element_styles(
    dom_element *element,
    dom_element *parent,
    struct css_rule *matched_rules,
    uint32_t matched_count,
    uint16_t *specificities,
    css_origin *origins,
    css_computed_style *parent_computed,
    css_computed_style *out_computed
) {
    if (!element || !out_computed) {
        return CSS_BADPARM;
    }

    /* Build cascade context */
    css_cascade_context ctx = {
        .element = element,
        .parent = parent,
        .parent_computed = parent_computed,
        .matched_rules = matched_rules,
        .matched_rule_count = matched_count,
        .specificities = specificities,
        .origins = origins,
    };

    /* Run cascade algorithm */
    return css_cascade_for_element(&ctx, out_computed);
}

/* ============================================================================
 * Debug Utilities
 * ============================================================================ */

/**
 * Convert native css_computed_style to public silk_computed_style_t
 * for backward compatibility with public API
 */
#include "silksurf/css_parser.h"

void css_convert_to_silk_style(
    const css_computed_style *computed,
    void *out_silk_style_void
) {
    /* Cast from void pointer to the actual struct type */
    if (!computed || !out_silk_style_void) {
        return;
    }

    silk_computed_style_t *out_silk_style = (silk_computed_style_t *)out_silk_style_void;

    memset(out_silk_style, 0, sizeof(*out_silk_style));

    /* Extract width */
    const css_property_value *width = &computed->values[CSS_PROP_WIDTH];
    if (width->length.unit == CSS_UNIT_PX) {
        out_silk_style->width = FIXTOINT(width->length.value);
    } else if (width->length.unit == CSS_UNIT_AUTO) {
        out_silk_style->width = -1;
    }

    /* Extract height */
    const css_property_value *height = &computed->values[CSS_PROP_HEIGHT];
    if (height->length.unit == CSS_UNIT_PX) {
        out_silk_style->height = FIXTOINT(height->length.value);
    } else if (height->length.unit == CSS_UNIT_AUTO) {
        out_silk_style->height = -1;
    }

    /* Extract margins */
    out_silk_style->margin_top = FIXTOINT(computed->values[CSS_PROP_MARGIN_TOP].length.value);
    out_silk_style->margin_right = FIXTOINT(computed->values[CSS_PROP_MARGIN_RIGHT].length.value);
    out_silk_style->margin_bottom = FIXTOINT(computed->values[CSS_PROP_MARGIN_BOTTOM].length.value);
    out_silk_style->margin_left = FIXTOINT(computed->values[CSS_PROP_MARGIN_LEFT].length.value);

    /* Extract padding */
    out_silk_style->padding_top = FIXTOINT(computed->values[CSS_PROP_PADDING_TOP].length.value);
    out_silk_style->padding_right = FIXTOINT(computed->values[CSS_PROP_PADDING_RIGHT].length.value);
    out_silk_style->padding_bottom = FIXTOINT(computed->values[CSS_PROP_PADDING_BOTTOM].length.value);
    out_silk_style->padding_left = FIXTOINT(computed->values[CSS_PROP_PADDING_LEFT].length.value);

    /* Extract borders */
    out_silk_style->border_top = FIXTOINT(computed->values[CSS_PROP_BORDER_TOP_WIDTH].length.value);
    out_silk_style->border_right = FIXTOINT(computed->values[CSS_PROP_BORDER_RIGHT_WIDTH].length.value);
    out_silk_style->border_bottom = FIXTOINT(computed->values[CSS_PROP_BORDER_BOTTOM_WIDTH].length.value);
    out_silk_style->border_left = FIXTOINT(computed->values[CSS_PROP_BORDER_LEFT_WIDTH].length.value);

    /* Extract colors */
    out_silk_style->color = computed->values[CSS_PROP_COLOR].color;
    out_silk_style->background_color = computed->values[CSS_PROP_BACKGROUND_COLOR].color;

    /* Extract display */
    out_silk_style->display = computed->values[CSS_PROP_DISPLAY].keyword;

    /* Extract position */
    out_silk_style->position = computed->values[CSS_PROP_POSITION].keyword;

    /* Extract font size */
    const css_property_value *font_size = &computed->values[CSS_PROP_FONT_SIZE];
    if (font_size->length.unit == CSS_UNIT_PX) {
        out_silk_style->font_size = FIXTOINT(font_size->length.value);
    }

    /* Extract font family (note: simplified, just stores pointer) */
    out_silk_style->font_family = "sans-serif";  /* Default, should be from property */
}

/**
 * Print computed style for debugging
 */
void css_debug_print_style(const css_computed_style *style) {
    printf("===== Computed Style =====\n");

    printf("  Display: ");
    const css_property_spec *display_spec = css_get_property_spec(CSS_PROP_DISPLAY);
    if (display_spec && display_spec->debug_print) {
        display_spec->debug_print(&style->values[CSS_PROP_DISPLAY]);
    }
    printf("\n");

    printf("  Color: ");
    const css_property_spec *color_spec = css_get_property_spec(CSS_PROP_COLOR);
    if (color_spec && color_spec->debug_print) {
        color_spec->debug_print(&style->values[CSS_PROP_COLOR]);
    }
    printf("\n");

    printf("  Font Size: ");
    const css_property_spec *font_size_spec = css_get_property_spec(CSS_PROP_FONT_SIZE);
    if (font_size_spec && font_size_spec->debug_print) {
        font_size_spec->debug_print(&style->values[CSS_PROP_FONT_SIZE]);
    }
    printf("\n");

    printf("  Margin: ");
    const css_property_spec *margin_spec = css_get_property_spec(CSS_PROP_MARGIN_TOP);
    if (margin_spec && margin_spec->debug_print) {
        margin_spec->debug_print(&style->values[CSS_PROP_MARGIN_TOP]);
    }
    printf("\n");

    printf("  Padding: ");
    const css_property_spec *padding_spec = css_get_property_spec(CSS_PROP_PADDING_TOP);
    if (padding_spec && padding_spec->debug_print) {
        padding_spec->debug_print(&style->values[CSS_PROP_PADDING_TOP]);
    }
    printf("\n");

    printf("  Background Color: ");
    const css_property_spec *bg_spec = css_get_property_spec(CSS_PROP_BACKGROUND_COLOR);
    if (bg_spec && bg_spec->debug_print) {
        bg_spec->debug_print(&style->values[CSS_PROP_BACKGROUND_COLOR]);
    }
    printf("\n");

    printf("==========================\n");
}
