#ifndef SILKSURF_TREE_BUILDER_H
#define SILKSURF_TREE_BUILDER_H

#include <hubbub/hubbub.h>
#include <hubbub/tree.h>
#include "silksurf/dom_node.h"
#include "silksurf/allocator.h"

/* Opaque tree context type */
typedef struct tree_context tree_context_t;

/* Create a tree builder context for parsing */
tree_context_t *silk_tree_context_create(silk_arena_t *arena);

/* Get the root node from a completed parse */
silk_dom_node_t *silk_tree_context_get_root(tree_context_t *ctx);

/* Create and return a fully-configured hubbub tree handler with context */
hubbub_tree_handler *silk_tree_handler_create(tree_context_t *tree_ctx);

/* Free a tree handler structure (does not free context) */
void silk_tree_handler_destroy(hubbub_tree_handler *handler);

#endif
