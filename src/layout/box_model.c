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
#include "silksurf/css_parser.h"

#include "silksurf/dom_node.h"

/* Flag set by silk_layout_compute to indicate DOM-aware mode.
 * When false, resolve functions return defaults (for math-only tests). */
static bool g_layout_dom_mode = false;

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
    if (!element || !g_layout_dom_mode) return 0;

    silk_computed_style_t *style = silk_dom_node_get_style((silk_dom_node_t *)element);
    if (!style) return 0;

    if (style->width == -1) return 0;
    if (style->width > 0) return style->width;

    (void)container_width;
    return 0;
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
    if (!element || !g_layout_dom_mode) return 0;

    silk_computed_style_t *style = silk_dom_node_get_style((silk_dom_node_t *)element);
    if (!style) return 0;

    if (style->height == -1) return 0;
    if (style->height > 0) return style->height;

    (void)container_height;
    return 0;
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
    if (!element || !property_name || !g_layout_dom_mode) return 0;

    silk_computed_style_t *style = silk_dom_node_get_style((silk_dom_node_t *)element);
    if (!style) return 0;

    (void)container_width;

    if (strcmp(property_name, "margin-top") == 0) return style->margin_top;
    if (strcmp(property_name, "margin-right") == 0) return style->margin_right;
    if (strcmp(property_name, "margin-bottom") == 0) return style->margin_bottom;
    if (strcmp(property_name, "margin-left") == 0) return style->margin_left;

    return 0;
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
 * Detect if element is a replaced element (img, video, canvas, etc.)
 *
 * Replaced elements have intrinsic dimensions and don't have children to layout.
 *
 * \param tag_name Element tag name
 * \return true if replaced element, false otherwise
 */
static bool is_replaced_element(const char *tag_name) {
    if (!tag_name) return false;

    /* Per HTML spec: replaced elements */
    return strcmp(tag_name, "img") == 0 ||
           strcmp(tag_name, "video") == 0 ||
           strcmp(tag_name, "audio") == 0 ||
           strcmp(tag_name, "canvas") == 0 ||
           strcmp(tag_name, "embed") == 0 ||
           strcmp(tag_name, "iframe") == 0 ||
           strcmp(tag_name, "input") == 0 ||
           strcmp(tag_name, "object") == 0;
}

/**
 * Layout replaced element (img, video, canvas, etc.)
 *
 * Replaced elements:
 * - Have intrinsic width/height (from element attributes or defaults)
 * - Do not have children to layout
 * - Aspect ratio preserved if only one dimension specified
 * - Can be overridden by CSS width/height properties
 *
 * Algorithm:
 * 1. Get intrinsic dimensions from element (attributes or defaults)
 * 2. Apply CSS width/height (explicit values override intrinsic)
 * 3. Preserve aspect ratio if only one dimension specified
 * 4. Clamp to min/max width constraints
 * 5. Apply margins, padding, borders
 *
 * Aspect ratio formula:
 * - If width specified, height = width / aspect_ratio
 * - If height specified, width = height * aspect_ratio
 * - Both specified: use as-is (don't preserve ratio)
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
    box.display = DISPLAY_INLINE;  /* Replaced elements are inline-level */

    /* ================================================================
       STEP 1: DETECT ELEMENT TYPE AND GET INTRINSIC DIMENSIONS
       ================================================================ */

    /* For now, use default intrinsic dimensions
       TODO: Get actual dimensions from element attributes (width, height)
       TODO: For images, would need image file metadata (requires image loading)
       TODO: For canvas, get from canvas.width and canvas.height attributes */

    int32_t intrinsic_width = 150;   /* Default: 150px (common for replaced) */
    int32_t intrinsic_height = 150;
    double aspect_ratio = 1.0;       /* Default: 1:1 (square) */

    /* ================================================================
       STEP 2: RESOLVE MARGINS, PADDING, BORDER (like block layout)
       ================================================================ */

    int32_t available_width = ctx->viewport_width;
    if (ctx->root_node && !is_replaced_element("")) {
        /* Would get parent width if integrated with tree layout */
    }

    box.margin.left = silk_layout_resolve_margin(
        element, "margin-left", available_width);
    box.margin.right = silk_layout_resolve_margin(
        element, "margin-right", available_width);
    box.margin.top = silk_layout_resolve_margin(
        element, "margin-top", available_width);
    box.margin.bottom = silk_layout_resolve_margin(
        element, "margin-bottom", available_width);

    /* TODO: Resolve padding and border from CSS
       For now, use defaults */
    memset(&box.padding, 0, sizeof(box.padding));
    memset(&box.border, 0, sizeof(box.border));

    /* ================================================================
       STEP 3: RESOLVE CSS WIDTH AND HEIGHT
       ================================================================ */

    int32_t css_width = silk_layout_resolve_width(element, available_width);
    int32_t css_height = silk_layout_resolve_height(element, ctx->viewport_height);

    /* ================================================================
       STEP 4: APPLY DIMENSIONS WITH ASPECT RATIO PRESERVATION
       ================================================================ */

    int32_t final_width = intrinsic_width;
    int32_t final_height = intrinsic_height;

    if (css_width > 0 && css_height > 0) {
        /* Both specified: use as-is, don't preserve ratio */
        final_width = css_width;
        final_height = css_height;
    } else if (css_width > 0) {
        /* Only width specified: preserve aspect ratio */
        final_width = css_width;
        if (aspect_ratio > 0) {
            final_height = (int32_t)(css_width / aspect_ratio);
        } else {
            final_height = intrinsic_height;
        }
    } else if (css_height > 0) {
        /* Only height specified: preserve aspect ratio */
        final_height = css_height;
        if (aspect_ratio > 0) {
            final_width = (int32_t)(css_height * aspect_ratio);
        } else {
            final_width = intrinsic_width;
        }
    } else {
        /* Neither specified: use intrinsic dimensions */
        final_width = intrinsic_width;
        final_height = intrinsic_height;
    }

    /* ================================================================
       STEP 5: APPLY CONSTRAINTS AND FINALIZE
       ================================================================ */

    /* Apply min/max width constraints */
    final_width = silk_layout_constrain_width(
        final_width,
        box.min_width,
        box.max_width);

    /* Similar for height (TODO: implement silk_layout_constrain_height) */
    if (box.min_height > 0 && final_height < box.min_height) {
        final_height = box.min_height;
    }
    if (box.max_height > 0 && final_height > box.max_height) {
        final_height = box.max_height;
    }

    /* Position (inherited from parent, set by block layout) */
    box.x = 0;
    box.y = 0;

    /* Content dimensions (not including margin/padding/border) */
    box.width = final_width;
    box.height = final_height;

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
/* Recursive layout: compute layout box for a DOM node and its children */
static layout_box_t *layout_node_recursive(
    layout_context_t *ctx,
    silk_dom_node_t *node,
    const layout_box_t *parent_box,
    int32_t *cursor_y
) {
    if (!node || !ctx || !ctx->arena) return NULL;

    /* Skip non-element nodes (text, comment, etc.) */
    if (silk_dom_node_get_type(node) != SILK_NODE_ELEMENT) return NULL;

    /* Check display type from computed style */
    silk_computed_style_t *style = silk_dom_node_get_style(node);
    if (style && style->display == 4) return NULL;  /* display: none */

    /* Compute this element's box */
    layout_box_t *box = silk_arena_alloc(ctx->arena, sizeof(layout_box_t));
    if (!box) return NULL;

    *box = silk_layout_compute_block(ctx, node, parent_box);

    /* Link this box back to its DOM node for the paint phase */
    box->dom_node = node;
    box->first_child = NULL;
    box->next_sibling = NULL;

    /* Set vertical position from cursor */
    box->y = *cursor_y + box->margin.top;

    /* Extract padding and border from style */
    if (style) {
        box->padding.top = style->padding_top;
        box->padding.right = style->padding_right;
        box->padding.bottom = style->padding_bottom;
        box->padding.left = style->padding_left;
        box->border.top = style->border_top;
        box->border.right = style->border_right;
        box->border.bottom = style->border_bottom;
        box->border.left = style->border_left;
    }

    /* Layout children, building the sibling chain for the paint phase */
    int32_t child_cursor_y = box->y + box->border.top + box->padding.top;
    int32_t prev_margin_bottom = 0;
    layout_box_t *last_child_box = NULL;

    silk_dom_node_t *child = silk_dom_node_get_first_child(node);
    while (child) {
        if (silk_dom_node_get_type(child) == SILK_NODE_ELEMENT) {
            silk_computed_style_t *child_style = silk_dom_node_get_style(child);
            int32_t child_margin_top = child_style ? child_style->margin_top : 0;

            /* Margin collapse between siblings */
            int32_t collapsed = silk_layout_collapse_margins(prev_margin_bottom, child_margin_top);
            child_cursor_y -= prev_margin_bottom;
            child_cursor_y -= child_margin_top;
            child_cursor_y += collapsed;

            layout_box_t *child_box = layout_node_recursive(ctx, child, box, &child_cursor_y);
            if (child_box) {
                /* Link into parent's child chain */
                if (!box->first_child) {
                    box->first_child = child_box;
                } else {
                    last_child_box->next_sibling = child_box;
                }
                last_child_box = child_box;

                int32_t child_total = child_box->y + child_box->border.top + child_box->padding.top
                    + child_box->height + child_box->padding.bottom + child_box->border.bottom;
                prev_margin_bottom = child_box->margin.bottom;
                child_cursor_y = child_total + prev_margin_bottom;
            }
        }
        child = silk_dom_node_get_next_sibling(child);
    }

    /* Auto height: from content top to last child bottom */
    if (box->height == 0) {
        int32_t content_top = box->y + box->border.top + box->padding.top;
        box->height = child_cursor_y - content_top - prev_margin_bottom;
        if (box->height < 0) box->height = 0;
    }

    /* Update cursor past this box */
    *cursor_y = box->y + box->border.top + box->padding.top + box->height
                + box->padding.bottom + box->border.bottom;

    /* Store box on the node for paint phase */
    silk_dom_node_set_layout_index(node, (int)ctx->box_count);
    ctx->box_count++;

    return box;
}

bool silk_layout_compute(layout_context_t *ctx) {
    if (!ctx || !ctx->root_node) return false;

    g_layout_dom_mode = true;

    layout_box_t viewport = {0};
    viewport.width = ctx->viewport_width;
    viewport.height = ctx->viewport_height;
    viewport.display = DISPLAY_BLOCK;
    viewport.opacity = 255;

    int32_t cursor_y = 0;
    ctx->root_box = layout_node_recursive(ctx, (silk_dom_node_t *)ctx->root_node, &viewport, &cursor_y);
    ctx->reflow_count++;

    return true;
}
