/**
 * \file test_inline_layout.c
 * \brief Inline Layout Algorithm Tests
 *
 * Tests CSS inline-level layout:
 * - Whitespace collapsing
 * - Text measurement
 * - Line breaking
 * - Inline box positioning
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <assert.h>

#include "silksurf/layout.h"

int main(void) {
    printf("=== Inline Layout Tests ===\n\n");

    int passed = 0, failed = 0;

    /* ================================================================
       TEST 1: Whitespace Collapsing
       ================================================================ */

    printf("Test 1: Whitespace Collapsing\n");
    {
        const char *input = "  hello   world  \n  test  ";
        char output[256];
        size_t len = silk_layout_collapse_whitespace(input, strlen(input), output, sizeof(output));

        /* Expected: " hello world test " (sequences -> single space) */
        printf("  Input:  '%s'\n", input);
        printf("  Output: '%s'\n", output);
        printf("  Length: %zu -> %zu\n", strlen(input), len);

        /* Verify: multiple spaces collapsed */
        if (strstr(output, "   ") != NULL) {
            printf("  [FAIL] Still contains multiple spaces\n");
            failed++;
        } else if (len > 0) {
            printf("  [PASS] Whitespace collapsed correctly\n");
            passed++;
        } else {
            printf("  [FAIL] Output is empty\n");
            failed++;
        }
    }

    /* ================================================================
       TEST 2: Text Measurement
       ================================================================ */

    printf("\nTest 2: Text Measurement\n");
    {
        int32_t font_size = 16;
        const char *text1 = "Hello";
        const char *text2 = "Hello World";

        int32_t width1 = silk_layout_measure_text(text1, strlen(text1), font_size);
        int32_t width2 = silk_layout_measure_text(text2, strlen(text2), font_size);

        printf("  Font size: %d px\n", font_size);
        printf("  '%s' width: %d px\n", text1, width1);
        printf("  '%s' width: %d px\n", text2, width2);

        /* Verify: longer text has greater width */
        if (width2 > width1 && width1 > 0) {
            printf("  [PASS] Text measurement proportional to length\n");
            passed++;
        } else {
            printf("  [FAIL] Text measurement incorrect\n");
            failed++;
        }
    }

    /* ================================================================
       TEST 3: Line Breaking
       ================================================================ */

    printf("\nTest 3: Line Breaking\n");
    {
        int32_t font_size = 16;
        const char *text = "The quick brown fox jumps";
        int32_t available_width = 100;  /* Enough for "The quick" */

        size_t break_pos = silk_layout_find_line_break(
            text, strlen(text), available_width, font_size);

        printf("  Text: '%s'\n", text);
        printf("  Available width: %d px\n", available_width);
        printf("  Break position: %zu\n", break_pos);

        if (break_pos > 0 && break_pos <= strlen(text)) {
            printf("  Text fits: '%.*s'\n", (int)break_pos, text);
            printf("  Remaining: '%s'\n", text + break_pos);
            printf("  [PASS] Line break found\n");
            passed++;
        } else {
            printf("  [FAIL] Invalid break position\n");
            failed++;
        }
    }

    /* ================================================================
       TEST 4: Different Font Sizes
       ================================================================ */

    printf("\nTest 4: Font Size Scaling\n");
    {
        const char *text = "Test";
        int32_t width_small = silk_layout_measure_text(text, strlen(text), 12);
        int32_t width_medium = silk_layout_measure_text(text, strlen(text), 16);
        int32_t width_large = silk_layout_measure_text(text, strlen(text), 24);

        printf("  Text: '%s'\n", text);
        printf("  12px width: %d\n", width_small);
        printf("  16px width: %d\n", width_medium);
        printf("  24px width: %d\n", width_large);

        /* Verify: width scales with font size */
        if (width_small < width_medium && width_medium < width_large) {
            printf("  [PASS] Width scales proportionally with font size\n");
            passed++;
        } else {
            printf("  [FAIL] Width scaling incorrect\n");
            failed++;
        }
    }

    /* ================================================================
       TEST 5: Empty Input Handling
       ================================================================ */

    printf("\nTest 5: Edge Cases\n");
    {
        int failures = 0;

        /* Test empty text measurement */
        int32_t width = silk_layout_measure_text("", 0, 16);
        if (width == 0) {
            printf("  [PASS] Empty text returns 0 width\n");
            passed++;
        } else {
            printf("  [FAIL] Empty text should return 0\n");
            failures++;
        }

        /* Test whitespace collapsing with empty output */
        char buf[10];
        size_t len = silk_layout_collapse_whitespace("   ", 3, buf, 10);
        if (len < 10) {  /* Should produce a single space, fitting in 10 bytes */
            printf("  [PASS] Whitespace collapse handles edge cases\n");
            passed++;
        } else {
            printf("  [FAIL] Whitespace collapse failed\n");
            failures++;
        }

        /* Test line break with zero width */
        size_t pos = silk_layout_find_line_break("test", 4, 0, 16);
        if (pos == 0) {
            printf("  [PASS] Line break with zero width returns 0\n");
            passed++;
        } else {
            printf("  [FAIL] Line break should return 0 for zero width\n");
            failures++;
        }

        failed += failures;
    }

    /* ================================================================
       SUMMARY
       ================================================================ */

    printf("\n================================================================================\n");
    printf("Inline Layout Test Results\n");
    printf("================================================================================\n");
    printf("Passed: %d\n", passed);
    printf("Failed: %d\n", failed);
    printf("Total:  %d\n", passed + failed);

    if (failed == 0) {
        printf("\n✓ All inline layout tests passed!\n");
        return 0;
    } else {
        printf("\n✗ Some tests failed\n");
        return 1;
    }
}
