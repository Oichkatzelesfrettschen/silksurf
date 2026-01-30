#include <stdio.h>
#include <string.h>
#include <assert.h>
#include "../src/document/css_cascade.h"

/* Test 1: Basic cascade with single rule */
static int test_basic_cascade(void) {
    printf("TEST 1: Basic cascade with single rule\n");

    css_computed_style computed;
    css_cascade_context ctx = {0};

    /* Create a simple matched rule */
    css_rule rule = {0};
    rule.properties[0].id = CSS_PROP_WIDTH;
    rule.properties[0].value.length.value = INTTOFIX(100);
    rule.properties[0].value.length.unit = CSS_UNIT_PX;
    rule.property_count = 1;

    ctx.matched_rules = &rule;
    ctx.matched_rule_count = 1;

    uint16_t specificity = 1;
    css_origin origin = CSS_ORIGIN_AUTHOR;

    ctx.specificities = &specificity;
    ctx.origins = &origin;

    /* Run cascade */
    css_error err = css_cascade_for_element(&ctx, &computed);
    if (err != CSS_OK) {
        printf("  FAILED: Cascade returned error %d\n", err);
        return 0;
    }

    /* Verify width was set */
    if (computed.values[CSS_PROP_WIDTH].length.value == INTTOFIX(100) &&
        computed.values[CSS_PROP_WIDTH].length.unit == CSS_UNIT_PX) {
        printf("  PASSED: Width set correctly (100px)\n");
        return 1;
    } else {
        printf("  FAILED: Width not set correctly\n");
        return 0;
    }
}

/* Test 2: Cascade order - specificity */
static int test_cascade_specificity(void) {
    printf("\nTEST 2: Cascade specificity (higher specificity wins)\n");

    css_computed_style computed;
    css_cascade_context ctx = {0};

    /* Create two rules: first is less specific, second is more specific */
    css_rule rules[2] = {0};

    /* Rule 1: width: 100px (specificity 1) */
    rules[0].properties[0].id = CSS_PROP_WIDTH;
    rules[0].properties[0].value.length.value = INTTOFIX(100);
    rules[0].properties[0].value.length.unit = CSS_UNIT_PX;
    rules[0].property_count = 1;

    /* Rule 2: width: 200px (specificity 2 - higher) */
    rules[1].properties[0].id = CSS_PROP_WIDTH;
    rules[1].properties[0].value.length.value = INTTOFIX(200);
    rules[1].properties[0].value.length.unit = CSS_UNIT_PX;
    rules[1].property_count = 1;

    ctx.matched_rules = rules;
    ctx.matched_rule_count = 2;

    uint16_t specificities[] = {1, 2};
    css_origin origins[] = {CSS_ORIGIN_AUTHOR, CSS_ORIGIN_AUTHOR};

    ctx.specificities = specificities;
    ctx.origins = origins;

    /* Run cascade */
    css_error err = css_cascade_for_element(&ctx, &computed);
    if (err != CSS_OK) {
        printf("  FAILED: Cascade returned error\n");
        return 0;
    }

    /* Verify higher specificity rule won */
    if (computed.values[CSS_PROP_WIDTH].length.value == INTTOFIX(200)) {
        printf("  PASSED: Higher specificity rule won (200px)\n");
        return 1;
    } else {
        printf("  FAILED: Wrong rule won (expected 200px, got %d)\n",
               FIXTOINT(computed.values[CSS_PROP_WIDTH].length.value));
        return 0;
    }
}

/* Test 3: Cascade origin - !important wins */
static int test_cascade_origin(void) {
    printf("\nTEST 3: Cascade origin (!important wins)\n");

    css_computed_style computed;
    css_cascade_context ctx = {0};

    css_rule rules[2] = {0};

    /* Rule 1: width: 100px (author normal) */
    rules[0].properties[0].id = CSS_PROP_WIDTH;
    rules[0].properties[0].value.length.value = INTTOFIX(100);
    rules[0].properties[0].value.length.unit = CSS_UNIT_PX;
    rules[0].property_count = 1;

    /* Rule 2: width: 50px (!important) */
    rules[1].properties[0].id = CSS_PROP_WIDTH;
    rules[1].properties[0].value.length.value = INTTOFIX(50);
    rules[1].properties[0].value.length.unit = CSS_UNIT_PX;
    rules[1].property_count = 1;

    ctx.matched_rules = rules;
    ctx.matched_rule_count = 2;

    uint16_t specificities[] = {1, 1};
    css_origin origins[] = {CSS_ORIGIN_AUTHOR, CSS_ORIGIN_AUTHOR_IMPORTANT};

    ctx.specificities = specificities;
    ctx.origins = origins;

    /* Run cascade */
    css_error err = css_cascade_for_element(&ctx, &computed);
    if (err != CSS_OK) {
        printf("  FAILED: Cascade returned error\n");
        return 0;
    }

    /* Verify !important rule won despite lower specificity */
    if (computed.values[CSS_PROP_WIDTH].length.value == INTTOFIX(50)) {
        printf("  PASSED: !important rule won (50px)\n");
        return 1;
    } else {
        printf("  FAILED: Wrong rule won (expected 50px, got %d)\n",
               FIXTOINT(computed.values[CSS_PROP_WIDTH].length.value));
        return 0;
    }
}

/* Test 4: Initial values - all properties have defaults */
static int test_initial_values(void) {
    printf("\nTEST 4: Initial values\n");

    css_computed_style computed;
    css_cascade_context ctx = {0};

    /* No matched rules - should get all initial values */
    css_error err = css_cascade_for_element(&ctx, &computed);
    if (err != CSS_OK) {
        printf("  FAILED: Cascade returned error\n");
        return 0;
    }

    /* Verify initial values are set */
    int passed = 1;

    /* Color should be black by default */
    if (computed.values[CSS_PROP_COLOR].color != CSS_COLOR_BLACK) {
        printf("  FAILED: Color not set to black\n");
        passed = 0;
    }

    /* Display should be inline by default (per CSS spec initial value) */
    if (computed.values[CSS_PROP_DISPLAY].keyword != CSS_DISPLAY_INLINE) {
        printf("  FAILED: Display not set to inline (got %u)\n",
               computed.values[CSS_PROP_DISPLAY].keyword);
        passed = 0;
    }

    if (passed) {
        printf("  PASSED: All initial values set correctly\n");
    }

    return passed;
}

/* Test 5: Color property */
static int test_color_property(void) {
    printf("\nTEST 5: Color property\n");

    css_computed_style computed;
    css_cascade_context ctx = {0};

    css_rule rule = {0};
    rule.properties[0].id = CSS_PROP_COLOR;
    rule.properties[0].value.color = 0xFFFF0000;  /* Red */
    rule.property_count = 1;

    ctx.matched_rules = &rule;
    ctx.matched_rule_count = 1;

    uint16_t specificity = 1;
    css_origin origin = CSS_ORIGIN_AUTHOR;
    ctx.specificities = &specificity;
    ctx.origins = &origin;

    css_error err = css_cascade_for_element(&ctx, &computed);
    if (err != CSS_OK) {
        printf("  FAILED: Cascade returned error\n");
        return 0;
    }

    if (computed.values[CSS_PROP_COLOR].color == 0xFFFF0000) {
        printf("  PASSED: Color set correctly (red)\n");
        return 1;
    } else {
        printf("  FAILED: Color not set correctly\n");
        return 0;
    }
}

int main(void) {
    printf("===== Native CSS Cascade Algorithm Tests =====\n\n");

    int total = 0, passed = 0;

    total++;
    passed += test_basic_cascade();

    total++;
    passed += test_cascade_specificity();

    total++;
    passed += test_cascade_origin();

    total++;
    passed += test_initial_values();

    total++;
    passed += test_color_property();

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
