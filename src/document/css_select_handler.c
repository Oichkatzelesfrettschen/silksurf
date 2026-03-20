/* CSS Selection Handler - Bridge between libcss selector matching and libdom */
#include <string.h>
#include <stdio.h>
#include <stdlib.h>
#include <stdbool.h>
#include <libcss/libcss.h>
#include <dom/dom.h>
#include "silksurf/css_parser.h"

/* ========== LIBCSS SELECTION HANDLER CALLBACKS ========== */

static css_error node_name(void *pw, void *node, css_qname *qname) {
    dom_node *n = (dom_node *)node;
    dom_string *name = NULL;
    dom_exception err;
    (void)pw;

    if (!n) return CSS_BADPARM;

    err = dom_node_get_node_name(n, &name);
    if (err != DOM_NO_ERR || !name) return CSS_BADPARM;

    qname->ns = NULL;
    err = dom_string_intern(name, &qname->name);
    dom_string_unref(name);

    return (err == DOM_NO_ERR) ? CSS_OK : CSS_NOMEM;
}

static css_error node_classes(void *pw, void *node, lwc_string ***classes, uint32_t *n_classes) {
    dom_node *n = (dom_node *)node;
    (void)pw;

    *classes = NULL;
    *n_classes = 0;
    if (!n) return CSS_OK;

    dom_exception err = dom_element_get_classes((dom_element *)n, classes, n_classes);
    return (err == DOM_NO_ERR) ? CSS_OK : CSS_NOMEM;
}

static css_error node_id(void *pw, void *node, lwc_string **id) {
    dom_node *n = (dom_node *)node;
    dom_string *attr = NULL;
    dom_exception err;
    (void)pw;

    *id = NULL;
    if (!n) return CSS_OK;

    err = dom_html_element_get_id((dom_html_element *)n, &attr);
    if (err != DOM_NO_ERR) return CSS_NOMEM;

    if (attr != NULL) {
        err = dom_string_intern(attr, id);
        dom_string_unref(attr);
        if (err != DOM_NO_ERR) return CSS_NOMEM;
    }
    return CSS_OK;
}

static css_error node_has_attribute(void *pw, void *node, const css_qname *qname, bool *match) {
    dom_node *n = (dom_node *)node;
    dom_string *attr_name = NULL;
    dom_exception err;
    (void)pw;

    *match = false;
    if (!n || !qname || !qname->name) return CSS_OK;

    const char *name_data = lwc_string_data(qname->name);
    size_t name_len = lwc_string_length(qname->name);

    err = dom_string_create((const uint8_t *)name_data, name_len, &attr_name);
    if (err != DOM_NO_ERR) return CSS_OK;

    err = dom_element_has_attribute((dom_element *)n, attr_name, match);
    dom_string_unref(attr_name);

    if (err != DOM_NO_ERR) *match = false;
    return CSS_OK;
}

static css_error node_has_class(void *pw, void *node, lwc_string *name, bool *match) {
    lwc_string **classes = NULL;
    uint32_t n_classes = 0;
    css_error err;
    (void)pw;

    *match = false;

    err = node_classes(pw, node, &classes, &n_classes);
    if (err != CSS_OK) return err;

    for (uint32_t i = 0; i < n_classes; i++) {
        bool is_match = false;
        if (lwc_string_isequal(name, classes[i], &is_match) == lwc_error_ok && is_match) {
            *match = true;
            break;
        }
    }

    for (uint32_t i = 0; i < n_classes; i++) {
        lwc_string_unref(classes[i]);
    }
    free(classes);
    return CSS_OK;
}

static css_error node_has_id(void *pw, void *node, lwc_string *name, bool *match) {
    lwc_string *element_id = NULL;
    css_error err;
    (void)pw;

    *match = false;
    err = node_id(pw, node, &element_id);
    if (err != CSS_OK) return err;

    if (element_id != NULL) {
        *match = (lwc_string_isequal(name, element_id, match) == lwc_error_ok && *match);
        lwc_string_unref(element_id);
    }
    return CSS_OK;
}

static css_error node_parent_node(void *pw, void *node, void **parent) {
    dom_node *n = (dom_node *)node;
    dom_node *p = NULL;
    (void)pw;

    if (!n) { *parent = NULL; return CSS_OK; }

    dom_node_get_parent_node(n, &p);
    *parent = (void *)p;
    return CSS_OK;
}

static css_error node_next_sibling(void *pw, void *node, void **sibling) {
    dom_node *n = (dom_node *)node;
    dom_node *s = NULL;
    (void)pw;

    if (!n) { *sibling = NULL; return CSS_OK; }

    dom_node_get_next_sibling(n, &s);
    *sibling = (void *)s;
    return CSS_OK;
}

static css_error node_is_root(void *pw, void *node, bool *match) {
    dom_node *n = (dom_node *)node;
    dom_node *parent = NULL;
    (void)pw;

    if (!n) { *match = false; return CSS_OK; }

    dom_node_get_parent_node(n, &parent);
    *match = (parent == NULL);
    if (parent) dom_node_unref(parent);
    return CSS_OK;
}

static css_error node_count_siblings(void *pw, void *node, bool same_name, bool after, int32_t *count) {
    dom_node *n = (dom_node *)node;
    dom_node *sibling = NULL;
    dom_exception err;
    int32_t cnt = 0;
    (void)pw;

    *count = 0;
    if (!n) return CSS_OK;

    dom_string *target_name = NULL;
    if (same_name) {
        err = dom_node_get_node_name(n, &target_name);
        if (err != DOM_NO_ERR || !target_name) return CSS_OK;
    }

    sibling = n;
    dom_node_ref(sibling);

    while (true) {
        dom_node *next = NULL;
        if (after) {
            err = dom_node_get_next_sibling(sibling, &next);
        } else {
            err = dom_node_get_previous_sibling(sibling, &next);
        }
        dom_node_unref(sibling);

        if (err != DOM_NO_ERR || !next) break;
        sibling = next;

        dom_node_type type;
        err = dom_node_get_node_type(sibling, &type);
        if (err == DOM_NO_ERR && type == DOM_ELEMENT_NODE) {
            if (same_name && target_name) {
                dom_string *sibling_name = NULL;
                err = dom_node_get_node_name(sibling, &sibling_name);
                if (err == DOM_NO_ERR && sibling_name) {
                    if (dom_string_isequal(target_name, sibling_name)) cnt++;
                    dom_string_unref(sibling_name);
                }
            } else {
                cnt++;
            }
        }
    }

    if (target_name) dom_string_unref(target_name);
    *count = cnt;
    return CSS_OK;
}

static css_error node_is_empty(void *pw, void *node, bool *match) {
    dom_node *n = (dom_node *)node;
    dom_node *child = NULL;
    (void)pw;

    if (!n) { *match = false; return CSS_OK; }

    dom_node_get_first_child(n, &child);
    *match = (child == NULL);
    if (child) dom_node_unref(child);
    return CSS_OK;
}

static css_error node_is_link(void *pw, void *node, bool *match) {
    dom_node *n = (dom_node *)node;
    dom_string *name = NULL;
    (void)pw;

    if (!n) { *match = false; return CSS_OK; }

    dom_exception err = dom_node_get_node_name(n, &name);
    if (err != DOM_NO_ERR || !name) { *match = false; return CSS_OK; }

    const char *tag_name = dom_string_data(name);
    *match = (tag_name && strcmp(tag_name, "A") == 0);
    dom_string_unref(name);
    return CSS_OK;
}

static css_error node_is_visited(void *pw, void *node, bool *match) {
    (void)pw; (void)node; *match = false; return CSS_OK;
}

static css_error node_is_hover(void *pw, void *node, bool *match) {
    (void)pw; (void)node; *match = false; return CSS_OK;
}

static css_error node_is_active(void *pw, void *node, bool *match) {
    (void)pw; (void)node; *match = false; return CSS_OK;
}

static css_error node_is_focus(void *pw, void *node, bool *match) {
    (void)pw; (void)node; *match = false; return CSS_OK;
}

static css_error node_is_enabled(void *pw, void *node, bool *match) {
    (void)pw; (void)node; *match = true; return CSS_OK;
}

static css_error node_is_disabled(void *pw, void *node, bool *match) {
    (void)pw; (void)node; *match = false; return CSS_OK;
}

static css_error node_is_checked(void *pw, void *node, bool *match) {
    (void)pw; (void)node; *match = false; return CSS_OK;
}

static css_error node_is_target(void *pw, void *node, bool *match) {
    (void)pw; (void)node; *match = false; return CSS_OK;
}

static css_error node_is_lang(void *pw, void *node, lwc_string *lang, bool *match) {
    (void)pw; (void)node; (void)lang; *match = false; return CSS_OK;
}

static css_error node_presentational_hint(void *pw, void *node, uint32_t *nhints, css_hint **hints) {
    (void)pw; (void)node; *nhints = 0; *hints = NULL; return CSS_OK;
}

static css_error ua_default_for_property(void *pw, uint32_t property, css_hint *hint) {
    (void)pw; (void)hint; (void)property;
    return CSS_OK;
}

static css_error node_has_name(void *pw, void *node, const css_qname *qname, bool *match) {
    dom_node *n = (dom_node *)node;
    dom_string *name = NULL;
    (void)pw;

    if (!n || !qname) { *match = false; return CSS_OK; }

    dom_exception err = dom_node_get_node_name(n, &name);
    if (err != DOM_NO_ERR || !name) { *match = false; return CSS_OK; }

    const char *node_name_str = dom_string_data(name);
    const char *selector_name = lwc_string_data(qname->name);
    *match = (strcasecmp(node_name_str, selector_name) == 0);

    dom_string_unref(name);
    return CSS_OK;
}

static css_error node_has_attribute_equal(void *pw, void *node, const css_qname *qname,
                                           lwc_string *value, bool *match) {
    (void)pw; (void)node; (void)qname; (void)value; *match = false; return CSS_OK;
}

static css_error node_has_attribute_dashmatch(void *pw, void *node, const css_qname *qname,
                                                lwc_string *value, bool *match) {
    (void)pw; (void)node; (void)qname; (void)value; *match = false; return CSS_OK;
}

static css_error node_has_attribute_includes(void *pw, void *node, const css_qname *qname,
                                               lwc_string *value, bool *match) {
    (void)pw; (void)node; (void)qname; (void)value; *match = false; return CSS_OK;
}

static css_error node_has_attribute_prefix(void *pw, void *node, const css_qname *qname,
                                             lwc_string *value, bool *match) {
    (void)pw; (void)node; (void)qname; (void)value; *match = false; return CSS_OK;
}

static css_error node_has_attribute_suffix(void *pw, void *node, const css_qname *qname,
                                             lwc_string *value, bool *match) {
    (void)pw; (void)node; (void)qname; (void)value; *match = false; return CSS_OK;
}

static css_error node_has_attribute_substring(void *pw, void *node, const css_qname *qname,
                                                lwc_string *value, bool *match) {
    (void)pw; (void)node; (void)qname; (void)value; *match = false; return CSS_OK;
}

/* Per-engine node data storage -- reset between engine lifecycles */
static void *last_node = NULL;
static void *last_node_data = NULL;

void silk_css_handler_reset(void) {
    last_node = NULL;
    last_node_data = NULL;
}

static css_error set_libcss_node_data(void *pw, void *node, void *libcss_node_data) {
    (void)pw;
    last_node = node;
    last_node_data = libcss_node_data;
    return CSS_OK;
}

static css_error get_libcss_node_data(void *pw, void *node, void **libcss_node_data) {
    (void)pw;
    (void)node;
    *libcss_node_data = last_node_data;
    return CSS_OK;
}

static css_error named_ancestor_node(void *pw, void *node, const css_qname *qname, void **ancestor) {
    (void)pw; (void)node; (void)qname; *ancestor = NULL; return CSS_INVALID;
}

static css_error named_parent_node(void *pw, void *node, const css_qname *qname, void **parent) {
    (void)pw; (void)node; (void)qname; *parent = NULL; return CSS_INVALID;
}

static css_error named_sibling_node(void *pw, void *node, const css_qname *qname, void **sibling) {
    (void)pw; (void)node; (void)qname; *sibling = NULL; return CSS_INVALID;
}

static css_error named_generic_sibling_node(void *pw, void *node, const css_qname *qname, void **sibling) {
    (void)pw; (void)node; (void)qname; *sibling = NULL; return CSS_INVALID;
}

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

css_select_handler *silk_css_get_select_handler(void) {
    return &silk_select_handler;
}
