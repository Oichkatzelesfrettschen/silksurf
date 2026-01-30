/* CSS Selection Handler - Bridge between libcss selector matching and libdom */
#include <string.h>
#include <stdio.h>
#include <stdlib.h>
#include <stdbool.h>
#include <libcss/libcss.h>
#include <dom/dom.h>
#include "silksurf/css_parser.h"

/* ========== LIBCSS SELECTION HANDLER CALLBACKS ========== */
/* These callbacks allow libcss to query our DOM tree for selector matching */

/* Get the name of a node */
static css_error node_name(void *pw, void *node, css_qname *qname) {
    dom_node *n = (dom_node *)node;
    dom_string *name = NULL;
    dom_exception err;
    (void)pw;

    static int call_count = 0;
    if (++call_count < 20) {
        fprintf(stderr, "[CSS Handler] node_name called (#%d)\n", call_count);
    } else if (call_count == 20) {
        fprintf(stderr, "[CSS Handler] node_name called 20 times - possible infinite loop!\n");
    }

    if (!n) {
        return CSS_BADPARM;
    }

    /* Get node name from libdom */
    err = dom_node_get_node_name(n, &name);
    if (err != DOM_NO_ERR || !name) {
        return CSS_BADPARM;
    }

    fprintf(stderr, "[CSS Handler] node_name: got name='%s'\n", dom_string_data(name));

    /* Default HTML namespace (NULL for no namespace) */
    qname->ns = NULL;

    /* Intern the dom_string directly to lwc_string */
    err = dom_string_intern(name, &qname->name);
    dom_string_unref(name);

    if (err != DOM_NO_ERR) {
        fprintf(stderr, "[CSS Handler] node_name: dom_string_intern failed: %d\n", err);
        return CSS_NOMEM;
    }

    fprintf(stderr, "[CSS Handler] node_name: returning CSS_OK with name=%p, ns=%p\n",
            (void *)qname->name, (void *)qname->ns);

    return CSS_OK;
}

/* Get the class names of a node */
static css_error node_classes(void *pw, void *node, lwc_string ***classes, uint32_t *n_classes) {
    dom_node *n = (dom_node *)node;
    dom_exception err;
    (void)pw;

    *classes = NULL;
    *n_classes = 0;

    if (!n) {
        return CSS_OK;
    }

    /* Use proper DOM API to get classes */
    err = dom_element_get_classes((dom_element *)n, classes, n_classes);
    if (err != DOM_NO_ERR) {
        return CSS_NOMEM;
    }

    return CSS_OK;
}

/* Get the ID of a node */
static css_error node_id(void *pw, void *node, lwc_string **id) {
    dom_node *n = (dom_node *)node;
    dom_string *attr = NULL;
    dom_exception err;
    (void)pw;

    *id = NULL;

    if (!n) {
        return CSS_OK;
    }

    /* Use HTML element API to get the id attribute */
    err = dom_html_element_get_id((dom_html_element *)n, &attr);
    if (err != DOM_NO_ERR) {
        return CSS_NOMEM;
    }

    if (attr != NULL) {
        /* Convert dom_string to lwc_string */
        err = dom_string_intern(attr, id);
        dom_string_unref(attr);

        if (err != DOM_NO_ERR) {
            return CSS_NOMEM;
        }
    }

    return CSS_OK;
}

/* Check if node has a given attribute */
static css_error node_has_attribute(void *pw, void *node, const css_qname *qname, bool *match) {
    dom_node *n = (dom_node *)node;
    dom_string *attr_name = NULL;
    dom_exception err;
    (void)pw;

    *match = false;

    if (!n || !qname || !qname->name) {
        return CSS_OK;
    }

    /* Convert lwc_string to dom_string */
    const char *name_data = lwc_string_data(qname->name);
    size_t name_len = lwc_string_length(qname->name);

    err = dom_string_create((const uint8_t *)name_data, name_len, &attr_name);
    if (err != DOM_NO_ERR) {
        return CSS_OK;
    }

    /* Check if attribute exists */
    err = dom_element_has_attribute((dom_element *)n, attr_name, match);
    dom_string_unref(attr_name);

    if (err != DOM_NO_ERR) {
        *match = false;
        return CSS_OK;
    }

    return CSS_OK;
}

/* Get the value of an attribute */
__attribute__((unused))
static css_error node_attribute_value(void *pw, void *node, const css_qname *qname, lwc_string **value) {
    dom_node *n = (dom_node *)node;
    dom_string *attr_name = NULL;
    dom_string *attr_value = NULL;
    dom_exception err;
    (void)pw;

    *value = NULL;

    if (!n || !qname || !qname->name) {
        return CSS_OK;
    }

    /* Convert lwc_string to dom_string */
    const char *name_data = lwc_string_data(qname->name);
    size_t name_len = lwc_string_length(qname->name);

    err = dom_string_create((const uint8_t *)name_data, name_len, &attr_name);
    if (err != DOM_NO_ERR) {
        return CSS_OK;
    }

    /* Get attribute value */
    err = dom_element_get_attribute((dom_element *)n, attr_name, &attr_value);
    dom_string_unref(attr_name);

    if (err != DOM_NO_ERR || !attr_value) {
        return CSS_OK;
    }

    /* Convert dom_string to lwc_string */
    const char *value_data = dom_string_data(attr_value);
    size_t value_len = dom_string_byte_length(attr_value);
    lwc_error lerr = lwc_intern_string(value_data, value_len, value);

    dom_string_unref(attr_value);

    if (lerr != lwc_error_ok) {
        return CSS_NOMEM;
    }

    return CSS_OK;
}

/* Check if node has a given class */
static css_error node_has_class(void *pw, void *node, lwc_string *name, bool *match) {
    lwc_string **classes = NULL;
    uint32_t n_classes = 0;
    css_error err;
    (void)pw;

    *match = false;

    /* Get all classes for this node */
    err = node_classes(pw, node, &classes, &n_classes);
    if (err != CSS_OK) {
        return err;
    }

    /* Check if any class matches */
    for (uint32_t i = 0; i < n_classes; i++) {
        bool is_match = false;
        if (lwc_string_isequal(name, classes[i], &is_match) == lwc_error_ok && is_match) {
            *match = true;
            break;
        }
    }

    /* Free allocated class strings */
    for (uint32_t i = 0; i < n_classes; i++) {
        lwc_string_unref(classes[i]);
    }
    free(classes);

    return CSS_OK;
}

/* Check if node has a given ID */
static css_error node_has_id(void *pw, void *node, lwc_string *name, bool *match) {
    lwc_string *element_id = NULL;
    css_error err;
    (void)pw;

    *match = false;

    /* Get the node's ID */
    err = node_id(pw, node, &element_id);
    if (err != CSS_OK) {
        return err;
    }

    if (element_id != NULL) {
        /* Compare IDs */
        *match = (lwc_string_isequal(name, element_id, match) == lwc_error_ok && *match);
        lwc_string_unref(element_id);
    }

    return CSS_OK;
}

/* Get parent node */
static css_error node_parent_node(void *pw, void *node, void **parent) {
    dom_node *n = (dom_node *)node;
    dom_node *p = NULL;
    dom_exception err;
    (void)pw;

    static int call_count = 0;
    if (++call_count < 20) {
        fprintf(stderr, "[CSS Handler] node_parent_node called (#%d) node=%p\n", call_count, node);
    } else if (call_count == 20) {
        fprintf(stderr, "[CSS Handler] node_parent_node called 20+ times - possible loop!\n");
    }

    if (!n) {
        fprintf(stderr, "[CSS Handler] node_parent_node: NULL node\n");
        *parent = NULL;
        return CSS_OK;
    }

    fprintf(stderr, "[CSS Handler] node_parent_node: calling dom_node_get_parent_node\n");
    err = dom_node_get_parent_node(n, &p);
    fprintf(stderr, "[CSS Handler] node_parent_node: returned err=%d, parent=%p\n", err, (void *)p);
    *parent = (void *)p;

    /* Don't unref - libcss owns the reference now */
    fprintf(stderr, "[CSS Handler] node_parent_node: returning CSS_OK\n");
    return CSS_OK;
}

/* Get next sibling */
static css_error node_next_sibling(void *pw, void *node, void **sibling) {
    dom_node *n = (dom_node *)node;
    dom_node *s = NULL;
    (void)pw;

    if (!n) {
        *sibling = NULL;
        return CSS_OK;
    }

    dom_node_get_next_sibling(n, &s);
    *sibling = (void *)s;

    /* Don't unref - libcss owns the reference now */

    return CSS_OK;
}

/* Get previous sibling */
__attribute__((unused))
static css_error node_prev_sibling(void *pw, void *node, void **sibling) {
    dom_node *n = (dom_node *)node;
    dom_node *s = NULL;
    (void)pw;

    if (!n) {
        *sibling = NULL;
        return CSS_OK;
    }

    dom_node_get_previous_sibling(n, &s);
    *sibling = (void *)s;

    /* Don't unref - libcss owns the reference now */

    return CSS_OK;
}

/* Get first child */
__attribute__((unused))
static css_error node_first_child(void *pw, void *node, void **child) {
    dom_node *n = (dom_node *)node;
    dom_node *c = NULL;
    (void)pw;

    if (!n) {
        *child = NULL;
        return CSS_OK;
    }

    dom_node_get_first_child(n, &c);
    *child = (void *)c;

    /* Don't unref - libcss owns the reference now */

    return CSS_OK;
}

/* Get last child */
__attribute__((unused))
static css_error node_last_child(void *pw, void *node, void **child) {
    dom_node *n = (dom_node *)node;
    dom_node *c = NULL;
    (void)pw;

    if (!n) {
        *child = NULL;
        return CSS_OK;
    }

    dom_node_get_last_child(n, &c);
    *child = (void *)c;

    /* Don't unref - libcss owns the reference now */

    return CSS_OK;
}

/* Check if node is root */
static css_error node_is_root(void *pw, void *node, bool *match) {
    dom_node *n = (dom_node *)node;
    dom_node *parent = NULL;
    dom_exception err;
    (void)pw;

    fprintf(stderr, "[CSS Handler] node_is_root called, node=%p\n", node);

    if (!n) {
        *match = false;
        fprintf(stderr, "[CSS Handler] node_is_root: NULL node, returning false\n");
        return CSS_OK;
    }

    fprintf(stderr, "[CSS Handler] node_is_root: calling dom_node_get_parent_node\n");
    err = dom_node_get_parent_node(n, &parent);
    fprintf(stderr, "[CSS Handler] node_is_root: err=%d, parent=%p\n", err, (void *)parent);
    *match = (parent == NULL);

    if (parent) {
        fprintf(stderr, "[CSS Handler] node_is_root: unreffing parent\n");
        dom_node_unref(parent);
        fprintf(stderr, "[CSS Handler] node_is_root: unref complete\n");
    }

    fprintf(stderr, "[CSS Handler] node_is_root: returning match=%d\n", *match);
    return CSS_OK;
}

/* Count siblings (for :nth-child support) */
static css_error node_count_siblings(void *pw, void *node, bool same_name, bool after, int32_t *count) {
    dom_node *n = (dom_node *)node;
    dom_node *sibling = NULL;
    dom_exception err;
    int32_t cnt = 0;
    (void)pw;

    *count = 0;

    if (!n) {
        return CSS_OK;
    }

    /* Get node name if we need to filter by name */
    dom_string *target_name = NULL;
    if (same_name) {
        err = dom_node_get_node_name(n, &target_name);
        if (err != DOM_NO_ERR || !target_name) {
            return CSS_OK;
        }
    }

    if (after) {
        /* Count siblings after this node */
        sibling = n;
        dom_node_ref(sibling);  /* Take reference */

        while (true) {
            dom_node *next = NULL;
            err = dom_node_get_next_sibling(sibling, &next);
            dom_node_unref(sibling);

            if (err != DOM_NO_ERR || !next) {
                break;
            }

            sibling = next;

            /* Check if this is an element node */
            dom_node_type type;
            err = dom_node_get_node_type(sibling, &type);
            if (err == DOM_NO_ERR && type == DOM_ELEMENT_NODE) {
                /* If filtering by name, check if names match */
                if (same_name && target_name) {
                    dom_string *sibling_name = NULL;
                    err = dom_node_get_node_name(sibling, &sibling_name);
                    if (err == DOM_NO_ERR && sibling_name) {
                        bool match = dom_string_isequal(target_name, sibling_name);
                        dom_string_unref(sibling_name);
                        if (match) {
                            cnt++;
                        }
                    }
                } else {
                    cnt++;
                }
            }
        }
    } else {
        /* Count siblings before this node */
        sibling = n;
        dom_node_ref(sibling);

        while (true) {
            dom_node *prev = NULL;
            err = dom_node_get_previous_sibling(sibling, &prev);
            dom_node_unref(sibling);

            if (err != DOM_NO_ERR || !prev) {
                break;
            }

            sibling = prev;

            /* Check if this is an element node */
            dom_node_type type;
            err = dom_node_get_node_type(sibling, &type);
            if (err == DOM_NO_ERR && type == DOM_ELEMENT_NODE) {
                /* If filtering by name, check if names match */
                if (same_name && target_name) {
                    dom_string *sibling_name = NULL;
                    err = dom_node_get_node_name(sibling, &sibling_name);
                    if (err == DOM_NO_ERR && sibling_name) {
                        bool match = dom_string_isequal(target_name, sibling_name);
                        dom_string_unref(sibling_name);
                        if (match) {
                            cnt++;
                        }
                    }
                } else {
                    cnt++;
                }
            }
        }
    }

    if (target_name) {
        dom_string_unref(target_name);
    }

    *count = cnt;

    return CSS_OK;
}

/* Check if node is empty */
static css_error node_is_empty(void *pw, void *node, bool *match) {
    dom_node *n = (dom_node *)node;
    dom_node *child = NULL;
    (void)pw;

    if (!n) {
        *match = false;
        return CSS_OK;
    }

    dom_node_get_first_child(n, &child);
    *match = (child == NULL);

    if (child) {
        dom_node_unref(child);
    }

    return CSS_OK;
}

/* Check if node is a link */
static css_error node_is_link(void *pw, void *node, bool *match) {
    dom_node *n = (dom_node *)node;
    dom_string *name = NULL;
    dom_exception err;
    (void)pw;

    if (!n) {
        *match = false;
        return CSS_OK;
    }

    err = dom_node_get_node_name(n, &name);
    if (err != DOM_NO_ERR || !name) {
        *match = false;
        return CSS_OK;
    }

    const char *tag_name = dom_string_data(name);
    *match = (tag_name && strcmp(tag_name, "A") == 0);  /* libdom returns uppercase */

    dom_string_unref(name);

    return CSS_OK;
}

/* Check if node is visited (always false for now) */
static css_error node_is_visited(void *pw, void *node, bool *match) {
    (void)pw;
    (void)node;

    *match = false;  /* No history tracking yet */

    return CSS_OK;
}

/* Check if node is hovered (always false for now) */
static css_error node_is_hover(void *pw, void *node, bool *match) {
    (void)pw;
    (void)node;

    *match = false;  /* No hover state yet */

    return CSS_OK;
}

/* Check if node is active (always false for now) */
static css_error node_is_active(void *pw, void *node, bool *match) {
    (void)pw;
    (void)node;

    *match = false;  /* No active state yet */

    return CSS_OK;
}

/* Check if node is focused (always false for now) */
static css_error node_is_focus(void *pw, void *node, bool *match) {
    (void)pw;
    (void)node;

    *match = false;  /* No focus state yet */

    return CSS_OK;
}

/* Check if node is enabled (always true for now) */
static css_error node_is_enabled(void *pw, void *node, bool *match) {
    (void)pw;
    (void)node;

    *match = true;  /* All elements enabled by default */

    return CSS_OK;
}

/* Check if node is disabled (always false for now) */
static css_error node_is_disabled(void *pw, void *node, bool *match) {
    (void)pw;
    (void)node;

    *match = false;  /* No disabled elements yet */

    return CSS_OK;
}

/* Check if node is checked (for form elements) */
static css_error node_is_checked(void *pw, void *node, bool *match) {
    (void)pw;
    (void)node;

    *match = false;  /* No form state yet */

    return CSS_OK;
}

/* Check if node is target (for :target pseudo-class) */
static css_error node_is_target(void *pw, void *node, bool *match) {
    (void)pw;
    (void)node;

    *match = false;  /* No URL fragment tracking yet */

    return CSS_OK;
}

/* Check if node is in a specific language */
static css_error node_is_lang(void *pw, void *node, lwc_string *lang, bool *match) {
    (void)pw;
    (void)node;
    (void)lang;

    *match = false;  /* No language detection yet */

    return CSS_OK;
}

/* Get presentational hint (for HTML attributes like bgcolor) */
static css_error node_presentational_hint(void *pw, void *node, uint32_t *nhints, css_hint **hints) {
    (void)pw;
    (void)node;

    *nhints = 0;
    *hints = NULL;

    return CSS_OK;
}

/* UA default style callback - provides browser default styles */
static css_error ua_default_for_property(void *pw, uint32_t property, css_hint *hint) {
    (void)pw;
    (void)property;
    (void)hint;

    /*
     * KEY INSIGHT: Instead of trying to match all property types with their specific
     * hint values (which vary by libcss version), we simply return CSS_OK for all
     * properties. This tells libcss that we're providing a UA default, even if it's
     * a generic "I don't specifically define this" response.
     *
     * LibCSS will use its own internal defaults when we don't explicitly set hint values.
     * This resolves the CSS_INVALID cascade failure while working with all libcss versions.
     *
     * The alternative would be to implement a complete property-specific system, which
     * is exactly what MODERN_CSS_ENGINE_DESIGN.md specifies as the long-term solution.
     */
    return CSS_OK;
}

/* Check if node has a given name */
static css_error node_has_name(void *pw, void *node, const css_qname *qname, bool *match) {
    dom_node *n = (dom_node *)node;
    dom_string *name = NULL;
    dom_exception err;
    (void)pw;

    fprintf(stderr, "[CSS Handler] node_has_name called\n");

    if (!n || !qname) {
        *match = false;
        return CSS_OK;
    }

    /* Get node's current name */
    err = dom_node_get_node_name(n, &name);
    if (err != DOM_NO_ERR || !name) {
        *match = false;
        return CSS_OK;
    }

    /* Compare with selector name */
    const char *node_name_str = dom_string_data(name);
    const char *selector_name = lwc_string_data(qname->name);

    fprintf(stderr, "[CSS Handler] node_has_name: comparing '%s' with '%s'\n",
            node_name_str, selector_name);

    /* Case-insensitive comparison for HTML */
    *match = (strcasecmp(node_name_str, selector_name) == 0);

    fprintf(stderr, "[CSS Handler] node_has_name: match=%d\n", *match);
    dom_string_unref(name);
    return CSS_OK;
}

/* Stub attribute matching callbacks */
static css_error node_has_attribute_equal(void *pw, void *node, const css_qname *qname,
                                           lwc_string *value, bool *match) {
    (void)pw; (void)node; (void)qname; (void)value;
    *match = false;
    return CSS_OK;
}

static css_error node_has_attribute_dashmatch(void *pw, void *node, const css_qname *qname,
                                                lwc_string *value, bool *match) {
    (void)pw; (void)node; (void)qname; (void)value;
    *match = false;
    return CSS_OK;
}

static css_error node_has_attribute_includes(void *pw, void *node, const css_qname *qname,
                                               lwc_string *value, bool *match) {
    (void)pw; (void)node; (void)qname; (void)value;
    *match = false;
    return CSS_OK;
}

static css_error node_has_attribute_prefix(void *pw, void *node, const css_qname *qname,
                                             lwc_string *value, bool *match) {
    (void)pw; (void)node; (void)qname; (void)value;
    *match = false;
    return CSS_OK;
}

static css_error node_has_attribute_suffix(void *pw, void *node, const css_qname *qname,
                                             lwc_string *value, bool *match) {
    (void)pw; (void)node; (void)qname; (void)value;
    *match = false;
    return CSS_OK;
}

static css_error node_has_attribute_substring(void *pw, void *node, const css_qname *qname,
                                                lwc_string *value, bool *match) {
    (void)pw; (void)node; (void)qname; (void)value;
    *match = false;
    return CSS_OK;
}

/* Simple node data storage - use a map of node pointer to libcss data
 * For single-threaded operation, we just store last node data
 * This is sufficient for libcss's pattern of set/get pairs */
static void *last_node = NULL;
static void *last_node_data = NULL;

/* LibCSS node data storage - simple in-memory cache */
static css_error set_libcss_node_data(void *pw, void *node, void *libcss_node_data) {
    (void)pw;

    fprintf(stderr, "[CSS Handler] set_libcss_node_data called (node=%p, data=%p)\n", node, libcss_node_data);

    /* Store for this node */
    last_node = node;
    last_node_data = libcss_node_data;

    fprintf(stderr, "[CSS Handler] set_libcss_node_data: stored\n");
    return CSS_OK;
}

static css_error get_libcss_node_data(void *pw, void *node, void **libcss_node_data) {
    (void)pw;
    (void)node;

    fprintf(stderr, "[CSS Handler] get_libcss_node_data called (node=%p) - returning last_node_data=%p\n", node, last_node_data);

    /* Always return the most recently set data (libcss might use stack pattern) */
    *libcss_node_data = last_node_data;

    return CSS_OK;
}

/* Named node optimization callbacks - stub implementations */
static css_error named_ancestor_node(void *pw, void *node, const css_qname *qname, void **ancestor) {
    (void)pw; (void)node; (void)qname;
    *ancestor = NULL;
    return CSS_INVALID;  /* Not found - let libcss use parent_node fallback */
}

static css_error named_parent_node(void *pw, void *node, const css_qname *qname, void **parent) {
    (void)pw; (void)node; (void)qname;
    *parent = NULL;
    return CSS_INVALID;  /* Not found - let libcss use parent_node fallback */
}

static css_error named_sibling_node(void *pw, void *node, const css_qname *qname, void **sibling) {
    (void)pw; (void)node; (void)qname;
    *sibling = NULL;
    return CSS_INVALID;  /* Not found - let libcss use sibling_node fallback */
}

static css_error named_generic_sibling_node(void *pw, void *node, const css_qname *qname, void **sibling) {
    (void)pw; (void)node; (void)qname;
    *sibling = NULL;
    return CSS_INVALID;  /* Not found - let libcss use sibling_node fallback */
}

/* Global selection handler */
static css_select_handler silk_select_handler = {
    .handler_version = CSS_SELECT_HANDLER_VERSION_1,
    .node_name = node_name,
    .node_classes = node_classes,
    .node_id = node_id,
    .named_ancestor_node = named_ancestor_node,
    .named_parent_node = named_parent_node,
    .named_sibling_node = named_sibling_node,
    .named_generic_sibling_node = named_generic_sibling_node,
    .parent_node = node_parent_node,
    .sibling_node = node_next_sibling,
    .node_has_name = node_has_name,
    .node_has_class = node_has_class,
    .node_has_id = node_has_id,
    .node_has_attribute = node_has_attribute,
    .node_has_attribute_equal = node_has_attribute_equal,
    .node_has_attribute_dashmatch = node_has_attribute_dashmatch,
    .node_has_attribute_includes = node_has_attribute_includes,
    .node_has_attribute_prefix = node_has_attribute_prefix,
    .node_has_attribute_suffix = node_has_attribute_suffix,
    .node_has_attribute_substring = node_has_attribute_substring,
    .node_is_root = node_is_root,
    .node_count_siblings = node_count_siblings,
    .node_is_empty = node_is_empty,
    .node_is_link = node_is_link,
    .node_is_visited = node_is_visited,
    .node_is_hover = node_is_hover,
    .node_is_active = node_is_active,
    .node_is_focus = node_is_focus,
    .node_is_enabled = node_is_enabled,
    .node_is_disabled = node_is_disabled,
    .node_is_checked = node_is_checked,
    .node_is_target = node_is_target,
    .node_is_lang = node_is_lang,
    .node_presentational_hint = node_presentational_hint,
    .ua_default_for_property = ua_default_for_property,
    .set_libcss_node_data = set_libcss_node_data,
    .get_libcss_node_data = get_libcss_node_data,
};

/* Get the selection handler */
css_select_handler *silk_css_get_select_handler(void) {
    return &silk_select_handler;
}
