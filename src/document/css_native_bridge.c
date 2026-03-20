/* Native CSS cascade bridge - isolates native cascade types from libcss
 *
 * This file does NOT include <libcss/libcss.h>, so our native css_error,
 * css_unit, css_origin, etc. types can be used without conflict.
 */

#include "css_cascade.h"
#include "css_selector_match.h"
#include "css_native_bridge.h"

#include <string.h>
#include <strings.h>

/* Map property name string to css_property_id */
static int property_name_to_id(const char *name, size_t len) {
    struct { const char *n; size_t l; int id; } map[] = {
        {"color", 5, CSS_PROP_COLOR},
        {"display", 7, CSS_PROP_DISPLAY},
        {"font-size", 9, CSS_PROP_FONT_SIZE},
        {"font-family", 11, CSS_PROP_FONT_FAMILY},
        {"margin-top", 10, CSS_PROP_MARGIN_TOP},
        {"margin-right", 12, CSS_PROP_MARGIN_RIGHT},
        {"margin-bottom", 13, CSS_PROP_MARGIN_BOTTOM},
        {"margin-left", 11, CSS_PROP_MARGIN_LEFT},
        {"margin", 6, -2},
        {"padding-top", 11, CSS_PROP_PADDING_TOP},
        {"padding-right", 13, CSS_PROP_PADDING_RIGHT},
        {"padding-bottom", 14, CSS_PROP_PADDING_BOTTOM},
        {"padding-left", 12, CSS_PROP_PADDING_LEFT},
        {"padding", 7, -3},
        {"width", 5, CSS_PROP_WIDTH},
        {"height", 6, CSS_PROP_HEIGHT},
        {"background-color", 16, CSS_PROP_BACKGROUND_COLOR},
        {"background", 10, CSS_PROP_BACKGROUND_COLOR},
        {"position", 8, CSS_PROP_POSITION},
    };
    for (size_t i = 0; i < sizeof(map) / sizeof(map[0]); i++) {
        if (len == map[i].l && strncasecmp(name, map[i].n, len) == 0)
            return map[i].id;
    }
    return -1;
}

/* Convert parsed CSS value to native css_property_value */
static css_property_value convert_value(const css_parsed_declaration_t *decl, int prop_id) {
    css_property_value val;
    memset(&val, 0, sizeof(val));
    const css_parsed_value_t *pv = &decl->value;

    switch (pv->type) {
        case CSS_VAL_LENGTH:
            val.length.value = (css_fixed)(pv->data.length.value * (1 << CSS_RADIX_POINT));
            if (pv->data.length.unit && pv->data.length.unit_len >= 2) {
                if (strncasecmp(pv->data.length.unit, "px", 2) == 0) val.length.unit = CSS_UNIT_PX;
                else if (strncasecmp(pv->data.length.unit, "em", 2) == 0) val.length.unit = CSS_UNIT_EM;
                else if (strncasecmp(pv->data.length.unit, "re", 2) == 0) val.length.unit = CSS_UNIT_REM;
                else val.length.unit = CSS_UNIT_PX;
            } else {
                val.length.unit = CSS_UNIT_PX;
            }
            break;
        case CSS_VAL_PERCENTAGE:
            val.length.value = (css_fixed)(pv->data.percentage * (1 << CSS_RADIX_POINT));
            val.length.unit = CSS_UNIT_PERCENT;
            break;
        case CSS_VAL_COLOR:
            val.color = pv->data.color;
            break;
        case CSS_VAL_NUMBER:
            val.length.value = (css_fixed)(pv->data.number * (1 << CSS_RADIX_POINT));
            val.length.unit = CSS_UNIT_PX;
            break;
        case CSS_VAL_KEYWORD:
            if (pv->data.keyword) {
                if (prop_id == CSS_PROP_DISPLAY) {
                    if (strcasecmp(pv->data.keyword, "block") == 0) val.keyword = CSS_DISPLAY_BLOCK;
                    else if (strcasecmp(pv->data.keyword, "inline") == 0) val.keyword = CSS_DISPLAY_INLINE;
                    else if (strcasecmp(pv->data.keyword, "inline-block") == 0) val.keyword = CSS_DISPLAY_INLINE_BLOCK;
                    else if (strcasecmp(pv->data.keyword, "none") == 0) val.keyword = CSS_DISPLAY_NONE;
                } else if (prop_id == CSS_PROP_POSITION) {
                    if (strcasecmp(pv->data.keyword, "static") == 0) val.keyword = CSS_POSITION_STATIC;
                    else if (strcasecmp(pv->data.keyword, "absolute") == 0) val.keyword = CSS_POSITION_ABSOLUTE;
                    else if (strcasecmp(pv->data.keyword, "relative") == 0) val.keyword = CSS_POSITION_RELATIVE;
                    else if (strcasecmp(pv->data.keyword, "fixed") == 0) val.keyword = CSS_POSITION_FIXED;
                } else if (strcasecmp(pv->data.keyword, "auto") == 0) {
                    val.length.unit = CSS_UNIT_AUTO;
                } else if (strcasecmp(pv->data.keyword, "inherit") == 0) {
                    val.length.unit = CSS_UNIT_INHERIT;
                }
            }
            break;
        default:
            break;
    }
    return val;
}

/* Build a css_rule from parsed declarations */
static void build_native_rule(const css_parsed_rule_t *parsed, css_rule *out) {
    memset(out, 0, sizeof(*out));
    for (uint32_t i = 0; i < parsed->declaration_count; i++) {
        const css_parsed_declaration_t *decl = &parsed->declarations[i];
        int prop_id = property_name_to_id(decl->property, decl->property_len);

        if (prop_id == -2) {
            /* margin shorthand */
            css_property_value val = convert_value(decl, CSS_PROP_MARGIN_TOP);
            int ids[] = {CSS_PROP_MARGIN_TOP, CSS_PROP_MARGIN_RIGHT,
                         CSS_PROP_MARGIN_BOTTOM, CSS_PROP_MARGIN_LEFT};
            for (int j = 0; j < 4 && out->property_count < CSS_PROPERTY_COUNT; j++) {
                out->properties[out->property_count].id = ids[j];
                out->properties[out->property_count].value = val;
                out->property_count++;
            }
        } else if (prop_id == -3) {
            /* padding shorthand */
            css_property_value val = convert_value(decl, CSS_PROP_PADDING_TOP);
            int ids[] = {CSS_PROP_PADDING_TOP, CSS_PROP_PADDING_RIGHT,
                         CSS_PROP_PADDING_BOTTOM, CSS_PROP_PADDING_LEFT};
            for (int j = 0; j < 4 && out->property_count < CSS_PROPERTY_COUNT; j++) {
                out->properties[out->property_count].id = ids[j];
                out->properties[out->property_count].value = val;
                out->property_count++;
            }
        } else if (prop_id >= 0 && prop_id < (int)CSS_PROPERTY_COUNT) {
            if (out->property_count < CSS_PROPERTY_COUNT) {
                out->properties[out->property_count].id = prop_id;
                out->properties[out->property_count].value = convert_value(decl, prop_id);
                out->property_count++;
            }
        }
    }
}

int silk_native_compute_style(
    silk_arena_t *arena,
    dom_element *element,
    dom_element *parent,
    css_parsed_stylesheet_t **sheets,
    int sheet_count,
    const char *inline_style,
    silk_computed_style_t *out_style
) {
    if (!element || !out_style) return -1;

    css_rule matched_rules[64];
    uint16_t specificities[64];
    css_origin origins[64];
    uint32_t match_count = 0;

    for (int si = 0; si < sheet_count; si++) {
        css_parsed_stylesheet_t *sheet = sheets[si];
        if (!sheet) continue;

        css_origin origin = (si == 0) ? CSS_ORIGIN_UA : CSS_ORIGIN_AUTHOR;

        for (uint32_t ri = 0; ri < sheet->rule_count; ri++) {
            if (match_count >= 64) break;

            css_parsed_rule_t *prule = &sheet->rules[ri];
            css_rule_selector_t *sel = css_selector_parse(arena, prule->selector_text);
            if (!sel) continue;

            if (css_selector_matches(sel, element, parent)) {
                css_specificity_t spec = css_selector_specificity(sel);
                uint16_t flat = (uint16_t)(spec.ids * 100 + spec.classes_and_attrs * 10 + spec.elements);

                bool has_important = false;
                for (uint32_t di = 0; di < prule->declaration_count; di++) {
                    if (prule->declarations[di].important) { has_important = true; break; }
                }

                build_native_rule(prule, &matched_rules[match_count]);
                specificities[match_count] = flat;
                origins[match_count] = has_important ? CSS_ORIGIN_AUTHOR_IMPORTANT : origin;
                match_count++;
            }
            css_selector_free(sel);
        }
    }

    /* Inline style */
    if (inline_style && inline_style[0] != '\0' && match_count < 64) {
        css_parsed_declaration_t inline_decls[32];
        uint32_t ic = css_parse_inline_style(arena, inline_style,
            strlen(inline_style), inline_decls, 32);
        if (ic > 0) {
            css_parsed_rule_t ir;
            memset(&ir, 0, sizeof(ir));
            ir.declarations = inline_decls;
            ir.declaration_count = ic;
            build_native_rule(&ir, &matched_rules[match_count]);
            specificities[match_count] = 1000;
            origins[match_count] = CSS_ORIGIN_AUTHOR;
            match_count++;
        }
    }

    css_computed_style computed;
    memset(&computed, 0, sizeof(computed));

    css_error cerr = css_compute_element_styles(
        element, parent,
        matched_rules, match_count,
        specificities, origins,
        NULL, &computed
    );

    if (cerr != CSS_OK) {
        memset(out_style, 0, sizeof(*out_style));
        out_style->display = 0;
        out_style->width = -1;
        out_style->height = -1;
        out_style->color = 0xFF000000;
        out_style->font_size = 16;
        return 0;
    }

    css_convert_to_silk_style(&computed, out_style);
    return 0;
}
