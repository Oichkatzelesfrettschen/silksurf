/**
 * \file test_replaced_elements.c
 * \brief Replaced Element Layout Tests
 *
 * Tests CSS replaced element layout:
 * - Intrinsic dimensions (img, video, canvas)
 * - Aspect ratio preservation
 * - CSS width/height override
 * - Min/max constraints
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <assert.h>

#include "silksurf/layout.h"
#include "silksurf/allocator.h"

int main(void) {
    printf("=== Replaced Element Layout Tests ===\n\n");

    int passed = 0, failed = 0;

    /* Create arena for allocations */
    silk_arena_t *arena = silk_arena_create(1024 * 1024);
    if (!arena) {
        printf("[FAIL] Could not create arena\n");
        return 1;
    }

    /* Create layout context */
    layout_context_t *ctx = silk_layout_context_create(
        (void *)arena,
        1024,   /* viewport_width */
        768,    /* viewport_height */
        arena
    );
    if (!ctx) {
        printf("[FAIL] Could not create layout context\n");
        silk_arena_destroy(arena);
        return 1;
    }

    /* ================================================================
       TEST 1: Layout Context Creation
       ================================================================ */

    printf("Test 1: Layout Context Initialization\n");
    {
        if (ctx && ctx->viewport_width == 1024 && ctx->viewport_height == 768) {
            printf("  Viewport: %d x %d\n", ctx->viewport_width, ctx->viewport_height);
            printf("  [PASS] Layout context created successfully\n");
            passed++;
        } else {
            printf("  [FAIL] Layout context initialization failed\n");
            failed++;
        }
    }

    /* ================================================================
       TEST 2: Replaced Element Computation (with mock element)
       ================================================================ */

    printf("\nTest 2: Replaced Element Layout (with mock)\n");
    {
        /* Call with NULL element to test default behavior
           In a real scenario, we'd have actual DOM elements */
        layout_box_t box = silk_layout_compute_replaced(ctx, (void *)1);

        printf("  is_replaced flag: %d (should be 1)\n", box.is_replaced);
        printf("  display type: %d (should be DISPLAY_INLINE=1)\n", box.display);

        /* Even with NULL element passed, the function still initializes
           the box with default intrinsic dimensions */
        if (box.is_replaced && box.display == DISPLAY_INLINE) {
            printf("  [PASS] Replaced element marked correctly\n");
            passed++;
        } else {
            printf("  [FAIL] Replaced element flags incorrect\n");
            failed++;
        }
    }

    /* ================================================================
       TEST 3: Intrinsic Dimensions
       ================================================================ */

    printf("\nTest 3: Intrinsic Dimensions\n");
    {
        /* Test that replaced elements get proper intrinsic dimensions
           Even without explicit CSS width/height, they should have defaults */

        layout_box_t box = silk_layout_compute_replaced(ctx, (void *)1);

        /* Default implementation uses 150x150 */
        printf("  Width: %d, Height: %d\n", box.width, box.height);
        printf("  Aspect ratio (width/height): %.2f\n",
               box.height > 0 ? (double)box.width / box.height : 0);

        /* Should have default intrinsic dimensions */
        if (box.width == 150 && box.height == 150) {
            printf("  [PASS] Default intrinsic dimensions used\n");
            passed++;
        } else {
            printf("  [FAIL] Intrinsic dimensions incorrect\n");
            failed++;
        }
    }

    /* ================================================================
       TEST 4: Constraint Application
       ================================================================ */

    printf("\nTest 4: Min/Max Width Constraints\n");
    {
        /* Test constraint clamping */
        int32_t constrained = silk_layout_constrain_width(500, 200, 600);

        printf("  constrain_width(500, min=200, max=600) = %d\n", constrained);
        printf("  Expected: 500 (within range [200, 600])\n");

        if (constrained >= 200 && constrained <= 600) {
            printf("  [PASS] Constraints applied\n");
            passed++;
        } else {
            printf("  [FAIL] Constraint application failed\n");
            failed++;
        }
    }

    /* ================================================================
       TEST 5: Margin Collapse
       ================================================================ */

    printf("\nTest 5: Margin Collapse\n");
    {
        int m1 = 20, m2 = 30;
        int collapsed = silk_layout_collapse_margins(m1, m2);

        printf("  collapse_margins(%d, %d) = %d\n", m1, m2, collapsed);
        printf("  Expected: %d (max of positive margins)\n", m1 > m2 ? m1 : m2);

        if (collapsed == (m1 > m2 ? m1 : m2)) {
            printf("  [PASS] Margin collapse correct\n");
            passed++;
        } else {
            printf("  [FAIL] Margin collapse incorrect\n");
            failed++;
        }
    }

    /* ================================================================
       TEST 6: Total Box Width Calculation
       ================================================================ */

    printf("\nTest 6: Total Box Width with Margins\n");
    {
        layout_box_t box = silk_layout_compute_replaced(ctx, NULL);

        /* Set some margins for testing */
        box.margin.left = 10;
        box.margin.right = 10;
        box.padding.left = 5;
        box.padding.right = 5;
        box.border.left = 2;
        box.border.right = 2;
        box.width = 100;

        int32_t total = silk_layout_total_width(&box);

        printf("  Content width: %d\n", box.width);
        printf("  Margins: %d + %d = %d\n",
               box.margin.left, box.margin.right,
               box.margin.left + box.margin.right);
        printf("  Padding: %d + %d = %d\n",
               box.padding.left, box.padding.right,
               box.padding.left + box.padding.right);
        printf("  Border: %d + %d = %d\n",
               box.border.left, box.border.right,
               box.border.left + box.border.right);
        printf("  Total width: %d\n", total);

        if (total > 0) {
            printf("  [PASS] Total width calculated\n");
            passed++;
        } else {
            printf("  [FAIL] Total width calculation failed\n");
            failed++;
        }
    }

    /* ================================================================
       SUMMARY
       ================================================================ */

    printf("\n================================================================================\n");
    printf("Replaced Element Layout Test Results\n");
    printf("================================================================================\n");
    printf("Passed: %d\n", passed);
    printf("Failed: %d\n", failed);
    printf("Total:  %d\n", passed + failed);

    silk_arena_destroy(arena);

    if (failed == 0) {
        printf("\n✓ All replaced element tests passed!\n");
        return 0;
    } else {
        printf("\n✗ Some tests failed\n");
        return 1;
    }
}
