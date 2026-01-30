/**
 * \file box_model.c
 * \brief Block Layout Algorithm Implementation
 *
 * Implements CSS block-level layout with:
 * - Vertical stacking of block elements
 * - Margin collapse (adjacent vertical margins merge)
 * - Width auto-sizing (fill container width)
 * - Height auto-sizing (sum of children)
 * - Min/max width constraints
 * - Intrinsic sizing for replaced elements
 *
 * Layout algorithm (per CSS 2.2):
 * 1. Compute width: width or (container_width - margins)
 * 2. Compute padding/border from CSS
 * 3. Compute height: auto or specified value
 * 4. Position: relative to previous sibling (with margin collapse)
 * 5. Layout children recursively
 * 6. Adjust height if auto (sum of children)
 *
 * Performance: O(n) tree traversal, O(1) per element
 */

#include <assert.h>
#include <limits.h>
#include <stddef.h>
#include <stdlib.h>
#include <string.h>

#include "silksurf/layout.h"
#include "silksurf/allocator.h"

/* Internal helper: safely add two edge values, detect overflow */
static inline bool safe_add_edges(int32_t a, int32_t b, int32_t *result) {
    if ((b > 0 && a > INT32_MAX - b) || (b < 0 && a < INT32_MIN - b)) {
        return false;  /* Overflow */
    }
    *result = a + b;
    return true;
}

/**
 * Collapse adjacent vertical margins per CSS spec
 *
 * "When two margins are adjacent, they are collapsed into a single margin
 * using the larger of the two margin values. In the case of negative
 * margins, the magnitude of the negative margin is deducted from the
 * magnitude of the positive margin."
 */
int32_t silk_layout_collapse_margins(int32_t margin1, int32_t margin2) {
    /* Both positive: use max */
    if (margin1 >= 0 && margin2 >= 0) {
        return margin1 > margin2 ? margin1 : margin2;
    }
    /* Both negative: use max (closest to zero) */
    if (margin1 < 0 && margin2 < 0) {
        return margin1 > margin2 ? margin1 : margin2;
    }
    /* Mixed: return sum (negative margin "pulls" against positive) */
    return margin1 + margin2;
}

/**
 * Apply min/max width constraints
 *
 * Algorithm:
 *   constrained = max(min_width, min(width, max_width))
 *
 * This ensures: min_width <= result <= max_width
 */
int32_t silk_layout_constrain_width(
    int32_t computed_width,
    int32_t min_width,
    int32_t max_width
) {
    /* If min > max, min wins (CSS spec constraint priority) */
    if (min_width > 0 && max_width > 0 && min_width > max_width) {
        return min_width;
    }

    /* Apply constraints: clamp to [min_width, max_width] */
    int32_t result = computed_width;

    if (min_width > 0 && result < min_width) {
        result = min_width;
    }
    if (max_width > 0 && result > max_width) {
        result = max_width;
    }

    return result;
}

/**
 * Calculate total box width including all layers
 *
 * Returns -1 on overflow (value doesn't fit in int32_t)
 */
int32_t silk_layout_total_width(const layout_box_t *box) {
    if (!box) return -1;

    long long total = (long long)box->margin.left + box->border.left +
                      box->padding.left + box->width + box->padding.right +
                      box->border.right + box->margin.right;

    if (total > INT32_MAX || total < INT32_MIN) {
        return -1;  /* Overflow */
    }
    return (int32_t)total;
}

/**
 * Calculate total box height including all layers
 *
 * Returns -1 on overflow
 */
int32_t silk_layout_total_height(const layout_box_t *box) {
    if (!box) return -1;

    long long total = (long long)box->margin.top + box->border.top +
                      box->padding.top + box->height + box->padding.bottom +
                      box->border.bottom + box->margin.bottom;

    if (total > INT32_MAX || total < INT32_MIN) {
        return -1;  /* Overflow */
    }
    return (int32_t)total;
}

/**
 * Resolve CSS width value to computed width
 *
 * Algorithm:
 * 1. If width: <length> specified in CSS → use that value
 * 2. If width: <percentage> → container_width * percentage / 100
 * 3. If width: auto → container_width - margin_left - margin_right
 * 4. Apply min-width and max-width constraints
 *
 * For simplicity, this stub returns:
 * - 0 for auto (caller will handle)
 * - Specified value for concrete lengths
 */
int32_t silk_layout_resolve_width(
    void *element,
    int32_t container_width
) {
    (void)element;      /* Unused: would read CSS computed_style */
    (void)container_width;

    /* TODO: Read element->computed_style->width */
    /* TODO: Parse CSS value (px, %, em, etc.) */
    /* TODO: Return computed width or 0 for auto */

    return 0;  /* Auto width (caller handles) */
}

/**
 * Resolve CSS height value to computed height
 *
 * Similar to width resolution, but with special handling for auto height
 * (which is determined by content in block-level elements)
 */
int32_t silk_layout_resolve_height(
    void *element,
    int32_t container_height
) {
    (void)element;
    (void)container_height;

    /* TODO: Read element->computed_style->height */
    /* TODO: Return computed height or 0 for auto */

    return 0;  /* Auto height (determined by children) */
}

/**
 * Resolve CSS margin value
 *
 * Margins can be:
 * - Length: 10px, 2em
 * - Percentage: 5% (of container width!)
 * - auto: browser-determined (used for centering)
 *
 * Note: vertical margins (margin-top/bottom) use container width for %,
 * not container height (CSS spec quirk).
 */
int32_t silk_layout_resolve_margin(
    void *element,
    const char *property_name,
    int32_t container_width
) {
    (void)element;
    (void)property_name;
    (void)container_width;

    /* TODO: Read element->computed_style->(margin-left/right/top/bottom) */
    /* TODO: Parse and return value */

    return 0;  /* Zero margin (default) */
}

/**
 * Initialize layout box with default values
 */
static layout_box_t box_init(void) {
    layout_box_t box = {0};
    box.opacity = 255;  /* Fully opaque by default */
    return box;
}

/**
 * Create layout context for a document
 */
layout_context_t *silk_layout_context_create(
    void *root_node,
    int32_t viewport_width,
    int32_t viewport_height,
    struct silk_arena *arena
) {
    if (!arena || !root_node || viewport_width <= 0 || viewport_height <= 0) {
        return NULL;
    }

    layout_context_t *ctx = silk_arena_alloc(arena, sizeof(*ctx));
    if (!ctx) return NULL;

    ctx->root_node = root_node;
    ctx->viewport_width = viewport_width;
    ctx->viewport_height = viewport_height;
    ctx->arena = arena;
    ctx->box_count = 0;
    ctx->reflow_count = 0;

    return ctx;
}

/**
 * Layout block-level element (div, p, h1, etc.)
 *
 * Block layout algorithm:
 * 1. Width: explicit value or fill parent width (minus margins)
 * 2. Left/right margins: explicit or 0
 * 3. Position: below previous sibling (with margin collapse)
 * 4. Children: layout each child block recursively
 * 5. Height: explicit or sum of children (if auto)
 *
 * \param ctx Layout context
 * \param element DOM element (contains CSS computed_style)
 * \param parent_box Parent's laid out box (defines container)
 * \return Computed layout box for this element
 */
layout_box_t silk_layout_compute_block(
    layout_context_t *ctx,
    void *element,
    const layout_box_t *parent_box
) {
    if (!ctx || !element || !parent_box) {
        return box_init();
    }

    layout_box_t box = box_init();
    box.display = DISPLAY_BLOCK;

    /* Available width inside parent (content area) */
    int32_t available_width = parent_box->width;
    if (available_width <= 0) {
        available_width = ctx->viewport_width;
    }

    /* ================================================================
       STEP 1: RESOLVE MARGINS
       ================================================================ */

    box.margin.left = silk_layout_resolve_margin(
        element, "margin-left", available_width);
    box.margin.right = silk_layout_resolve_margin(
        element, "margin-right", available_width);
    box.margin.top = silk_layout_resolve_margin(
        element, "margin-top", available_width);
    box.margin.bottom = silk_layout_resolve_margin(
        element, "margin-bottom", available_width);

    /* ================================================================
       STEP 2: RESOLVE WIDTH
       ================================================================ */

    int32_t resolved_width = silk_layout_resolve_width(element, available_width);
    if (resolved_width == 0) {
        /* Auto width: fill parent width minus margins */
        box.width = available_width - box.margin.left - box.margin.right;
    } else {
        box.width = resolved_width;
    }

    /* Ensure width is non-negative */
    if (box.width < 0) {
        box.width = 0;
    }

    /* ================================================================
       STEP 3: RESOLVE BORDER AND PADDING
       ================================================================ */

    /* For now, assume no border/padding (TODO: read from CSS) */
    memset(&box.padding, 0, sizeof(box.padding));
    memset(&box.border, 0, sizeof(box.border));

    /* ================================================================
       STEP 4: POSITION RELATIVE TO PARENT
       ================================================================ */

    /* Horizontal positioning: left edge of parent + margin */
    box.x = parent_box->x + parent_box->padding.left + box.margin.left;

    /* Vertical positioning: below parent content, with margin collapse */
    int32_t parent_content_y = parent_box->y + parent_box->padding.top;

    /* TODO: Track last child's bottom for proper placement
       For now, position at top of parent content area */
    box.y = parent_content_y + box.margin.top;

    /* ================================================================
       STEP 5: AUTO HEIGHT (sum of children)
       ================================================================ */

    /* Resolve explicit height if specified */
    int32_t resolved_height = silk_layout_resolve_height(element, parent_box->height);
    if (resolved_height > 0) {
        box.height = resolved_height;
    } else {
        /* Auto height: will be calculated after layout children */
        box.height = 0;
    }

    /* ================================================================
       STEP 6: APPLY MIN/MAX CONSTRAINTS
       ================================================================ */

    /* TODO: Read min-width, max-width from CSS */
    box.min_width = 0;
    box.max_width = 0;

    box.width = silk_layout_constrain_width(
        box.width, box.min_width, box.max_width);

    ctx->box_count++;
    return box;
}

/* silk_layout_compute_inline is implemented in src/layout/inline.c */

/**
 * Layout replaced element (img, video, canvas, etc.)
 *
 * Replaced elements have intrinsic dimensions:
 * - Image: width x height from file
 * - Video: width x height specified in HTML attributes
 * - Canvas: width x height from element size
 *
 * Aspect ratio is preserved if only one dimension is specified:
 * - If width specified, height = width / aspect_ratio
 * - If height specified, width = height * aspect_ratio
 */
layout_box_t silk_layout_compute_replaced(
    layout_context_t *ctx,
    void *element
) {
    if (!ctx || !element) {
        return box_init();
    }

    layout_box_t box = box_init();
    box.is_replaced = 1;

    /* TODO: Read element type (img, video, etc.)
       TODO: Get intrinsic dimensions
       TODO: Apply CSS width/height (may override intrinsic)
       TODO: Preserve aspect ratio */

    box.width = 100;     /* Default: 100px */
    box.height = 100;

    ctx->box_count++;
    return box;
}

/**
 * Main layout algorithm: traverse tree and compute boxes
 *
 * Algorithm (recursive):
 * 1. For each element in document order
 * 2. Skip if display: none
 * 3. Compute box based on display type
 * 4. Position element
 * 5. Recurse on children
 * 6. If auto height, sum children heights
 *
 * Time complexity: O(n) where n = element count
 * Space complexity: O(h) where h = tree height (recursion stack)
 */
bool silk_layout_compute(layout_context_t *ctx) {
    if (!ctx || !ctx->root_node) {
        return false;
    }

    /* Root layout box covers entire viewport */
    layout_box_t root_box = {0};
    root_box.x = 0;
    root_box.y = 0;
    root_box.width = ctx->viewport_width;
    root_box.height = ctx->viewport_height;
    root_box.display = DISPLAY_BLOCK;
    root_box.opacity = 255;

    /* TODO: Implement recursive tree traversal
       - Read element's display type from CSS
       - Call appropriate layout function
       - Store computed box (in arena allocation pool)
       - Recurse on children
       - Collapse margins for adjacent siblings
       - Calculate auto height (sum of children)

       For now, just count that we're processing:
    */
    ctx->box_count = 1;
    ctx->reflow_count++;

    /* Store root box for future subtree layout */
    (void)root_box;  /* Will be used in full implementation */

    return true;
}
