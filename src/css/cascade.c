#include <string.h>
#include "silksurf/cascade.h"
#include "silksurf/dom_node.h"
#include "silksurf/css_parser.h"

void silk_css_cascade(silk_dom_node_t *node, silk_css_engine_t *engine) {
    if (!node || !engine) return;

    silk_computed_style_t *style = silk_dom_node_get_style(node);
    if (!style) return;

    /* 1. Default Values (UA Style Baseline) */
    style->background_color = 0x00000000; /* Transparent default */
    style->color = 0xFF000000;            /* Black default */
    style->width = -1;                    /* auto */
    style->height = -1;                   /* auto */
    style->display = 1;                   /* block (simplified) */

    /* 2. Inheritance from Parent */
    silk_dom_node_t *parent = silk_dom_node_get_parent(node);
    if (parent) {
        silk_computed_style_t *parent_style = silk_dom_node_get_style(parent);
        if (parent_style) {
            /* Inherit properties like color (CSS spec defines which ones inherit) */
            style->color = parent_style->color;
        }
    }

    /* 3. Match Selectors and apply Author Styles */
    /* This will call silk_css_get_computed_style when fully integrated with libcss */
    /* For the First Paint prototype, we'll use a simplified check */
    const char *tag = silk_dom_node_get_tag_name(node);
    if (strcmp(tag, "div") == 0) {
        style->background_color = 0xFFFF0000; /* Red */
        style->width = 100;
        style->height = 100;
    } else if (strcmp(tag, "body") == 0) {
        style->background_color = 0xFFFFFFFF; /* White background for body */
    }

    /* 4. Recursive walk for children */
    silk_dom_node_t *child = silk_dom_node_get_first_child(node);
    while (child) {
        silk_css_cascade(child, engine);
        child = silk_dom_node_get_next_sibling(child);
    }
}
