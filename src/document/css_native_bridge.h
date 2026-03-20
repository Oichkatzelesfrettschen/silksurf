#ifndef SILK_CSS_NATIVE_BRIDGE_H
#define SILK_CSS_NATIVE_BRIDGE_H

/* Bridge between css_engine.c (which includes libcss) and the native cascade
 * (which defines its own types). This header uses only basic types to avoid
 * type conflicts between libcss and the native cascade engine.
 */

#include <stdint.h>
#include <stdbool.h>
#include <dom/dom.h>
#include "silksurf/css_parser.h"
#include "silksurf/css_native_parser.h"
#include "silksurf/allocator.h"

/* Compute styles for a DOM element using the native CSS pipeline.
 *
 * This function:
 * 1. Iterates all rules in the given stylesheets
 * 2. Matches each rule's selector against the element
 * 3. Runs the native cascade to compute final property values
 * 4. Converts the result to silk_computed_style_t
 *
 * Also handles inline style attributes.
 */
int silk_native_compute_style(
    silk_arena_t *arena,
    dom_element *element,
    dom_element *parent,
    css_parsed_stylesheet_t **sheets,
    int sheet_count,
    const char *inline_style,
    silk_computed_style_t *out_style
);

#endif
