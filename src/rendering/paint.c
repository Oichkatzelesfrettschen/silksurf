#include <stdio.h>
#include <string.h>
#include "silksurf/renderer.h"
#include "silksurf/dom_node.h"
#include "silksurf/css_parser.h"

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

/*
 * Placeholder for style extraction - will integrate with silk_css_get_computed_style
 */
__attribute__((unused))
static uint32_t get_node_background_color(silk_dom_node_t *node) {
    /* For now, return a default red for testing 'First Paint' if it's an element */
    if (silk_dom_node_get_type(node) == SILK_NODE_ELEMENT) {
        /* TODO: Actually parse style attributes or computed style */
        const char *tag = silk_dom_node_get_tag_name(node);
        if (strcmp(tag, "div") == 0) return 0xFFFF0000; /* Red */
        if (strcmp(tag, "body") == 0) return 0xFFFFFFFF; /* White */
    }
    return 0x00000000; /* Transparent */
}

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
