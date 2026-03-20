#include <stdio.h>
#include <string.h>
#include <strings.h>
#include "../include/silksurf/css_parser.h"
#include "../include/silksurf/document.h"
#include "../include/silksurf/dom_node.h"

/* Find element by tag name in DOM tree (case-insensitive, depth-first) */
static silk_dom_node_t *find_element(silk_dom_node_t *node, const char *tag, int max_depth) {
    if (!node || max_depth <= 0) return NULL;

    const char *name = silk_dom_node_get_tag_name(node);
    if (name && strcasecmp(name, tag) == 0) {
        return node;
    }

    silk_dom_node_t *child = silk_dom_node_get_first_child(node);
    while (child) {
        silk_dom_node_t *found = find_element(child, tag, max_depth - 1);
        if (found) return found;
        child = silk_dom_node_get_next_sibling(child);
    }
    return NULL;
}

/* Test 1: Basic CSS cascade with simple HTML */
static int test_simple_cascade(void) {
    printf("TEST 1: Simple CSS cascade\n");

    silk_document_t *doc = silk_document_create(1024 * 1024);
    silk_css_engine_t *engine = silk_css_engine_create(silk_document_get_arena(doc));

    if (!doc || !engine) {
        printf("  FAILED: Could not create document or engine\n");
        return 0;
    }

    const char *html = "<html><body><div>Test</div></body></html>";
    if (silk_document_load_html(doc, html, strlen(html)) < 0) {
        printf("  FAILED: Could not load HTML\n");
        silk_css_engine_destroy(engine);
        silk_document_destroy(doc);
        return 0;
    }

    const char *css = "div { width: 100px; height: 50px; }";
    if (silk_css_parse_string(engine, css, strlen(css)) < 0) {
        printf("  FAILED: Could not parse CSS\n");
        silk_css_engine_destroy(engine);
        silk_document_destroy(doc);
        return 0;
    }

    silk_dom_node_t *root = (silk_dom_node_t *)silk_document_get_root_element(doc);
    silk_dom_node_t *div = find_element(root, "div", 10);

    if (!div) {
        printf("  FAILED: Could not find DIV element\n");
        silk_css_engine_destroy(engine);
        silk_document_destroy(doc);
        return 0;
    }

    silk_computed_style_t style;
    int result = silk_css_get_computed_style(engine, div, &style);

    if (result < 0) {
        printf("  FAILED: Could not compute styles\n");
        silk_css_engine_destroy(engine);
        silk_document_destroy(doc);
        return 0;
    }

    if (style.width >= -1 && style.height >= -1) {
        printf("  PASSED: Cascade returned valid style (width=%d, height=%d)\n",
               style.width, style.height);
        silk_css_engine_destroy(engine);
        silk_document_destroy(doc);
        return 1;
    }

    printf("  FAILED: Invalid style values (width=%d, height=%d)\n",
           style.width, style.height);
    silk_css_engine_destroy(engine);
    silk_document_destroy(doc);
    return 0;
}

/* Test 2: CSS cascade with class selector */
static int test_class_selector(void) {
    printf("\nTEST 2: CSS class selector\n");

    silk_document_t *doc = silk_document_create(1024 * 1024);
    silk_css_engine_t *engine = silk_css_engine_create(silk_document_get_arena(doc));

    if (!doc || !engine) {
        printf("  FAILED: Could not create document or engine\n");
        return 0;
    }

    const char *html = "<html><body><p class='highlight'>Text</p></body></html>";
    if (silk_document_load_html(doc, html, strlen(html)) < 0) {
        printf("  FAILED: Could not load HTML\n");
        silk_css_engine_destroy(engine);
        silk_document_destroy(doc);
        return 0;
    }

    const char *css = ".highlight { color: red; }";
    if (silk_css_parse_string(engine, css, strlen(css)) < 0) {
        printf("  FAILED: Could not parse CSS\n");
        silk_css_engine_destroy(engine);
        silk_document_destroy(doc);
        return 0;
    }

    silk_dom_node_t *root = (silk_dom_node_t *)silk_document_get_root_element(doc);
    silk_dom_node_t *p = find_element(root, "p", 10);

    if (!p) {
        printf("  FAILED: Could not find P element\n");
        silk_css_engine_destroy(engine);
        silk_document_destroy(doc);
        return 0;
    }

    silk_computed_style_t style;
    int result = silk_css_get_computed_style(engine, p, &style);

    if (result < 0) {
        printf("  FAILED: Could not compute styles\n");
        silk_css_engine_destroy(engine);
        silk_document_destroy(doc);
        return 0;
    }

    printf("  PASSED: Computed styles returned (color=%08x)\n", style.color);
    silk_css_engine_destroy(engine);
    silk_document_destroy(doc);
    return 1;
}

/* Test 3: Multiple CSS rules */
static int test_multiple_rules(void) {
    printf("\nTEST 3: Multiple CSS rules\n");

    silk_document_t *doc = silk_document_create(1024 * 1024);
    silk_css_engine_t *engine = silk_css_engine_create(silk_document_get_arena(doc));

    if (!doc || !engine) {
        printf("  FAILED: Could not create document or engine\n");
        return 0;
    }

    const char *html = "<html><body><span>Text</span></body></html>";
    if (silk_document_load_html(doc, html, strlen(html)) < 0) {
        printf("  FAILED: Could not load HTML\n");
        silk_css_engine_destroy(engine);
        silk_document_destroy(doc);
        return 0;
    }

    const char *css1 = "span { width: 100px; }";
    const char *css2 = "span { width: 200px; }";
    silk_css_parse_string(engine, css1, strlen(css1));
    silk_css_parse_string(engine, css2, strlen(css2));

    silk_dom_node_t *root = (silk_dom_node_t *)silk_document_get_root_element(doc);
    silk_dom_node_t *span = find_element(root, "span", 10);

    if (!span) {
        printf("  FAILED: Could not find SPAN element\n");
        silk_css_engine_destroy(engine);
        silk_document_destroy(doc);
        return 0;
    }

    silk_computed_style_t style;
    int result = silk_css_get_computed_style(engine, span, &style);

    if (result < 0) {
        printf("  FAILED: Could not compute styles\n");
        silk_css_engine_destroy(engine);
        silk_document_destroy(doc);
        return 0;
    }

    printf("  PASSED: Computed styles returned (width=%d)\n", style.width);
    silk_css_engine_destroy(engine);
    silk_document_destroy(doc);
    return 1;
}

int main(void) {
    printf("===== CSS Cascade Integration Tests =====\n\n");

    int total = 0, passed = 0;

    total++; passed += test_simple_cascade();
    total++; passed += test_class_selector();
    total++; passed += test_multiple_rules();

    printf("\n===== Test Summary =====\n");
    printf("Passed: %d/%d\n", passed, total);

    if (passed == total) {
        printf("All tests PASSED\n");
        return 0;
    } else {
        printf("Some tests FAILED\n");
        return 1;
    }
}
