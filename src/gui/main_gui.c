#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include "silksurf/window.h"
#include "silksurf/xcb_wrapper.h"
#include "silksurf/renderer.h"
#include "silksurf/allocator.h"
#include "silksurf/html_tokenizer.h"
#include "silksurf/document.h"
#include "silksurf/dom_node.h"
#include "silksurf/css_parser.h"
#include "silksurf/cascade.h"

int main(void) {
    printf("Silksurf GUI: First Paint Prototype\n");

    /* 1. Setup Arena and Components */
    silk_arena_t *arena = silk_arena_create(1024 * 1024);
    silk_dom_set_arena(arena); /* Global arena for DOM nodes */
    
    silk_window_mgr_t *wm = silk_window_mgr_create(NULL);
    if (!wm) {
        fprintf(stderr, "Failed to connect to X server\n");
        return 1;
    }

    silk_app_window_t *win = silk_window_mgr_create_window(wm, "SilkSurf First Paint", 800, 600);
    silk_window_show(win);

    /* 2. Setup CSS Engine */
    silk_css_engine_t *css_engine = silk_css_engine_create(arena);

    /* 3. Mock Document Structure */
    /* <body><div style="background: red;"></div></body> */
    silk_document_t *doc = silk_document_create(1024 * 1024);
    
    /* Using public API for node creation */
    silk_dom_node_t *body = silk_dom_node_create_element("body");
    silk_dom_node_t *div = silk_dom_node_create_element("div");
    
    silk_document_set_root_element(doc, body);
    silk_dom_node_append_child_legacy(body, div);

    /* 4. Render Loop */
    silk_render_queue_t queue;
    silk_render_queue_init(&queue);

    /* Main Loop */
    int running = 1;
    while (running) {
        /* A. Resolve Styles (Cascade) */
        silk_css_cascade(body, css_engine);

        /* B. Layout & Paint (Generating Commands) */
        silk_render_queue_init(&queue);
        silk_paint_node(body, &queue);

        /* B. Flush to XCB (Drawing) */
        /* We need to get raw handles for the flush call */
        silk_display_t *dpy = silk_window_mgr_get_display(wm);
        silk_window_t *xcb_win = silk_window_get_xcb_handle(win);
        silk_gc_t *gc = silk_window_get_gc(win);
        
        silk_xcb_flush_commands(dpy, xcb_win, gc, &queue);

        /* C. Simple Event handling placeholder */
        usleep(16000); /* ~60 FPS */
        
        /* Exit after 5 seconds for automated tests */
        static int frames = 0;
        if (frames++ > 300) running = 0;
    }

    /* Cleanup */
    silk_window_mgr_destroy(wm);
    silk_arena_destroy(arena);

    return 0;
}
