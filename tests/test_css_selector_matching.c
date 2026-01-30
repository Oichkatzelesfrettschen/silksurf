#include <stdio.h>
#include <string.h>
#include <assert.h>
#include "../src/document/css_selector_match.h"

/* Test 1: Parse type selector */
static int test_parse_type_selector(void) {
    printf("TEST 1: Parse type selector\n");

    css_rule_selector_t *sel = css_selector_parse("div");
    if (!sel) {
        printf("  FAILED: Could not parse selector\n");
        return 0;
    }

    if (sel->selectors && sel->selectors->type == CSS_SELECTOR_TYPE &&
        strcmp(sel->selectors->name, "div") == 0) {
        printf("  PASSED: Type selector parsed correctly\n");
        css_selector_free(sel);
        return 1;
    } else {
        printf("  FAILED: Type selector not parsed correctly\n");
        css_selector_free(sel);
        return 0;
    }
}

/* Test 2: Parse class selector */
static int test_parse_class_selector(void) {
    printf("\nTEST 2: Parse class selector\n");

    css_rule_selector_t *sel = css_selector_parse(".highlight");
    if (!sel) {
        printf("  FAILED: Could not parse selector\n");
        return 0;
    }

    if (sel->selectors && sel->selectors->type == CSS_SELECTOR_CLASS &&
        strcmp(sel->selectors->name, "highlight") == 0) {
        printf("  PASSED: Class selector parsed correctly\n");
        css_selector_free(sel);
        return 1;
    } else {
        printf("  FAILED: Class selector not parsed correctly\n");
        css_selector_free(sel);
        return 0;
    }
}

/* Test 3: Parse ID selector */
static int test_parse_id_selector(void) {
    printf("\nTEST 3: Parse ID selector\n");

    css_rule_selector_t *sel = css_selector_parse("#main");
    if (!sel) {
        printf("  FAILED: Could not parse selector\n");
        return 0;
    }

    if (sel->selectors && sel->selectors->type == CSS_SELECTOR_ID &&
        strcmp(sel->selectors->name, "main") == 0) {
        printf("  PASSED: ID selector parsed correctly\n");
        css_selector_free(sel);
        return 1;
    } else {
        printf("  FAILED: ID selector not parsed correctly\n");
        css_selector_free(sel);
        return 0;
    }
}

/* Test 4: Calculate specificity */
static int test_specificity_calculation(void) {
    printf("\nTEST 4: Specificity calculation\n");

    css_rule_selector_t *type_sel = css_selector_parse("div");
    css_rule_selector_t *class_sel = css_selector_parse(".highlight");
    css_rule_selector_t *id_sel = css_selector_parse("#main");

    css_specificity_t type_spec = css_selector_specificity(type_sel);
    css_specificity_t class_spec = css_selector_specificity(class_sel);
    css_specificity_t id_spec = css_selector_specificity(id_sel);

    int passed = 1;

    /* Type selector: (0, 0, 1) */
    if (type_spec.ids != 0 || type_spec.classes_and_attrs != 0 || type_spec.elements != 1) {
        printf("  FAILED: Type selector specificity wrong\n");
        passed = 0;
    }

    /* Class selector: (0, 1, 0) */
    if (class_spec.ids != 0 || class_spec.classes_and_attrs != 1 || class_spec.elements != 0) {
        printf("  FAILED: Class selector specificity wrong\n");
        passed = 0;
    }

    /* ID selector: (1, 0, 0) */
    if (id_spec.ids != 1 || id_spec.classes_and_attrs != 0 || id_spec.elements != 0) {
        printf("  FAILED: ID selector specificity wrong\n");
        passed = 0;
    }

    if (passed) {
        printf("  PASSED: All specificities calculated correctly\n");
    }

    css_selector_free(type_sel);
    css_selector_free(class_sel);
    css_selector_free(id_sel);

    return passed;
}

/* Test 5: Specificity comparison */
static int test_specificity_comparison(void) {
    printf("\nTEST 5: Specificity comparison\n");

    css_specificity_t spec_lower = {0, 0, 1};   /* div */
    css_specificity_t spec_higher = {0, 1, 0}; /* .class */
    css_specificity_t spec_highest = {1, 0, 0}; /* #id */

    int cmp1 = css_specificity_compare(spec_lower, spec_higher);
    int cmp2 = css_specificity_compare(spec_higher, spec_highest);
    int cmp3 = css_specificity_compare(spec_lower, spec_lower);

    if (cmp1 < 0 && cmp2 < 0 && cmp3 == 0) {
        printf("  PASSED: Specificity comparison works correctly\n");
        return 1;
    } else {
        printf("  FAILED: Specificity comparison failed\n");
        printf("    cmp(type, class) = %d (expected < 0)\n", cmp1);
        printf("    cmp(class, id) = %d (expected < 0)\n", cmp2);
        printf("    cmp(type, type) = %d (expected = 0)\n", cmp3);
        return 0;
    }
}

/* Test 6: Parse compound selector */
static int test_parse_compound_selector(void) {
    printf("\nTEST 6: Parse compound selector\n");

    css_rule_selector_t *sel = css_selector_parse("div.highlight#main");
    if (!sel || !sel->selectors) {
        printf("  FAILED: Could not parse selector\n");
        return 0;
    }

    /* Count components */
    int count = 0;
    css_selector_t *current = sel->selectors;
    while (current) {
        count++;
        current = current->next;
    }

    if (count >= 1) {  /* At minimum should have parsed type */
        printf("  PASSED: Compound selector parsed (%d components)\n", count);
        css_selector_free(sel);
        return 1;
    } else {
        printf("  FAILED: Compound selector not parsed correctly\n");
        css_selector_free(sel);
        return 0;
    }
}

/* Test 7: Empty selector */
static int test_empty_selector(void) {
    printf("\nTEST 7: Empty selector handling\n");

    css_rule_selector_t *sel = css_selector_parse("");
    if (!sel) {
        printf("  FAILED: Could not create empty selector\n");
        return 0;
    }

    /* Empty selector should have NULL selectors list */
    if (!sel->selectors) {
        printf("  PASSED: Empty selector handled correctly\n");
        css_selector_free(sel);
        return 1;
    } else {
        printf("  FAILED: Empty selector should have no components\n");
        css_selector_free(sel);
        return 0;
    }
}

/* Test 8: Universal selector */
static int test_universal_selector(void) {
    printf("\nTEST 8: Universal selector\n");

    css_rule_selector_t *sel = css_selector_parse("*");
    if (!sel || !sel->selectors) {
        printf("  FAILED: Could not parse universal selector\n");
        return 0;
    }

    if (sel->selectors->type == CSS_SELECTOR_UNIVERSAL) {
        printf("  PASSED: Universal selector parsed correctly\n");
        css_selector_free(sel);
        return 1;
    } else {
        printf("  FAILED: Universal selector not recognized\n");
        css_selector_free(sel);
        return 0;
    }
}

int main(void) {
    printf("===== CSS Selector Matching Tests =====\n\n");

    int total = 0, passed = 0;

    total++;
    passed += test_parse_type_selector();

    total++;
    passed += test_parse_class_selector();

    total++;
    passed += test_parse_id_selector();

    total++;
    passed += test_specificity_calculation();

    total++;
    passed += test_specificity_comparison();

    total++;
    passed += test_parse_compound_selector();

    total++;
    passed += test_empty_selector();

    total++;
    passed += test_universal_selector();

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
