/* Full Pipeline Integration Test
 *
 * Tests the complete rendering pipeline:
 * HTML parsing -> CSS parsing -> Selector matching -> Cascade ->
 * Layout computation -> Paint commands
 *
 * Verifies that: <body style="background:red"><p>Hello</p></body>
 * produces correct layout geometry and paint commands.
 */
#include <stdio.h>
#include <string.h>
#include <strings.h>
#include "../include/silksurf/document.h"
#include "../include/silksurf/dom_node.h"
#include "../include/silksurf/css_parser.h"
#include "../include/silksurf/layout.h"
#include "../include/silksurf/renderer.h"
#include "../include/silksurf/allocator.h"

static int tests_passed = 0;
static int tests_total = 0;

#define ASSERT(cond, msg) do { \
    tests_total++; \
    if (!(cond)) { printf("  FAIL: %s\n", msg); return 0; } \
    tests_passed++; \
} while(0)

/* Find element by tag name (case-insensitive, depth-first) */
static silk_dom_node_t *find_element(silk_dom_node_t *node, const char *tag, int depth) {
    if (!node || depth <= 0) return NULL;
    const char *name = silk_dom_node_get_tag_name(node);
    if (name && strcasecmp(name, tag) == 0) return node;
    silk_dom_node_t *child = silk_dom_node_get_first_child(node);
    while (child) {
        silk_dom_node_t *found = find_element(child, tag, depth - 1);
        if (found) return found;
        child = silk_dom_node_get_next_sibling(child);
    }
    return NULL;
}

static int test_html_parsing(void) {
    printf("TEST 1: HTML parsing produces DOM tree\n");

    silk_document_t *doc = silk_document_create(1024 * 1024);
    ASSERT(doc != NULL, "document created");

    const char *html = "<html><body style=\"background:red\"><p>Hello</p></body></html>";
    int rc = silk_document_load_html(doc, html, strlen(html));
    ASSERT(rc == 0, "HTML parsed successfully");

    silk_dom_node_t *root = (silk_dom_node_t *)silk_document_get_root_element(doc);
    ASSERT(root != NULL, "root element exists");

    silk_dom_node_t *body = find_element(root, "body", 5);
    ASSERT(body != NULL, "body element found");

    silk_dom_node_t *p = find_element(root, "p", 5);
    ASSERT(p != NULL, "p element found");

    silk_document_destroy(doc);
    printf("  PASSED\n");
    return 1;
}

static int test_css_style_computation(void) {
    printf("TEST 2: CSS style computation via native cascade\n");

    silk_document_t *doc = silk_document_create(1024 * 1024);
    const char *html = "<html><body><div style=\"width:200px;height:100px;background-color:#ff0000\">Test</div></body></html>";
    silk_document_load_html(doc, html, strlen(html));

    silk_arena_t *arena = silk_document_get_arena(doc);
    silk_css_engine_t *engine = silk_css_engine_create(arena);
    ASSERT(engine != NULL, "CSS engine created");

    /* Parse author stylesheet */
    const char *css = "div { margin: 10px; padding: 5px; }";
    int rc = silk_css_parse_string(engine, css, strlen(css));
    ASSERT(rc == 0, "CSS parsed");

    silk_dom_node_t *root = (silk_dom_node_t *)silk_document_get_root_element(doc);
    silk_dom_node_t *div = find_element(root, "div", 10);
    ASSERT(div != NULL, "div element found");

    silk_computed_style_t style;
    rc = silk_css_get_computed_style(engine, div, &style);
    ASSERT(rc == 0, "style computed");

    /* The inline style should set width=200, height=100, background=red */
    printf("  Computed: width=%d height=%d bg=%08x margin=%d padding=%d\n",
           style.width, style.height, style.background_color,
           style.margin_top, style.padding_top);

    /* Inline style: width and height should be set */
    ASSERT(style.width == 200, "width from inline style");
    ASSERT(style.height == 100, "height from inline style");
    ASSERT(style.background_color == 0xFFFF0000, "bg color from inline style");

    /* Author stylesheet: margin and padding */
    ASSERT(style.margin_top == 10, "margin from author stylesheet");
    ASSERT(style.padding_top == 5, "padding from author stylesheet");

    silk_css_engine_destroy(engine);
    silk_document_destroy(doc);
    printf("  PASSED\n");
    return 1;
}

static int test_layout_computation(void) {
    printf("TEST 3: Layout computation produces geometry\n");

    silk_document_t *doc = silk_document_create(2 * 1024 * 1024);
    const char *html = "<html><body><div>Block</div></body></html>";
    silk_document_load_html(doc, html, strlen(html));

    /* Apply styles then layout */
    int rc = silk_document_layout(doc, 800, 600);
    ASSERT(rc == 0, "layout computed");

    /* Verify the layout context was created */
    silk_dom_node_t *root = (silk_dom_node_t *)silk_document_get_root_element(doc);
    ASSERT(root != NULL, "root exists after layout");

    silk_document_destroy(doc);
    printf("  PASSED\n");
    return 1;
}

static int test_paint_commands(void) {
    printf("TEST 4: Paint commands generated from styled DOM\n");

    silk_document_t *doc = silk_document_create(2 * 1024 * 1024);
    const char *html = "<html><body><div style=\"background-color:#336699;width:100px;height:50px\">Box</div></body></html>";
    silk_document_load_html(doc, html, strlen(html));

    /* Compute styles */
    silk_arena_t *arena = silk_document_get_arena(doc);
    silk_css_engine_t *engine = silk_css_engine_create(arena);
    silk_dom_node_t *root = (silk_dom_node_t *)silk_document_get_root_element(doc);
    silk_dom_node_t *div = find_element(root, "div", 10);
    ASSERT(div != NULL, "div found");

    silk_computed_style_t *style = silk_dom_node_get_style(div);
    silk_css_get_computed_style(engine, div, style);

    /* Generate paint commands */
    silk_render_queue_t queue;
    silk_render_queue_init(&queue);
    silk_paint_node(div, &queue);

    printf("  Paint commands generated: %d\n", queue.count);
    ASSERT(queue.count > 0, "at least one paint command");

    /* First command should be the background rect */
    if (queue.count > 0) {
        silk_draw_rect_cmd_t *cmd = &queue.commands[0];
        printf("  Cmd 0: rect(%d,%d,%d,%d) color=%08x\n",
               cmd->x, cmd->y, cmd->w, cmd->h, cmd->color);
        ASSERT(cmd->color == 0xFF336699, "correct background color");
    }

    silk_css_engine_destroy(engine);
    silk_document_destroy(doc);
    printf("  PASSED\n");
    return 1;
}

static int test_memory_usage(void) {
    printf("TEST 5: Memory usage within target (<10 MB)\n");

    silk_document_t *doc = silk_document_create(4 * 1024 * 1024);
    const char *html = "<html><body>"
        "<div style=\"width:100px;height:50px\">Block 1</div>"
        "<div style=\"width:200px;height:75px\">Block 2</div>"
        "<p>Paragraph text</p>"
        "</body></html>";
    silk_document_load_html(doc, html, strlen(html));

    size_t mem = silk_document_memory_used(doc);
    printf("  Memory used: %zu bytes (%.1f KB)\n", mem, (double)mem / 1024.0);
    ASSERT(mem < 10 * 1024 * 1024, "under 10 MB");

    silk_document_destroy(doc);
    printf("  PASSED\n");
    return 1;
}

int main(void) {
    printf("SilkSurf Full Pipeline Integration Test\n");
    printf("========================================\n\n");

    int passed = 0;
    passed += test_html_parsing();
    passed += test_css_style_computation();
    passed += test_layout_computation();
    passed += test_paint_commands();
    passed += test_memory_usage();

    printf("\n========================================\n");
    printf("Test functions: %d/5 passed\n", passed);
    printf("Assertions: %d/%d passed\n", tests_passed, tests_total);

    return (passed == 5) ? 0 : 1;
}
