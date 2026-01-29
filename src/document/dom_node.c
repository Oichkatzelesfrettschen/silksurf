/* DOM Node implementation - wraps libdom nodes for silk_document API */
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <dom/dom.h>
#include "silksurf/dom_node.h"
#include "silksurf/allocator.h"
#include "silksurf/css_parser.h"

/* Silk DOM node wrapper - light abstraction over libdom nodes */
struct silk_dom_node {
    dom_node *libdom_node;      /* Underlying libdom node (referenced) */
    int layout_index;           /* Layout tree index (for rendering) */
    int ref_count;              /* Reference count for this wrapper */
    
    /* Computed styles for this node */
    silk_computed_style_t computed_style;

    /* Legacy tree structure for First Paint / Prototype */
    struct silk_dom_node *parent;
    struct silk_dom_node *first_child;
    struct silk_dom_node *next_sibling;
    char tag_name_buf[64];
};

/* Thread-local (or static for simplicity) arena for wrapper allocation */
static silk_arena_t *g_node_arena = NULL;

/* Set the arena for DOM node allocation */
void silk_dom_set_arena(silk_arena_t *arena) {
    g_node_arena = arena;
}

/* Get underlying libdom node (for CSS engine integration) */
void *silk_dom_node_get_libdom_node(silk_dom_node_t *node) {
    if (!node) {
        return NULL;
    }
    return (void *)node->libdom_node;
}

/* Create a wrapper node (internal helper) */
static silk_dom_node_t *_silk_node_create(void) {
    if (!g_node_arena) {
        fprintf(stderr, "[dom_node] ERROR: Arena not initialized\n");
        return NULL;
    }

    silk_dom_node_t *node = silk_arena_alloc(g_node_arena, sizeof(silk_dom_node_t));
    if (!node) {
        fprintf(stderr, "[dom_node] ERROR: Failed to allocate wrapper node\n");
        return NULL;
    }

    memset(node, 0, sizeof(*node));
    node->ref_count = 1;
    node->layout_index = -1;

    return node;
}

/* ========== Public API - Creation Functions (Legacy/Compatibility) ========== */

silk_dom_node_t *silk_dom_node_create_element(const char *tag) {
    if (!tag || !g_node_arena) {
        return NULL;
    }

    silk_dom_node_t *wrapper = _silk_node_create();
    if (!wrapper)
        return NULL;

    strncpy(wrapper->tag_name_buf, tag, sizeof(wrapper->tag_name_buf) - 1);
    fprintf(stderr, "[dom_node] Created element wrapper (legacy): %s\n", tag);
    return wrapper;
}

silk_dom_node_t *silk_dom_node_create_text(const char *content) {
    if (!content || !g_node_arena) {
        return NULL;
    }

    silk_dom_node_t *wrapper = _silk_node_create();
    if (!wrapper)
        return NULL;

    fprintf(stderr, "[dom_node] Created text wrapper (legacy)\n");
    return wrapper;
}

silk_dom_node_t *silk_dom_node_create_comment(const char *data) {
    if (!data || !g_node_arena) {
        return NULL;
    }

    silk_dom_node_t *wrapper = _silk_node_create();
    if (!wrapper)
        return NULL;

    fprintf(stderr, "[dom_node] Created comment wrapper (legacy)\n");
    return wrapper;
}

/* ========== Wrapper Creation - Primary Path ========== */

silk_dom_node_t *silk_dom_node_wrap_libdom(struct dom_node *libdom_node) {
    if (!libdom_node || !g_node_arena) {
        return NULL;
    }

    silk_dom_node_t *wrapper = _silk_node_create();
    if (!wrapper) {
        return NULL;
    }

    wrapper->libdom_node = libdom_node;
    dom_node_ref(libdom_node);

    return wrapper;
}

/* ========== Tree Operations ========== */

void silk_dom_node_append_child_legacy(silk_dom_node_t *parent, silk_dom_node_t *child) {
    if (!parent || !child) return;
    
    child->parent = parent;
    if (!parent->first_child) {
        parent->first_child = child;
    } else {
        silk_dom_node_t *curr = parent->first_child;
        while (curr->next_sibling) {
            curr = curr->next_sibling;
        }
        curr->next_sibling = child;
    }
}

void silk_dom_node_append_child(silk_dom_node_t *parent, silk_dom_node_t *child) {
    if (!parent || !child || !parent->libdom_node || !child->libdom_node) {
        silk_dom_node_append_child_legacy(parent, child);
        return;
    }

    dom_exception err = dom_node_append_child(
        parent->libdom_node,
        child->libdom_node,
        NULL
    );

    if (err != DOM_NO_ERR) {
        fprintf(stderr, "[dom_node] ERROR: append_child failed: %d\n", err);
    }
}

void silk_dom_node_insert_before(silk_dom_node_t *parent,
                                  silk_dom_node_t *new_child,
                                  silk_dom_node_t *ref_child) {
    if (!parent || !new_child) return;
    /* Legacy support simplified - just append if not libdom */
    if (!parent->libdom_node) {
        silk_dom_node_append_child_legacy(parent, new_child);
        return;
    }

    dom_node *ref_libdom = ref_child ? ref_child->libdom_node : NULL;
    dom_exception err = dom_node_insert_before(parent->libdom_node, new_child->libdom_node, ref_libdom, NULL);
    (void)err;
}

void silk_dom_node_remove_child(silk_dom_node_t *parent, silk_dom_node_t *child) {
    if (!parent || !child || !parent->libdom_node || !child->libdom_node) return;
    dom_node_remove_child(parent->libdom_node, child->libdom_node, NULL);
}

/* ========== Tree Traversal ========== */

silk_dom_node_t *silk_dom_node_get_parent(silk_dom_node_t *node) {
    if (!node) return NULL;
    if (node->parent) return node->parent;
    if (!node->libdom_node) return NULL;

    dom_node *parent_libdom = NULL;
    dom_exception err = dom_node_get_parent_node(node->libdom_node, &parent_libdom);
    if (err != DOM_NO_ERR || !parent_libdom) return NULL;

    return silk_dom_node_wrap_libdom(parent_libdom);
}

silk_dom_node_t *silk_dom_node_get_first_child(silk_dom_node_t *node) {
    if (!node) return NULL;
    if (node->first_child) return node->first_child;
    if (!node->libdom_node) return NULL;

    dom_node *child_libdom = NULL;
    dom_exception err = dom_node_get_first_child(node->libdom_node, &child_libdom);
    if (err != DOM_NO_ERR || !child_libdom) return NULL;

    return silk_dom_node_wrap_libdom(child_libdom);
}

silk_dom_node_t *silk_dom_node_get_next_sibling(silk_dom_node_t *node) {
    if (!node) return NULL;
    if (node->next_sibling) return node->next_sibling;
    if (!node->libdom_node) return NULL;

    dom_node *sibling_libdom = NULL;
    dom_exception err = dom_node_get_next_sibling(node->libdom_node, &sibling_libdom);
    if (err != DOM_NO_ERR || !sibling_libdom) return NULL;

    return silk_dom_node_wrap_libdom(sibling_libdom);
}

silk_dom_node_t *silk_dom_node_get_last_child(silk_dom_node_t *node) {
    silk_dom_node_t *child = silk_dom_node_get_first_child(node);
    if (!child) return NULL;
    while (child->next_sibling) child = child->next_sibling;
    return child;
}

silk_dom_node_t *silk_dom_node_get_previous_sibling(silk_dom_node_t *node) {
    if (!node || !node->parent) return NULL;
    silk_dom_node_t *curr = node->parent->first_child;
    silk_dom_node_t *prev = NULL;
    while (curr && curr != node) {
        prev = curr;
        curr = curr->next_sibling;
    }
    return prev;
}

/* ========== Node Properties ========== */

silk_node_type_t silk_dom_node_get_type(silk_dom_node_t *node) {
    if (!node) return SILK_NODE_UNKNOWN;
    if (node->tag_name_buf[0] != '\0') return SILK_NODE_ELEMENT;
    if (!node->libdom_node) return SILK_NODE_UNKNOWN;

    dom_node_type node_type;
    dom_node_get_node_type(node->libdom_node, &node_type);

    switch (node_type) {
        case DOM_ELEMENT_NODE: return SILK_NODE_ELEMENT;
        case DOM_TEXT_NODE: return SILK_NODE_TEXT;
        case DOM_COMMENT_NODE: return SILK_NODE_COMMENT;
        case DOM_DOCUMENT_TYPE_NODE: return SILK_NODE_DOCTYPE;
        default: return SILK_NODE_UNKNOWN;
    }
}

const char *silk_dom_node_get_tag_name(silk_dom_node_t *node) {
    if (!node) return "";
    if (node->tag_name_buf[0] != '\0') return node->tag_name_buf;
    if (!node->libdom_node) return "";

    dom_element *element = (dom_element *)node->libdom_node;
    dom_string *tag_name = NULL;
    static char tag_buf[64];

    dom_element_get_tag_name(element, &tag_name);
    if (tag_name) {
        const char *data = dom_string_data(tag_name);
        strncpy(tag_buf, data, sizeof(tag_buf)-1);
        dom_string_unref(tag_name);
        return tag_buf;
    }
    return "";
}

const char *silk_dom_node_get_text_content(silk_dom_node_t *node) {
    if (!node || !node->libdom_node) return "";

    dom_node_type node_type;
    dom_exception err = dom_node_get_node_type(node->libdom_node, &node_type);
    if (err != DOM_NO_ERR) return "";

    /* Only text nodes have text content */
    if (node_type != DOM_TEXT_NODE) return "";

    /* Get text data from characterdata interface */
    dom_string *content = NULL;
    err = dom_characterdata_get_data((dom_characterdata *)node->libdom_node, &content);
    if (err != DOM_NO_ERR || !content) return "";

    /* Use thread-local static buffer for returned string */
    static _Thread_local char text_buf[4096];
    const char *data = dom_string_data(content);
    size_t len = dom_string_byte_length(content);

    /* Copy with bounds checking */
    if (len >= sizeof(text_buf)) len = sizeof(text_buf) - 1;
    memcpy(text_buf, data, len);
    text_buf[len] = '\0';

    dom_string_unref(content);
    return text_buf;
}

/* ========== Attributes ========== */

const char *silk_dom_node_get_attribute(silk_dom_node_t *node, const char *name) {
    if (!node || !name || !node->libdom_node) return NULL;

    /* Only element nodes have attributes */
    dom_node_type node_type;
    dom_exception err = dom_node_get_node_type(node->libdom_node, &node_type);
    if (err != DOM_NO_ERR || node_type != DOM_ELEMENT_NODE) return NULL;

    /* Convert attribute name to libdom string */
    dom_string *attr_name = NULL;
    err = dom_string_create((const uint8_t *)name, strlen(name), &attr_name);
    if (err != DOM_NO_ERR || !attr_name) return NULL;

    /* Get attribute value */
    dom_string *attr_value = NULL;
    err = dom_element_get_attribute((dom_element *)node->libdom_node,
                                    attr_name, &attr_value);
    dom_string_unref(attr_name);

    if (err != DOM_NO_ERR || !attr_value) return NULL;

    /* Use thread-local static buffer for returned string */
    static _Thread_local char attr_buf[1024];
    const char *data = dom_string_data(attr_value);
    size_t len = dom_string_byte_length(attr_value);

    /* Copy with bounds checking */
    if (len >= sizeof(attr_buf)) len = sizeof(attr_buf) - 1;
    memcpy(attr_buf, data, len);
    attr_buf[len] = '\0';

    dom_string_unref(attr_value);
    return attr_buf;
}

dom_exception silk_dom_node_set_attribute(silk_dom_node_t *node, const char *name, const char *value) {
    (void)node;  /* Unused - pending implementation */
    (void)name;  /* Unused - pending implementation */
    (void)value; /* Unused - pending implementation */
    return DOM_NO_ERR;
}

/* ========== Reference Counting ========== */

void silk_dom_node_ref(silk_dom_node_t *node) {
    if (node) node->ref_count++;
}

void silk_dom_node_unref(silk_dom_node_t *node) {
    if (!node) return;

    if (node->ref_count > 0) {
        node->ref_count--;

        /* When reference count hits zero, cleanup the libdom node reference */
        if (node->ref_count == 0) {
            if (node->libdom_node) {
                dom_node_unref(node->libdom_node);
                node->libdom_node = NULL;
            }
            /* Note: Wrapper itself is arena-allocated, no manual free needed */
        }
    }
}

/* ========== Layout Index ========== */

void silk_dom_node_set_layout_index(silk_dom_node_t *node, int index) {
    if (node) node->layout_index = index;
}

int silk_dom_node_get_layout_index(silk_dom_node_t *node) {

    return node ? node->layout_index : -1;

}



silk_computed_style_t *silk_dom_node_get_style(silk_dom_node_t *node) {

    return node ? &node->computed_style : NULL;

}
