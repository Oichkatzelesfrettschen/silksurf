#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <dom/dom.h>
#include <dom/bindings/hubbub/parser.h>
#include "silksurf/document.h"
#include "silksurf/allocator.h"
#include "silksurf/dom_node.h"
#include "silksurf/css_parser.h"
#include "silksurf/layout.h"
#include "silksurf/renderer.h"

/* Document structure - holds DOM tree and rendering state */
struct silk_document {
    silk_arena_t *arena;            /* Arena for all allocations */
    silk_dom_node_t *root;          /* Root of DOM tree (wrapped libdom node) */
    silk_renderer_t *renderer;      /* Rendering backend */

    /* Document metadata */
    char title[256];
    char content_type[64];

    /* Layout state */
    int viewport_width;
    int viewport_height;
    int scroll_x;
    int scroll_y;

    /* Parser state */
    int loaded;
    dom_hubbub_parser *parser;      /* libdom/hubbub parser instance */
    dom_document *dom_doc;          /* Underlying libdom document */

    /* CSS Engine */
    silk_css_engine_t *css_engine;

    /* Layout */
    layout_context_t *layout_ctx;
    layout_box_t *root_box;

    /* Statistics */
    int element_count;
};

/* ========== PUBLIC INTERFACE ========== */

/* Create a new document with allocated arena */
silk_document_t *silk_document_create(size_t arena_size) {
    /* Allocate the arena first */
    silk_arena_t *arena = silk_arena_create(arena_size);
    if (!arena)
        return NULL;

    /* Allocate document structure from regular heap (NOT from arena)
       because we need to free it independently from the arena */
    silk_document_t *doc = malloc(sizeof(silk_document_t));
    if (!doc) {
        silk_arena_destroy(arena);
        return NULL;
    }

    /* Initialize document */
    memset(doc, 0, sizeof(*doc));
    doc->arena = arena;
    doc->loaded = 0;
    doc->parser = NULL;
    doc->element_count = 0;
    doc->viewport_width = 1024;
    doc->viewport_height = 768;

    /* Set arena for DOM node allocation */
    silk_dom_set_arena(arena);

    /* Initialize metadata */
    strncpy(doc->title, "Untitled Document", sizeof(doc->title) - 1);
    doc->title[sizeof(doc->title) - 1] = '\0';
    strncpy(doc->content_type, "text/html", sizeof(doc->content_type) - 1);
    doc->content_type[sizeof(doc->content_type) - 1] = '\0';

    fprintf(stderr, "[Document] Created document: %p (arena: %p)\n",
            (void *)doc, (void *)arena);

    return doc;
}

/* Destroy a document and free all resources */
void silk_document_destroy(silk_document_t *doc) {
    if (!doc)
        return;

    fprintf(stderr, "[Document] Destroying document: %p\n", (void *)doc);

    /* Destroy parser if active */
    if (doc->parser) {
        fprintf(stderr, "[Document] Destroying parser: %p\n", (void *)doc->parser);
        dom_hubbub_parser_destroy(doc->parser);
        fprintf(stderr, "[Document] Parser destroyed\n");
        doc->parser = NULL;
    }

    /* Release the libdom document (don't unref, parser owns it) */
    /* Actually, dom_hubbub_parser_create passes ownership to client, but */
    /* dom_hubbub_parser_destroy might also clean it up. Let's not unref it here. */
    if (doc->dom_doc) {
        fprintf(stderr, "[Document] Clearing dom_document reference: %p\n", (void *)doc->dom_doc);
        /* DON'T unref here - parser destroy handles this */
        /* dom_node_unref((dom_node *)doc->dom_doc); */
        doc->dom_doc = NULL;
    }

    /* Clear root reference (it's a wrapper, no separate cleanup needed) */
    fprintf(stderr, "[Document] Clearing root reference\n");
    doc->root = NULL;

    /* Destroy arena (frees all allocations) */
    if (doc->arena) {
        fprintf(stderr, "[Document] Destroying arena: %p\n", (void *)doc->arena);
        silk_arena_destroy(doc->arena);
        fprintf(stderr, "[Document] Arena destroyed\n");
        doc->arena = NULL;
    }

    /* Free the document structure itself (allocated from heap, not arena) */
    fprintf(stderr, "[Document] Freeing document structure: %p\n", (void *)doc);
    free(doc);
    fprintf(stderr, "[Document] Document structure freed\n");
}

/* Get arena from document (for CSS engine) */
silk_arena_t *silk_document_get_arena(silk_document_t *doc) {
    if (!doc) {
        return NULL;
    }
    return doc->arena;
}

/* Parse HTML content into DOM tree */
int silk_document_load_html(silk_document_t *doc, const char *html,
                             size_t html_len) {
    if (!doc || !html || html_len == 0)
        return -1;

    dom_hubbub_error err;
    dom_hubbub_parser_params parser_params;
    dom_element *root_element = NULL;

    fprintf(stderr, "[Parse] Starting HTML parsing with libdom (len=%zu)\n", html_len);

    /* Initialize parser parameters */
    memset(&parser_params, 0, sizeof(parser_params));
    parser_params.enc = "UTF-8";  /* Encoding */
    parser_params.fix_enc = false; /* Don't override encoding detection */
    parser_params.enable_script = false; /* No script execution */
    parser_params.script = NULL;
    parser_params.msg = NULL;      /* No message callback */
    parser_params.ctx = NULL;      /* No context needed */
    parser_params.daf = NULL;      /* No default action fetcher */

    fprintf(stderr, "[Parse] Parser parameters initialized\n");

    /* Create libdom/hubbub parser - this returns both parser and document */
    err = dom_hubbub_parser_create(&parser_params, &doc->parser, &doc->dom_doc);
    if (err != DOM_HUBBUB_OK) {
        fprintf(stderr, "[Parse] Parser creation failed: %d\n", err);
        return -1;
    }
    fprintf(stderr, "[Parse] Parser created: %p, Document: %p\n",
            (void *)doc->parser, (void *)doc->dom_doc);

    /* Parse the HTML chunk */
    fprintf(stderr, "[Parse] Parsing HTML chunk...\n");
    err = dom_hubbub_parser_parse_chunk(doc->parser, (const uint8_t *)html, html_len);
    if (err != DOM_HUBBUB_OK && err != DOM_HUBBUB_HUBBUB_ERR_NEEDDATA) {
        fprintf(stderr, "[Parse] parse_chunk error: %d\n", err);
        return -1;
    }
    fprintf(stderr, "[Parse] parse_chunk returned: %d\n", err);

    /* Signal end of document */
    fprintf(stderr, "[Parse] Calling parser_completed...\n");
    err = dom_hubbub_parser_completed(doc->parser);
    if (err != DOM_HUBBUB_OK) {
        fprintf(stderr, "[Parse] parser_completed error: %d\n", err);
        return -1;
    }
    fprintf(stderr, "[Parse] parser_completed returned: %d\n", err);

    /* Get the root element from the document */
    fprintf(stderr, "[Parse] Getting root element from document\n");
    dom_exception dom_err = dom_document_get_document_element(doc->dom_doc, &root_element);
    if (dom_err != DOM_NO_ERR || !root_element) {
        fprintf(stderr, "[Parse] Failed to get document element: %d\n", dom_err);
        doc->root = NULL;
        doc->loaded = 1;  /* Parser succeeded */
        doc->element_count = 0;
        fprintf(stderr, "[Parse] HTML parsing complete - no root element\n");
        return 0;
    }

    fprintf(stderr, "[Parse] Root element found: %p\n", (void *)root_element);

    /* Wrap the libdom node in our silk_dom_node API */
    doc->root = silk_dom_node_wrap_libdom((dom_node *)root_element);
    if (doc->root) {
        fprintf(stderr, "[Parse] DOM root wrapped: %p\n", (void *)doc->root);
        doc->loaded = 1;
        doc->element_count = 1; /* Will be updated during traversal if needed */
    } else {
        fprintf(stderr, "[Parse] Failed to wrap root element\n");
        dom_node_unref((dom_node *)root_element);
        doc->loaded = 1;
        doc->element_count = 0;
    }

    fprintf(stderr, "[Parse] HTML parsing complete - root: %p\n", (void *)doc->root);
    return 0;
}

/* Load HTML from file */
int silk_document_load_html_file(silk_document_t *doc, const char *filename) {
    if (!doc || !filename)
        return -1;

    /* Open file */
    FILE *f = fopen(filename, "rb");
    if (!f)
        return -1;

    /* Get file size */
    fseek(f, 0, SEEK_END);
    size_t size = ftell(f);
    fseek(f, 0, SEEK_SET);

    /* Allocate buffer */
    char *buffer = malloc(size);
    if (!buffer) {
        fclose(f);
        return -1;
    }

    /* Read file */
    if (fread(buffer, 1, size, f) != size) {
        free(buffer);
        fclose(f);
        return -1;
    }

    fclose(f);

    /* Parse HTML */
    int result = silk_document_load_html(doc, buffer, size);
    free(buffer);

    return result;
}

/* Recursive CSS style computation for entire subtree */
static void apply_styles_recursive(silk_css_engine_t *engine, silk_dom_node_t *node) {
    if (!node || !engine) return;

    if (silk_dom_node_get_type(node) == SILK_NODE_ELEMENT) {
        silk_computed_style_t *style = silk_dom_node_get_style(node);
        if (style) {
            silk_css_get_computed_style(engine, node, style);
        }
    }

    silk_dom_node_t *child = silk_dom_node_get_first_child(node);
    while (child) {
        apply_styles_recursive(engine, child);
        child = silk_dom_node_get_next_sibling(child);
    }
}

/* Layout document - compute element positions and sizes */
int silk_document_layout(silk_document_t *doc, int width, int height) {
    if (!doc || width <= 0 || height <= 0) return -1;
    if (!doc->root) return -1;

    doc->viewport_width = width;
    doc->viewport_height = height;

    /* Step 1: Create/reuse CSS engine */
    if (!doc->css_engine) {
        doc->css_engine = silk_css_engine_create(doc->arena);
        if (!doc->css_engine) return -1;
    }

    /* Step 2: Apply CSS styles to all elements */
    apply_styles_recursive(doc->css_engine, doc->root);

    /* Step 3: Compute layout */
    doc->layout_ctx = silk_layout_context_create(
        doc->root, width, height, doc->arena);
    if (!doc->layout_ctx) return -1;

    if (!silk_layout_compute(doc->layout_ctx)) return -1;

    /* Cache root layout box for the render phase */
    doc->root_box = doc->layout_ctx->root_box;

    return 0;
}

/* Render document to screen via renderer */
void silk_document_render(silk_document_t *doc) {
    if (!doc || !doc->renderer || !doc->loaded || !doc->root) return;

    /* Build render queue from layout tree */
    silk_render_queue_t queue;
    silk_render_queue_init(&queue);

    /* Paint via layout tree when available (uses correct x/y from layout engine),
     * or fall back to legacy DOM paint for pages that skipped silk_document_layout(). */
    if (doc->root_box) {
        silk_paint_layout_tree(doc->root_box, NULL, &queue);
    } else {
        silk_paint_node(doc->root, &queue);
    }

    /* Execute render commands */
    silk_renderer_begin_frame(doc->renderer);
    silk_renderer_clear(doc->renderer, SILK_COLOR_WHITE);

    for (int i = 0; i < queue.count; i++) {
        silk_draw_rect_cmd_t *cmd = &queue.commands[i];
        uint8_t a = (cmd->color >> 24) & 0xFF;
        uint8_t r = (cmd->color >> 16) & 0xFF;
        uint8_t g = (cmd->color >> 8) & 0xFF;
        uint8_t b = cmd->color & 0xFF;
        silk_renderer_fill_rect(doc->renderer, cmd->x, cmd->y, cmd->w, cmd->h,
                                silk_color(a, r, g, b));
    }

    silk_renderer_end_frame(doc->renderer);
    silk_renderer_present(doc->renderer);
}

/* Set the rendering backend */
void silk_document_set_renderer(silk_document_t *doc, silk_renderer_t *renderer) {
    if (doc)
        doc->renderer = renderer;
}

/* Get document title */
const char *silk_document_get_title(silk_document_t *doc) {
    return doc ? doc->title : "";
}

/* Get content type */
const char *silk_document_get_content_type(silk_document_t *doc) {
    return doc ? doc->content_type : "";
}

/* Get element by ID (stub for now) */
silk_element_t *silk_document_get_element_by_id(silk_document_t *doc,
                                                  const char *id) {
    if (!doc || !id)
        return NULL;

    /* TODO: Implement ID lookup
       - Traverse DOM
       - Match id attribute
       - Return element
    */
    return NULL;
}

/* Get root element */
silk_element_t *silk_document_get_root_element(silk_document_t *doc) {
    return doc && doc->root ? (silk_element_t *)doc->root : NULL;
}

/* Handle an event in the document */
void silk_document_handle_event(silk_document_t *doc, silk_event_t *event) {
    if (!doc || !event)
        return;

    /* TODO: Implement event handling
       - Route to appropriate element
       - Call event handlers
       - Update DOM if needed
       - Queue layout/render
    */
}

/* Execute JavaScript code (stub for Phase 4e) */
int silk_document_execute_script(silk_document_t *doc, const char *script,
                                  size_t script_len) {
    if (!doc || !script || script_len == 0)
        return -1;

    /* TODO: Implement JavaScript execution (Phase 4e)
       - Create Duktape context
       - Expose DOM API
       - Execute script
       - Handle errors
    */
    return -1;
}

/* Execute JavaScript from file */
int silk_document_execute_script_file(silk_document_t *doc,
                                       const char *filename) {
    if (!doc || !filename)
        return -1;

    /* Open file */
    FILE *f = fopen(filename, "rb");
    if (!f)
        return -1;

    /* Get file size */
    fseek(f, 0, SEEK_END);
    size_t size = ftell(f);
    fseek(f, 0, SEEK_SET);

    /* Allocate buffer */
    char *buffer = malloc(size);
    if (!buffer) {
        fclose(f);
        return -1;
    }

    /* Read file */
    if (fread(buffer, 1, size, f) != size) {
        free(buffer);
        fclose(f);
        return -1;
    }

    fclose(f);

    /* Execute script */
    int result = silk_document_execute_script(doc, buffer, size);
    free(buffer);

    return result;
}

/* Check if document is loaded */
int silk_document_is_loaded(silk_document_t *doc) {
    return doc ? doc->loaded : 0;
}

/* Check if document is rendering */
int silk_document_is_rendering(silk_document_t *doc) {
    if (!doc)
        return 0;
    /* TODO: Track rendering state during render pass */
    return 0;
}

/* Get horizontal scroll position */
int silk_document_get_scroll_x(silk_document_t *doc) {
    return doc ? doc->scroll_x : 0;
}

/* Get vertical scroll position */
int silk_document_get_scroll_y(silk_document_t *doc) {
    return doc ? doc->scroll_y : 0;
}

/* Set scroll position */
void silk_document_set_scroll(silk_document_t *doc, int x, int y) {
    if (doc) {
        doc->scroll_x = x;
        doc->scroll_y = y;
        /* TODO: Queue damage region for scroll changes */
    }
}

/* Get total element count */
int silk_document_element_count(silk_document_t *doc) {
    return doc ? doc->element_count : 0;
}

void silk_document_set_root_element(silk_document_t *doc, silk_dom_node_t *root) {
    if (doc) {
        doc->root = root;
        if (root) doc->loaded = 1;
    }
}

/* Get arena memory usage */
size_t silk_document_memory_used(silk_document_t *doc) {
    if (!doc || !doc->arena)
        return 0;
    return silk_arena_used(doc->arena);
}
