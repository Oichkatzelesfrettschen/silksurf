#ifndef SILKSURF_CSS_SELECTOR_MATCH_H
#define SILKSURF_CSS_SELECTOR_MATCH_H

#include <stdint.h>
#include <stdbool.h>
#include <dom/dom.h>
#include "css_cascade.h"

/* Forward declaration - css_stylesheet defined in libcss */
typedef struct css_stylesheet css_stylesheet;

/* ============================================================================
 * CSS Selector Matching - Native Implementation
 * ============================================================================
 *
 * Matches CSS selectors against DOM elements and returns matching rules
 * with proper specificity and origin information.
 *
 * Design: Standalone selector matching independent of LibCSS cascade.
 * Takes parsed stylesheets as input, produces matched rules for cascade.
 *
 * Spec: CSS Selectors Level 3 (https://www.w3.org/TR/selectors-3/)
 */

/* ============================================================================
 * Selector Types (simplified CSS Selectors Level 3)
 * ============================================================================ */

typedef enum {
    CSS_SELECTOR_UNIVERSAL,      /* * */
    CSS_SELECTOR_TYPE,           /* div, p, span */
    CSS_SELECTOR_CLASS,          /* .classname */
    CSS_SELECTOR_ID,             /* #id */
    CSS_SELECTOR_ATTRIBUTE,      /* [attr], [attr="val"] */
    CSS_SELECTOR_PSEUDO_CLASS,   /* :hover, :first-child, etc. */
    CSS_SELECTOR_DESCENDANT,     /* div p */
    CSS_SELECTOR_CHILD,          /* div > p */
} css_selector_type_t;

/* Specificity: (a, b, c) where a=IDs, b=classes+attrs, c=elements */
typedef struct {
    uint16_t ids;
    uint16_t classes_and_attrs;
    uint16_t elements;
} css_specificity_t;

/* Single selector component in a selector chain */
typedef struct css_selector {
    css_selector_type_t type;
    const char *name;           /* For TYPE, CLASS, ID, ATTRIBUTE */
    const char *value;          /* For ATTRIBUTE selectors */
    css_specificity_t specificity;
    struct css_selector *next;  /* For compound selectors (e.g., div.class#id) */
} css_selector_t;

/* Full CSS rule selector (chain of selectors) */
typedef struct {
    css_selector_t *selectors;  /* Linked list of selector components */
    css_specificity_t specificity;
    uint32_t index;             /* Rule order for source order cascade */
} css_rule_selector_t;

/* ============================================================================
 * Rule Matching Context
 * ============================================================================ */

typedef struct {
    dom_element *element;       /* Current element being matched */
    dom_element *parent;        /* Parent for pseudo-class matching */
    dom_element *root;          /* Document root for :root pseudo-class */
    uint32_t sibling_index;     /* Position among siblings */
} css_match_context_t;

/* ============================================================================
 * Selector Matching API
 * ============================================================================ */

/* Parse CSS selector string into selector structure */
css_rule_selector_t *css_selector_parse(const char *selector_str);

/* Free parsed selector */
void css_selector_free(css_rule_selector_t *selector);

/* Calculate specificity of a selector */
css_specificity_t css_selector_specificity(const css_rule_selector_t *selector);

/* Match a selector against a DOM element */
bool css_selector_matches(
    const css_rule_selector_t *selector,
    dom_element *element,
    dom_element *parent
);

/* ============================================================================
 * Rule Collection and Matching
 * ============================================================================ */

/* Result of matching a single stylesheet */
typedef struct {
    css_rule *matched_rules;    /* Array of matched rules */
    uint32_t matched_count;     /* Number of matched rules */
    uint16_t *specificities;    /* Specificity for each matched rule */
    css_origin *origins;        /* Origin for each matched rule */
} css_match_results_t;

/* Match all rules from stylesheet against element */
css_match_results_t *css_stylesheet_match_element(
    css_stylesheet *stylesheet,
    dom_element *element,
    dom_element *parent,
    css_origin origin
);

/* Free match results */
void css_match_results_free(css_match_results_t *results);

/* ============================================================================
 * Specificity Comparison
 * ============================================================================ */

/* Compare two specificities per CSS cascade rules:
 * Returns: >0 if spec_a > spec_b, 0 if equal, <0 if spec_a < spec_b */
int css_specificity_compare(css_specificity_t spec_a, css_specificity_t spec_b);

#endif /* SILKSURF_CSS_SELECTOR_MATCH_H */
