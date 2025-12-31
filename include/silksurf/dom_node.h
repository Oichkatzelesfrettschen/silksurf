#ifndef SILK_DOM_NODE_H
#define SILK_DOM_NODE_H

#include <stdbool.h>
#include <dom/dom.h>
#include "silksurf/allocator.h"

/* Forward declaration of libdom types */
struct dom_node;

/* DOM node representation - W3C DOM-like interface */
typedef struct silk_dom_node silk_dom_node_t;

typedef enum {
    SILK_NODE_ELEMENT,
    SILK_NODE_TEXT,
    SILK_NODE_COMMENT,
    SILK_NODE_DOCTYPE,
    SILK_NODE_UNKNOWN
} silk_node_type_t;

/* Arena allocation setup */
void silk_dom_set_arena(silk_arena_t *arena);

/* Get underlying libdom node (for CSS integration) */
void *silk_dom_node_get_libdom_node(silk_dom_node_t *node);

/* Node creation */
silk_dom_node_t *silk_dom_node_create_element(const char *tag);
silk_dom_node_t *silk_dom_node_create_text(const char *content);
silk_dom_node_t *silk_dom_node_create_comment(const char *data);

/* Tree Operations */
void silk_dom_node_append_child(silk_dom_node_t *parent, silk_dom_node_t *child);
void silk_dom_node_append_child_legacy(silk_dom_node_t *parent, silk_dom_node_t *child);

void silk_dom_node_insert_before(silk_dom_node_t *parent,
                                  silk_dom_node_t *new_child,
                                  silk_dom_node_t *ref_child);
void silk_dom_node_remove_child(silk_dom_node_t *parent,
                                 silk_dom_node_t *child);

/* Tree traversal */
silk_dom_node_t *silk_dom_node_get_parent(silk_dom_node_t *node);
silk_dom_node_t *silk_dom_node_get_first_child(silk_dom_node_t *node);
silk_dom_node_t *silk_dom_node_get_last_child(silk_dom_node_t *node);
silk_dom_node_t *silk_dom_node_get_next_sibling(silk_dom_node_t *node);
silk_dom_node_t *silk_dom_node_get_previous_sibling(silk_dom_node_t *node);

/* Node properties */
silk_node_type_t silk_dom_node_get_type(silk_dom_node_t *node);
const char *silk_dom_node_get_tag_name(silk_dom_node_t *node);
const char *silk_dom_node_get_text_content(silk_dom_node_t *node);

/* Attributes */
const char *silk_dom_node_get_attribute(silk_dom_node_t *node,
                                         const char *name);
dom_exception silk_dom_node_set_attribute(silk_dom_node_t *node, const char *name,
                                  const char *value);


/* Reference counting */
void silk_dom_node_ref(silk_dom_node_t *node);
void silk_dom_node_unref(silk_dom_node_t *node);

/* Layout Index */
void silk_dom_node_set_layout_index(silk_dom_node_t *node, int index);
int silk_dom_node_get_layout_index(silk_dom_node_t *node);

/* Style Accessors */
#include "silksurf/css_parser.h"
silk_computed_style_t *silk_dom_node_get_style(silk_dom_node_t *node);

/* Wrapper Creation */
struct dom_node;
silk_dom_node_t *silk_dom_node_wrap_libdom(struct dom_node *libdom_node);

#endif
