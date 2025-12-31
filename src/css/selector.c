#include <string.h>
#include <stdbool.h>
#include "silksurf/selector.h"
#include "silksurf/dom_node.h"

/* Internal helper: match a compound selector (e.g., div.class#id) */
static bool _match_compound(SilkSelectorPart *part, silk_dom_node_t *node) {
    while (part) {
        switch (part->type) {
            case SELECTOR_UNIVERSAL:
                break;
            case SELECTOR_TAG:
                if (strcmp(silk_dom_node_get_tag_name(node), part->value) != 0)
                    return false;
                break;
            case SELECTOR_CLASS:
                /* TODO: Implement class list check after attribute interning */
                return false;
            case SELECTOR_ID:
                {
                    const char *id = silk_dom_node_get_attribute(node, "id");
                    if (!id || strcmp(id, part->value) != 0)
                        return false;
                }
                break;
        }
        part = part->next_in_compound;
    }
    return true;
}

bool silk_selector_match(SilkSelector *sel, silk_dom_node_t *node) {
    if (!sel || !node) return false;

    /* 1. Match current compound selector (Right-most) */
    if (!_match_compound(sel->compound, node)) {
        return false;
    }

    /* 2. If no more parts to the left, we found a match! */
    if (!sel->left) {
        return true;
    }

    /* 3. Handle combinators (R-to-L traversal) */
    silk_dom_node_t *parent = silk_dom_node_get_parent(node);
    
    switch (sel->combinator) {
        case COMBINATOR_CHILD:
            /* Just check immediate parent */
            return silk_selector_match(sel->left, parent);

        case COMBINATOR_DESCENDANT:
            /* Walk up the tree until a match is found or we hit the root */
            while (parent) {
                if (silk_selector_match(sel->left, parent)) {
                    return true;
                }
                /* Potentially dangerous recursion/loop - TLA+ will verify this logic */
                parent = silk_dom_node_get_parent(parent);
            }
            return false;

        default:
            return false;
    }
}
