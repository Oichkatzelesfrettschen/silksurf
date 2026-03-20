/**
 * \file paint.c
 * \brief Paint operations - Convert layout boxes to rendering commands
 *
 * Converts the computed layout tree (layout boxes with CSS styles) into
 * low-level rendering commands (fill_rect, copy_pixels, blend_pixels).
 *
 * Paint algorithm:
 * 1. Traverse layout tree depth-first (document order)
 * 2. For each visible box, extract rendering properties
 * 3. Paint background (color or image)
 * 4. Paint border (if visible)
 * 5. Paint children (recursively)
 * 6. Paint text content (inline boxes)
 *
 * Optimization: Skip boxes outside viewport (damage tracking)
 */

#include <stdio.h>
#include <string.h>
#include "silksurf/renderer.h"
#include "silksurf/dom_node.h"
#include "silksurf/css_parser.h"
#include "silksurf/layout.h"
#include "silksurf/pixel_ops.h"

void silk_render_queue_init(silk_render_queue_t *queue) {
    if (queue) {
        queue->count = 0;
    }
}

void silk_render_queue_push_rect(silk_render_queue_t *queue, int x, int y, int w, int h, uint32_t color) {
    if (!queue || queue->count >= SILK_RENDER_QUEUE_MAX) {
        return;
    }

    silk_draw_rect_cmd_t *cmd = &queue->commands[queue->count++];
    cmd->x = x;
    cmd->y = y;
    cmd->w = w;
    cmd->h = h;
    cmd->color = color;
}

/**
 * Paint a single layout box with background and borders
 *
 * Algorithm:
 * 1. Check if box is visible (opacity > 0, display != none)
 * 2. Paint background color (if specified)
 * 3. Paint borders (if visible)
 * 4. Update damage rect for incremental rendering
 *
 * \param box Layout box with computed geometry
 * \param dom_node Original DOM element (for style lookup)
 * \param queue Render queue to emit commands into
 */
static void paint_layout_box(const layout_box_t *box, silk_dom_node_t *dom_node,
                              silk_render_queue_t *queue) {
    if (!box || !queue || box->display == DISPLAY_NONE || box->opacity == 0) {
        return;
    }

    silk_computed_style_t *style = NULL;
    uint32_t bg_color = 0xFFFFFFFF;  /* Default: white */

    /* Extract style from DOM node if available */
    if (dom_node) {
        style = silk_dom_node_get_style(dom_node);
        if (style) {
            bg_color = style->background_color;
        }
    }

    /* ================================================================
       STEP 1: PAINT BACKGROUND
       ================================================================ */

    /* Check if background is visible (not fully transparent) */
    uint8_t alpha = (bg_color >> 24) & 0xFF;
    if (alpha > 0) {
        /* Paint background rectangle with content area */
        /* Coordinates: x + padding.left, y + padding.top */
        int bg_x = box->x + box->padding.left;
        int bg_y = box->y + box->padding.top;
        int bg_w = box->width;      /* Content width */
        int bg_h = box->height;     /* Content height */

        silk_render_queue_push_rect(queue, bg_x, bg_y, bg_w, bg_h, bg_color);
    }

    /* ================================================================
       STEP 2: PAINT BORDER (if visible)
       ================================================================ */

    /* TODO: Border painting requires:
       - Extract border-color, border-width from style
       - Paint 4 rectangles (top, right, bottom, left)
       - Handle border-radius for rounded corners
       - Handle border-style (solid, dashed, dotted, etc.) */

    uint32_t border_color = 0xFF000000;  /* Default: black */
    if (style) border_color = style->border_color;
    if ((border_color >> 24) == 0) border_color = 0xFF000000;

    int bx = box->x;
    int by = box->y;
    int outer_w = box->padding.left + box->width + box->padding.right;
    int outer_h = box->padding.top + box->height + box->padding.bottom;

    /* Top border */
    if (box->border.top > 0) {
        silk_render_queue_push_rect(queue, bx, by - box->border.top,
            outer_w + box->border.left + box->border.right, box->border.top, border_color);
    }
    /* Bottom border */
    if (box->border.bottom > 0) {
        silk_render_queue_push_rect(queue, bx, by + outer_h,
            outer_w + box->border.left + box->border.right, box->border.bottom, border_color);
    }
    /* Left border */
    if (box->border.left > 0) {
        silk_render_queue_push_rect(queue, bx - box->border.left, by,
            box->border.left, outer_h, border_color);
    }
    /* Right border */
    if (box->border.right > 0) {
        silk_render_queue_push_rect(queue, bx + outer_w, by,
            box->border.right, outer_h, border_color);
    }
}

/**
 * Recursively paint layout tree
 *
 * Traverses layout boxes in depth-first order (document order):
 * 1. Paint current box background and borders
 * 2. Recurse on children (already positioned by layout algorithm)
 * 3. Paint text content (TODO: text rendering)
 *
 * The layout box tree already has correct positioning from layout phase.
 * Paint phase only needs to render them with proper colors/styles.
 *
 * \param box Current layout box to paint
 * \param dom_node Corresponding DOM element (for style lookup)
 * \param queue Render queue to emit commands into
 */
static void paint_layout_box_recursive(const layout_box_t *box, silk_dom_node_t *dom_node,
                                        silk_render_queue_t *queue) {
    if (!box || !queue) {
        return;
    }

    /* Paint current box -- use embedded dom_node if caller did not supply one */
    silk_dom_node_t *node = dom_node ? dom_node : (silk_dom_node_t *)box->dom_node;
    paint_layout_box(box, node, queue);

    /* Recurse on children using the sibling chain populated by layout_node_recursive() */
    const layout_box_t *child = box->first_child;
    while (child) {
        paint_layout_box_recursive(child, (silk_dom_node_t *)child->dom_node, queue);
        child = child->next_sibling;
    }
}

/**
 * Paint layout tree starting from root box
 *
 * Entry point for converting layout results to rendering commands.
 *
 * \param root_box Root layout box (from silk_layout_compute)
 * \param dom_root Root DOM element
 * \param queue Render queue to populate
 */
void silk_paint_layout_tree(const layout_box_t *root_box, silk_dom_node_t *dom_root,
                            silk_render_queue_t *queue) {
    if (!root_box || !queue) {
        return;
    }

    paint_layout_box_recursive(root_box, dom_root, queue);
}

/* ================================================================
   LEGACY: DOM-based painting (for backward compatibility)
   ================================================================

   The following functions paint directly from DOM tree without
   layout computation. Used during transition period before full
   layout engine integration. Remove after all code uses layout tree. */

/**
 * Paint DOM node tree (legacy - bypasses layout engine)
 *
 * WARNING: This is deprecated. Use silk_paint_layout_tree() instead.
 *
 * Direct DOM tree painting doesn't account for:
 * - Auto width/height calculation
 * - Margin collapse
 * - Constraint resolution
 * - Proper positioning
 *
 * Only use during development before layout integration.
 */
void silk_paint_node(silk_dom_node_t *node, silk_render_queue_t *queue) {
    if (!node || !queue) return;

    silk_computed_style_t *style = silk_dom_node_get_style(node);
    if (!style) return;

    /* 1. Extract geometry from style */
    /* If width/height is auto (-1), use a fallback for now */
    int x = 0, y = 0;
    int w = (style->width == -1) ? 100 : style->width;
    int h = (style->height == -1) ? 100 : style->height;

    /* 2. Extract Color */
    uint32_t bg_color = style->background_color;

    /* 3. Emit Command if visible */
    if ((bg_color >> 24) != 0) { /* If not fully transparent */
        silk_render_queue_push_rect(queue, x, y, w, h, bg_color);
    }

    /* 4. Recursive walk for children */
    silk_dom_node_t *child = silk_dom_node_get_first_child(node);
    while (child) {
        /* Simple absolute positioning offset for testing */
        /* TODO: Real layout engine will compute these */
        silk_paint_node(child, queue);
        child = silk_dom_node_get_next_sibling(child);
    }
}
