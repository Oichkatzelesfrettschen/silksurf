#include "css_selector_match.h"
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <ctype.h>

/* ============================================================================
 * CSS Selector Matching Implementation
 * ============================================================================
 *
 * Matches CSS selectors against DOM elements per CSS Selectors Level 3.
 * Calculates specificity per: (IDs, classes+attrs, elements)
 *
 * Supports: type, class, ID, attribute, universal, pseudo-classes
 * Does not support: pseudo-elements, complex selectors (yet)
 */

/* ============================================================================
 * Helper Functions
 * ============================================================================ */

/* Get element tag name */
static const char *get_element_name(dom_element *element) {
    if (!element) return NULL;

    dom_string *name = NULL;
    dom_exception err = dom_element_get_tag_name((dom_element *)element, &name);

    if (err != DOM_NO_ERR || !name) {
        return NULL;
    }

    const char *result = dom_string_data(name);
    dom_string_unref(name);
    return result;
}

/* Check if element has a specific class */
static bool element_has_class(dom_element *element, const char *classname) {
    if (!element || !classname) return false;

    lwc_string **classes = NULL;
    uint32_t n_classes = 0;

    dom_exception err = dom_element_get_classes(element, &classes, &n_classes);
    if (err != DOM_NO_ERR || !classes) {
        return false;
    }

    bool found = false;
    for (uint32_t i = 0; i < n_classes; i++) {
        if (strcmp(lwc_string_data(classes[i]), classname) == 0) {
            found = true;
            break;
        }
    }

    return found;
}

/* Check if element has specific ID */
static bool element_has_id(dom_element *element, const char *id) {
    if (!element || !id) return false;

    dom_string *elem_id = NULL;
    dom_exception err = dom_html_element_get_id((dom_html_element *)element, &elem_id);

    if (err != DOM_NO_ERR || !elem_id) {
        return false;
    }

    bool match = (strcmp(dom_string_data(elem_id), id) == 0);
    dom_string_unref(elem_id);
    return match;
}

/* Check if element has attribute with value */
static bool element_has_attribute(dom_element *element, const char *attr_name, const char *attr_value) {
    if (!element || !attr_name) return false;

    dom_string *attr_name_str = NULL;
    dom_string_create((const uint8_t *)attr_name, strlen(attr_name), &attr_name_str);

    dom_string *attr_val = NULL;
    dom_exception err = dom_element_get_attribute(element, attr_name_str, &attr_val);

    dom_string_unref(attr_name_str);

    if (err != DOM_NO_ERR || !attr_val) {
        return false;
    }

    bool match = (attr_value == NULL || strcmp(dom_string_data(attr_val), attr_value) == 0);
    dom_string_unref(attr_val);
    return match;
}

/* Note: get_parent_element() removed - parent pseudo-classes not yet supported */

/* ============================================================================
 * Selector Parsing
 * ============================================================================ */

css_rule_selector_t *css_selector_parse(const char *selector_str) {
    if (!selector_str) return NULL;

    css_rule_selector_t *selector = malloc(sizeof(css_rule_selector_t));
    if (!selector) return NULL;

    memset(selector, 0, sizeof(*selector));

    /* Simplified parsing: handle basic selectors */
    const char *p = selector_str;
    css_selector_t *first = NULL;
    css_selector_t *current = NULL;

    while (*p) {
        /* Skip whitespace */
        while (*p && isspace(*p)) p++;
        if (!*p) break;

        css_selector_t *sel = malloc(sizeof(css_selector_t));
        if (!sel) return selector;

        memset(sel, 0, sizeof(*sel));

        /* Parse selector based on leading character */
        if (*p == '.') {
            /* Class selector */
            p++;
            const char *class_start = p;
            while (*p && !isspace(*p) && *p != '>' && *p != '+') p++;

            size_t len = p - class_start;
            sel->name = strndup(class_start, len);
            sel->type = CSS_SELECTOR_CLASS;
            sel->specificity.classes_and_attrs = 1;
        } else if (*p == '#') {
            /* ID selector */
            p++;
            const char *id_start = p;
            while (*p && !isspace(*p) && *p != '>' && *p != '+') p++;

            size_t len = p - id_start;
            sel->name = strndup(id_start, len);
            sel->type = CSS_SELECTOR_ID;
            sel->specificity.ids = 1;
        } else if (*p == '*') {
            /* Universal selector */
            p++;
            sel->type = CSS_SELECTOR_UNIVERSAL;
        } else {
            /* Type selector */
            const char *type_start = p;
            while (*p && !isspace(*p) && *p != '>' && *p != '+' && *p != '.' && *p != '#') p++;

            size_t len = p - type_start;
            sel->name = strndup(type_start, len);
            sel->type = CSS_SELECTOR_TYPE;
            sel->specificity.elements = 1;
        }

        if (!first) {
            first = sel;
            current = sel;
        } else {
            current->next = sel;
            current = sel;
        }
    }

    selector->selectors = first;
    return selector;
}

void css_selector_free(css_rule_selector_t *selector) {
    if (!selector) return;

    css_selector_t *current = selector->selectors;
    while (current) {
        css_selector_t *next = current->next;
        free((void *)current->name);
        free((void *)current->value);
        free(current);
        current = next;
    }

    free(selector);
}

css_specificity_t css_selector_specificity(const css_rule_selector_t *selector) {
    css_specificity_t spec = {0};

    if (!selector || !selector->selectors) {
        return spec;
    }

    /* Sum up specificity of all selector components */
    css_selector_t *current = selector->selectors;
    while (current) {
        spec.ids += current->specificity.ids;
        spec.classes_and_attrs += current->specificity.classes_and_attrs;
        spec.elements += current->specificity.elements;
        current = current->next;
    }

    return spec;
}

/* ============================================================================
 * Selector Matching
 * ============================================================================ */

bool css_selector_matches(
    const css_rule_selector_t *selector,
    dom_element *element,
    dom_element *parent
) {
    (void)parent;  /* Not used for basic selectors; reserved for pseudo-classes */
    if (!selector || !element) return false;
    if (!selector->selectors) return true;  /* Empty selector matches all */

    /* Match compound selector (all parts must match) */
    css_selector_t *current = selector->selectors;
    while (current) {
        bool component_matches = false;

        switch (current->type) {
            case CSS_SELECTOR_UNIVERSAL:
                component_matches = true;
                break;

            case CSS_SELECTOR_TYPE: {
                const char *elem_name = get_element_name(element);
                if (elem_name && current->name) {
                    component_matches = (strcasecmp(elem_name, current->name) == 0);
                }
                break;
            }

            case CSS_SELECTOR_CLASS:
                component_matches = element_has_class(element, current->name);
                break;

            case CSS_SELECTOR_ID:
                component_matches = element_has_id(element, current->name);
                break;

            case CSS_SELECTOR_ATTRIBUTE:
                component_matches = element_has_attribute(element, current->name, current->value);
                break;

            case CSS_SELECTOR_PSEUDO_CLASS:
                /* TODO: Implement pseudo-classes (:hover, :first-child, etc.) */
                component_matches = false;
                break;

            default:
                component_matches = false;
        }

        if (!component_matches) {
            return false;
        }

        current = current->next;
    }

    return true;
}

/* ============================================================================
 * Rule Collection
 * ============================================================================ */

css_match_results_t *css_stylesheet_match_element(
    css_stylesheet *stylesheet,
    dom_element *element,
    dom_element *parent,
    css_origin origin
) {
    (void)parent;   /* Reserved for pseudo-class context */
    (void)origin;   /* Reserved for origin tracking */

    if (!stylesheet || !element) return NULL;

    css_match_results_t *results = malloc(sizeof(css_match_results_t));
    if (!results) return NULL;

    memset(results, 0, sizeof(*results));

    /* TODO: Implement stylesheet rule matching
     * This requires access to parsed stylesheet structure
     * For now, return empty results */

    return results;
}

void css_match_results_free(css_match_results_t *results) {
    if (!results) return;
    free(results->matched_rules);
    free(results->specificities);
    free(results->origins);
    free(results);
}

/* ============================================================================
 * Specificity Comparison
 * ============================================================================ */

int css_specificity_compare(css_specificity_t spec_a, css_specificity_t spec_b) {
    /* Compare IDs first */
    if (spec_a.ids != spec_b.ids) {
        return (spec_a.ids > spec_b.ids) ? 1 : -1;
    }

    /* Compare classes+attributes */
    if (spec_a.classes_and_attrs != spec_b.classes_and_attrs) {
        return (spec_a.classes_and_attrs > spec_b.classes_and_attrs) ? 1 : -1;
    }

    /* Compare elements */
    if (spec_a.elements != spec_b.elements) {
        return (spec_a.elements > spec_b.elements) ? 1 : -1;
    }

    return 0;  /* Equal specificity */
}
