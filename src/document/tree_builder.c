#include <stdlib.h>
#include <string.h>
#include <stdbool.h>
#include <hubbub/hubbub.h>
#include <hubbub/types.h>
#include <hubbub/tree.h>
#include "silksurf/dom_node.h"
#include "silksurf/allocator.h"

/* Tree builder context - passed to all hubbub callbacks */
typedef struct {
    silk_arena_t *arena;                /* For allocating nodes */
    silk_dom_node_t *root;              /* Root element */
    silk_dom_node_t *current;           /* Current open element (for nesting) */
    int depth;                          /* Tree depth for debugging */
} tree_context_t;

/* Public interface: create a tree builder context */
tree_context_t *silk_tree_context_create(silk_arena_t *arena) {
    if (!arena)
        return NULL;

    tree_context_t *ctx = silk_arena_alloc(arena, sizeof(tree_context_t));
    if (!ctx)
        return NULL;

    memset(ctx, 0, sizeof(*ctx));
    ctx->arena = arena;
    ctx->root = NULL;
    ctx->current = NULL;
    ctx->depth = 0;

    return ctx;
}

/* Public interface: get the root node from a completed parse */
silk_dom_node_t *silk_tree_context_get_root(tree_context_t *ctx) {
    return ctx ? ctx->root : NULL;
}

/* ========== HUBBUB TREE HANDLER CALLBACKS ========== */

/* Callback: create a comment node */
static hubbub_error create_comment(void *ctx, const hubbub_string *data,
                                    void **result) {
    tree_context_t *tree = (tree_context_t *)ctx;
    if (!tree || !data || !result)
        return HUBBUB_BADPARM;

    /* Create comment node with data string */
    char comment_data[256];
    size_t len = (data->len < 255) ? data->len : 255;
    strncpy(comment_data, (const char *)data->ptr, len);
    comment_data[len] = '\0';

    silk_dom_node_t *node = silk_dom_node_create_comment(comment_data);
    if (!node)
        return HUBBUB_NOMEM;

    *result = (void *)node;
    return HUBBUB_OK;
}

/* Callback: create a doctype node */
static hubbub_error create_doctype(void *ctx, const hubbub_doctype *doctype,
                                    void **result) {
    tree_context_t *tree = (tree_context_t *)ctx;
    if (!tree || !result)
        return HUBBUB_BADPARM;

    /* For now, create an element node representing DOCTYPE */
    /* (Could extend dom_node.c to support SILK_NODE_DOCTYPE properly) */
    silk_dom_node_t *node = silk_dom_node_create_element("!DOCTYPE");
    if (!node)
        return HUBBUB_NOMEM;

    *result = (void *)node;
    return HUBBUB_OK;
}

/* Callback: create an element node */
static hubbub_error create_element(void *ctx, const hubbub_tag *tag,
                                    void **result) {
    tree_context_t *tree = (tree_context_t *)ctx;
    if (!tree || !tag || !result)
        return HUBBUB_BADPARM;

    /* Extract tag name from hubbub string */
    char tag_name[64];
    size_t len = (tag->name.len < 63) ? tag->name.len : 63;
    strncpy(tag_name, (const char *)tag->name.ptr, len);
    tag_name[len] = '\0';

    /* Create element node */
    silk_dom_node_t *node = silk_dom_node_create_element(tag_name);
    if (!node)
        return HUBBUB_NOMEM;

    /* Add to tree if we have a current parent */
    if (tree->current) {
        silk_dom_node_append_child(tree->current, node);
    } else if (!tree->root) {
        tree->root = node;  /* First element becomes root */
    } else {
        silk_dom_node_append_child(tree->root, node);
    }

    *result = (void *)node;
    return HUBBUB_OK;
}

/* Callback: create a text node */
static hubbub_error create_text(void *ctx, const hubbub_string *data,
                                 void **result) {
    tree_context_t *tree = (tree_context_t *)ctx;
    if (!tree || !data || !result)
        return HUBBUB_BADPARM;

    /* Extract text content */
    char text_content[512];
    size_t len = (data->len < 511) ? data->len : 511;
    strncpy(text_content, (const char *)data->ptr, len);
    text_content[len] = '\0';

    /* Create text node */
    silk_dom_node_t *node = silk_dom_node_create_text(text_content);
    if (!node)
        return HUBBUB_NOMEM;

    /* Add to current parent */
    if (tree->current) {
        silk_dom_node_append_child(tree->current, node);
    }

    *result = (void *)node;
    return HUBBUB_OK;
}

/* Callback: increment reference count (parser holds reference) */
static hubbub_error ref_node(void *ctx, void *node) {
    tree_context_t *tree = (tree_context_t *)ctx;
    if (!tree || !node)
        return HUBBUB_BADPARM;

    silk_dom_node_ref((silk_dom_node_t *)node);
    return HUBBUB_OK;
}

/* Callback: decrement reference count (parser releases reference) */
static hubbub_error unref_node(void *ctx, void *node) {
    tree_context_t *tree = (tree_context_t *)ctx;
    if (!tree || !node)
        return HUBBUB_BADPARM;

    silk_dom_node_unref((silk_dom_node_t *)node);
    return HUBBUB_OK;
}

/* Callback: append a child to a parent */
static hubbub_error append_child(void *ctx, void *parent, void *child,
                                  void **result) {
    tree_context_t *tree = (tree_context_t *)ctx;
    if (!tree || !parent || !child || !result)
        return HUBBUB_BADPARM;

    silk_dom_node_append_child((silk_dom_node_t *)parent,
                               (silk_dom_node_t *)child);
    *result = child;  /* hubbub may expect the modified node */
    return HUBBUB_OK;
}

/* Callback: insert a child before a reference sibling */
static hubbub_error insert_before(void *ctx, void *parent, void *child,
                                   void *ref_child, void **result) {
    tree_context_t *tree = (tree_context_t *)ctx;
    if (!tree || !parent || !child || !result)
        return HUBBUB_BADPARM;

    silk_dom_node_insert_before((silk_dom_node_t *)parent,
                                (silk_dom_node_t *)child,
                                (silk_dom_node_t *)ref_child);
    *result = child;
    return HUBBUB_OK;
}

/* Callback: remove a child from a parent */
static hubbub_error remove_child(void *ctx, void *parent, void *child,
                                  void **result) {
    tree_context_t *tree = (tree_context_t *)ctx;
    if (!tree || !parent || !child || !result)
        return HUBBUB_BADPARM;

    silk_dom_node_remove_child((silk_dom_node_t *)parent,
                               (silk_dom_node_t *)child);
    *result = child;
    return HUBBUB_OK;
}

/* Callback: clone a node (deep clone if deep=true) */
static hubbub_error clone_node(void *ctx, void *node, bool deep,
                                void **result) {
    tree_context_t *tree = (tree_context_t *)ctx;
    if (!tree || !node || !result)
        return HUBBUB_BADPARM;

    silk_dom_node_t *source = (silk_dom_node_t *)node;
    silk_dom_node_t *clone = NULL;

    /* Clone based on node type */
    switch (silk_dom_node_get_type(source)) {
    case SILK_NODE_ELEMENT:
        clone = silk_dom_node_create_element(silk_dom_node_get_tag_name(source));
        break;
    case SILK_NODE_TEXT:
        clone = silk_dom_node_create_text(silk_dom_node_get_text_content(source));
        break;
    case SILK_NODE_COMMENT:
        clone = silk_dom_node_create_comment(silk_dom_node_get_text_content(source));
        break;
    default:
        return HUBBUB_INVALID;
    }

    if (!clone)
        return HUBBUB_NOMEM;

    /* TODO: If deep=true, recursively clone children */

    *result = (void *)clone;
    return HUBBUB_OK;
}

/* Callback: reparent children of a node to a new parent */
static hubbub_error reparent_children(void *ctx, void *node, void *new_parent) {
    tree_context_t *tree = (tree_context_t *)ctx;
    if (!tree || !node || !new_parent)
        return HUBBUB_BADPARM;

    silk_dom_node_t *old_parent = (silk_dom_node_t *)node;
    silk_dom_node_t *parent = (silk_dom_node_t *)new_parent;

    /* Move each child from old_parent to new_parent */
    silk_dom_node_t *child = silk_dom_node_get_first_child(old_parent);
    while (child) {
        silk_dom_node_t *next = silk_dom_node_get_next_sibling(child);
        silk_dom_node_remove_child(old_parent, child);
        silk_dom_node_append_child(parent, child);
        child = next;
    }

    return HUBBUB_OK;
}

/* Callback: get parent of a node (element_only=true means only element parents) */
static hubbub_error get_parent(void *ctx, void *node, bool element_only,
                                void **result) {
    tree_context_t *tree = (tree_context_t *)ctx;
    if (!tree || !node || !result)
        return HUBBUB_BADPARM;

    silk_dom_node_t *parent = silk_dom_node_get_parent((silk_dom_node_t *)node);

    /* If element_only is true, skip non-element parents */
    if (element_only) {
        while (parent && silk_dom_node_get_type(parent) != SILK_NODE_ELEMENT) {
            parent = silk_dom_node_get_parent(parent);
        }
    }

    *result = (void *)parent;
    return HUBBUB_OK;
}

/* Callback: check if a node has children */
static hubbub_error has_children(void *ctx, void *node, bool *result) {
    tree_context_t *tree = (tree_context_t *)ctx;
    if (!tree || !node || !result)
        return HUBBUB_BADPARM;

    silk_dom_node_t *dom_node = (silk_dom_node_t *)node;
    *result = (silk_dom_node_get_first_child(dom_node) != NULL);
    return HUBBUB_OK;
}

/* Callback: form association (for form elements) - stub for now */
static hubbub_error form_associate(void *ctx, void *form, void *node) {
    tree_context_t *tree = (tree_context_t *)ctx;
    if (!tree || !form || !node)
        return HUBBUB_BADPARM;

    /* TODO: Implement form association if needed */
    return HUBBUB_OK;
}

/* Callback: add attributes to an element */
static hubbub_error add_attributes(void *ctx, void *node,
                                    const hubbub_attribute *attributes,
                                    uint32_t n_attributes) {
    tree_context_t *tree = (tree_context_t *)ctx;
    if (!tree || !node || !attributes)
        return HUBBUB_BADPARM;

    silk_dom_node_t *dom_node = (silk_dom_node_t *)node;

    /* Add each attribute to the element */
    for (uint32_t i = 0; i < n_attributes; i++) {
        const hubbub_attribute *attr = &attributes[i];

        /* Extract attribute name */
        char attr_name[32];
        size_t name_len = (attr->name.len < 31) ? attr->name.len : 31;
        strncpy(attr_name, (const char *)attr->name.ptr, name_len);
        attr_name[name_len] = '\0';

        /* Extract attribute value */
        char attr_value[256];
        size_t value_len = (attr->value.len < 255) ? attr->value.len : 255;
        strncpy(attr_value, (const char *)attr->value.ptr, value_len);
        attr_value[value_len] = '\0';

        /* Set attribute on element */
        silk_dom_node_set_attribute(dom_node, attr_name, attr_value);
    }

    return HUBBUB_OK;
}

/* Callback: set quirks mode (document mode indicator) */
static hubbub_error set_quirks_mode(void *ctx, hubbub_quirks_mode mode) {
    tree_context_t *tree = (tree_context_t *)ctx;
    if (!tree)
        return HUBBUB_BADPARM;
    /* TODO: Track document quirks mode if needed */
    /* Mode values: HUBBUB_QUIRKS_MODE_NONE, LIMITED, or FULL */
    return HUBBUB_OK;
}

/* Callback: encoding change (if detected by parser) */
static hubbub_error encoding_change(void *ctx, const char *charset) {
    tree_context_t *tree = (tree_context_t *)ctx;
    if (!tree || !charset)
        return HUBBUB_BADPARM;
    /* TODO: Handle encoding change if needed */
    return HUBBUB_OK;
}

/* Callback: script completion (for async scripts) */
static hubbub_error complete_script(void *ctx, void *node) {
    tree_context_t *tree = (tree_context_t *)ctx;
    if (!tree || !node)
        return HUBBUB_BADPARM;
    /* TODO: Handle script completion */
    return HUBBUB_OK;
}

/* ========== PUBLIC INTERFACE ========== */

/* Create and return a fully-configured hubbub tree handler with context */
hubbub_tree_handler *silk_tree_handler_create(tree_context_t *tree_ctx) {
    if (!tree_ctx)
        return NULL;

    hubbub_tree_handler *handler = malloc(sizeof(hubbub_tree_handler));
    if (!handler)
        return NULL;

    /* Initialize all 18 callback function pointers */
    handler->create_comment = create_comment;
    handler->create_doctype = create_doctype;
    handler->create_element = create_element;
    handler->create_text = create_text;
    handler->ref_node = ref_node;
    handler->unref_node = unref_node;
    handler->append_child = append_child;
    handler->insert_before = insert_before;
    handler->remove_child = remove_child;
    handler->clone_node = clone_node;
    handler->reparent_children = reparent_children;
    handler->get_parent = get_parent;
    handler->has_children = has_children;
    handler->form_associate = form_associate;
    handler->add_attributes = add_attributes;
    handler->set_quirks_mode = set_quirks_mode;
    handler->encoding_change = encoding_change;
    handler->complete_script = complete_script;

    /* Set the context pointer for callbacks */
    handler->ctx = (void *)tree_ctx;

    return handler;
}

/* Free a tree handler structure (does not free context) */
void silk_tree_handler_destroy(hubbub_tree_handler *handler) {
    if (handler)
        free(handler);
}
