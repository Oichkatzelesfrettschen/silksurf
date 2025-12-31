#ifndef SILK_SELECTOR_H
#define SILK_SELECTOR_H

#include "silksurf/dom_node.h"

typedef enum {
    SELECTOR_UNIVERSAL, // *
    SELECTOR_TAG,       // div
    SELECTOR_CLASS,     // .class
    SELECTOR_ID,        // #id
} SilkSelectorType;

typedef enum {
    COMBINATOR_NONE,
    COMBINATOR_DESCENDANT, // space
    COMBINATOR_CHILD,      // >
} SilkCombinator;

/* 
 * SilkSelectorPart: A single component of a selector (e.g., 'div' or '.class')
 * Multiple parts can be combined without a combinator (e.g., 'div.myclass')
 */
typedef struct SilkSelectorPart {
    SilkSelectorType type;
    const char *value;
    struct SilkSelectorPart *next_in_compound;
} SilkSelectorPart;

/* 
 * SilkSelector: A complex selector (Right-to-Left linked list)
 * e.g., 'div > p' is stored as [p] -> COMBINATOR_CHILD -> [div]
 */
typedef struct SilkSelector {
    SilkSelectorPart *compound;
    SilkCombinator combinator;
    struct SilkSelector *left; // The next part to the left (ancestor/parent)
} SilkSelector;

/**
 * Match a complex selector against a DOM node (Right-to-Left)
 */
bool silk_selector_match(SilkSelector *sel, silk_dom_node_t *node);

#endif
