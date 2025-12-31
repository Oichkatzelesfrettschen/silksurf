#ifndef SILK_CASCADE_H
#define SILK_CASCADE_H

#include "silksurf/dom_node.h"
#include "silksurf/css_parser.h"

/**
 * Resolve styles for a DOM subtree using the provided CSS engine.
 * This implements the CSS Cascade, specificity, and inheritance.
 */
void silk_css_cascade(silk_dom_node_t *root, silk_css_engine_t *engine);

#endif
