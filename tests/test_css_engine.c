#include <stdio.h>
#include <string.h>
#include "../include/silksurf/css_parser.h"
#include "../include/silksurf/allocator.h"

/* Test 1: Create and destroy CSS engine */
static int test_engine_lifecycle(void) {
    printf("TEST 1: CSS engine lifecycle\n");

    silk_arena_t *arena = silk_arena_create(1024 * 1024);
    if (!arena) {
        printf("  FAILED: Could not create arena\n");
        return 0;
    }

    silk_css_engine_t *engine = silk_css_engine_create(arena);
    if (!engine) {
        printf("  FAILED: Could not create CSS engine\n");
        silk_arena_destroy(arena);
        return 0;
    }

    printf("  PASSED: CSS engine created\n");

    silk_css_engine_destroy(engine);
    silk_arena_destroy(arena);

    printf("  PASSED: CSS engine destroyed cleanly\n");
    return 1;
}

/* Test 2: Parse simple CSS */
static int test_css_parsing(void) {
    printf("\nTEST 2: CSS parsing\n");

    silk_arena_t *arena = silk_arena_create(1024 * 1024);
    silk_css_engine_t *engine = silk_css_engine_create(arena);

    if (!engine) {
        printf("  FAILED: Could not create engine\n");
        silk_arena_destroy(arena);
        return 0;
    }

    const char *css = "body { color: red; font-size: 16px; }";
    printf("  CSS: %s\n", css);

    if (silk_css_parse_string(engine, css, strlen(css)) < 0) {
        printf("  FAILED: Could not parse CSS\n");
        silk_css_engine_destroy(engine);
        silk_arena_destroy(arena);
        return 0;
    }

    printf("  PASSED: CSS parsed successfully\n");

    silk_css_engine_destroy(engine);
    silk_arena_destroy(arena);
    return 1;
}

/* Test 3: Parse multiple CSS rules */
static int test_multiple_rules(void) {
    printf("\nTEST 3: Multiple CSS rules\n");

    silk_arena_t *arena = silk_arena_create(1024 * 1024);
    silk_css_engine_t *engine = silk_css_engine_create(arena);

    if (!engine) {
        printf("  FAILED: Could not create engine\n");
        silk_arena_destroy(arena);
        return 0;
    }

    const char *css =
        "div { width: 100px; height: 50px; }\n"
        ".container { margin: 10px; padding: 5px; }\n"
        "#main { background-color: blue; }";

    printf("  Parsing CSS with multiple selectors...\n");

    if (silk_css_parse_string(engine, css, strlen(css)) < 0) {
        printf("  FAILED: Could not parse CSS\n");
        silk_css_engine_destroy(engine);
        silk_arena_destroy(arena);
        return 0;
    }

    printf("  PASSED: Multiple CSS rules parsed\n");

    silk_css_engine_destroy(engine);
    silk_arena_destroy(arena);
    return 1;
}

/* Test 4: Parse CSS with comments */
static int test_css_with_comments(void) {
    printf("\nTEST 4: CSS with comments\n");

    silk_arena_t *arena = silk_arena_create(1024 * 1024);
    silk_css_engine_t *engine = silk_css_engine_create(arena);

    if (!engine) {
        printf("  FAILED: Could not create engine\n");
        silk_arena_destroy(arena);
        return 0;
    }

    const char *css =
        "/* This is a comment */\n"
        "body {\n"
        "    /* Set text color */\n"
        "    color: black;\n"
        "}\n"
        "/* End of CSS */";

    printf("  Parsing CSS with comments...\n");

    if (silk_css_parse_string(engine, css, strlen(css)) < 0) {
        printf("  FAILED: Could not parse CSS\n");
        silk_css_engine_destroy(engine);
        silk_arena_destroy(arena);
        return 0;
    }

    printf("  PASSED: CSS with comments parsed\n");

    silk_css_engine_destroy(engine);
    silk_arena_destroy(arena);
    return 1;
}

/* Main test runner */
int main(void) {
    printf("SilkSurf CSS Engine Test Suite\n");
    printf("===============================\n\n");

    int passed = 0;
    int total = 4;

    if (test_engine_lifecycle())
        passed++;

    if (test_css_parsing())
        passed++;

    if (test_multiple_rules())
        passed++;

    if (test_css_with_comments())
        passed++;

    printf("\n===============================\n");
    printf("Results: %d/%d tests passed\n", passed, total);

    return (passed == total) ? 0 : 1;
}
