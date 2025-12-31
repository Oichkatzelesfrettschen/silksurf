#include <limits.h>
#include <stdbool.h>
#include <stdio.h>

/* 1. The Data Structure (Constrained Resource Friendly) */
typedef struct {
    int content_width;
    int padding_left;
    int padding_right;
    int border_left;
    int border_right;
    int margin_left;
    int margin_right;
} SilkBox;

/* 2. The Logic (With UBSan Traps) */
bool silk_layout_calculate_total_width(SilkBox *b, int *total_out) {
    /* We use long long to detect overflow before casting back to int */
    long long total = (long long)b->margin_left + b->border_left + b->padding_left +
                      b->content_width +
                      b->padding_right + b->border_right + b->margin_right;

    /* 3. The Constraint Check */
    if (total > INT_MAX || total < 0) {
        return false; /* Result cannot fit in 'int' or is invalid */
    }

    *total_out = (int)total;
    return true;
}

/* 4. The Harness */
int main() {
    printf("Silksurf Layout Engine: Box Model First Light\n");
    
    /* Test Case 1: Normal Box */
    SilkBox b1 = { .content_width = 100, .padding_left = 10, .padding_right = 10 };
    int total;
    if (silk_layout_calculate_total_width(&b1, &total)) {
        printf("Normal Box Total Width: %d (Expected 120)\n", total);
    }

    /* Test Case 2: Deliberate Overflow */
    printf("\nSimulating extreme overflow (Long Path)...\n");
    SilkBox b2 = { .content_width = INT_MAX - 5, .padding_left = 100 }; 
    
    if (silk_layout_calculate_total_width(&b2, &total)) {
        printf("Error: This should have failed but returned: %d\n", total);
    } else {
        printf("Success: Layout Overflow Detected and Handled Safely via checked math.\n");
    }

    /* Test Case 3: Proving UBSan works */
    printf("\nProving UBSan can detect direct overflows...\n");
    int overflow_test = INT_MAX - 5;
    printf("Note: The following should print a 'runtime error' if UBSan is active:\n");
    
    /* Direct addition to trigger sanitizer */
    int direct_add = overflow_test + 100;
    printf("Result (should not see this if UBSan traps): %d\n", direct_add);

    return 0;
}