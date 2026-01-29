/* CSS Selection Handler - Bridge between libcss selector matching and libdom */
#include <string.h>
#include <stdio.h>
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

    /* Convert dom_string to lwc_string */
    const char *name_data = dom_string_data(name);
    lwc_error lerr = lwc_intern_string(name_data, dom_string_byte_length(name), &qname->name);

    dom_string_unref(name);

    if (lerr != lwc_error_ok) {
        return CSS_NOMEM;
    }

    /* HTML namespace (empty string for default HTML namespace) */
    lerr = lwc_intern_string("", 0, &qname->ns);
    if (lerr != lwc_error_ok) {
        lwc_string_unref(qname->name);
        return CSS_NOMEM;
    }

    return CSS_OK;
}

/* Get the class names of a node */
static css_error node_classes(void *pw, void *node, lwc_string ***classes, uint32_t *n_classes) {
    dom_node *n = (dom_node *)node;
    (void)pw;
    (void)n;

    /* TODO: Implement class attribute parsing */
    /* For now, no classes */
    *classes = NULL;
    *n_classes = 0;

    return CSS_OK;
}

/* Get the ID of a node */
static css_error node_id(void *pw, void *node, lwc_string **id) {
    dom_node *n = (dom_node *)node;
    (void)pw;
    (void)n;

    /* TODO: Implement ID attribute retrieval */
    /* For now, no ID */
    *id = NULL;

    return CSS_OK;
}

/* Check if node has a given attribute */
static css_error node_has_attribute(void *pw, void *node, const css_qname *qname, bool *match) {
    dom_node *n = (dom_node *)node;
    (void)pw;
    (void)n;
    (void)qname;

    /* TODO: Implement attribute checking */
    *match = false;

    return CSS_OK;
}

/* Get the value of an attribute */
__attribute__((unused))
static css_error node_attribute_value(void *pw, void *node, const css_qname *qname, lwc_string **value) {
    dom_node *n = (dom_node *)node;
    (void)pw;
    (void)n;
    (void)qname;

    /* TODO: Implement attribute value retrieval */
    *value = NULL;

    return CSS_OK;
}

/* Check if node has a given class */
static css_error node_has_class(void *pw, void *node, lwc_string *name, bool *match) {
    dom_node *n = (dom_node *)node;
    (void)pw;
    (void)n;
    (void)name;

    /* TODO: Implement class checking */
    *match = false;

    return CSS_OK;
}

/* Check if node has a given ID */
static css_error node_has_id(void *pw, void *node, lwc_string *name, bool *match) {
    dom_node *n = (dom_node *)node;
    (void)pw;
    (void)n;
    (void)name;

    /* TODO: Implement ID checking */
    *match = false;

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
    (void)pw;

    if (!n) {
        *count = 0;
        return CSS_OK;
    }

    /* TODO: Implement sibling counting for :nth-child */
    /* This requires iterating siblings and optionally filtering by name */
    /* For now, return 0 as libcss will fall back to defaults */
    (void)same_name;
    (void)after;
    *count = 0;

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

    /* Provide sensible defaults for common properties */
    switch (property) {
        case CSS_PROP_COLOR:
            hint->data.color = 0xFF000000;  /* Black */
            hint->status = CSS_COLOR_COLOR;
            break;

        case CSS_PROP_DISPLAY:
            hint->status = CSS_DISPLAY_INLINE;
            break;

        case CSS_PROP_FONT_SIZE:
            hint->data.length.value = 16;
            hint->data.length.unit = CSS_UNIT_PX;
            hint->status = CSS_FONT_SIZE_DIMENSION;
            break;

        case CSS_PROP_FONT_FAMILY:
            hint->status = CSS_FONT_FAMILY_SANS_SERIF;
            break;

        default:
            return CSS_INVALID;
    }

    return CSS_OK;
}

/* Stub: Check if node has a given name */
static css_error node_has_name(void *pw, void *node, const css_qname *qname, bool *match) {
    dom_node *n = (dom_node *)node;
    (void)pw;
    (void)qname;
    (void)n;

    /* TODO: Implement by comparing node name with qname */
    *match = false;
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

/* LibCSS node data storage - for internal libcss caching */
static css_error set_libcss_node_data(void *pw, void *node, void *libcss_node_data) {
    (void)pw;
    (void)node;
    (void)libcss_node_data;
    /* TODO: Store this on the dom_node for caching - for now just accept it */
    return CSS_OK;
}

static css_error get_libcss_node_data(void *pw, void *node, void **libcss_node_data) {
    (void)pw;
    (void)node;
    /* TODO: Retrieve stored data - for now return NULL (no cached data) */
    *libcss_node_data = NULL;
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
