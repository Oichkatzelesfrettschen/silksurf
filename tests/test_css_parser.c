#include <stdio.h>
#include <string.h>
#include <math.h>
#include "../include/silksurf/css_native_parser.h"
#include "../include/silksurf/allocator.h"

static int tests_passed = 0;
static int tests_total = 0;

#define ASSERT(cond, msg) do { \
    tests_total++; \
    if (!(cond)) { printf("  FAIL: %s\n", msg); return 0; } \
    tests_passed++; \
} while(0)

static int test_single_rule(void) {
    printf("TEST: Single rule parsing\n");
    silk_arena_t *arena = silk_arena_create(8192);
    const char *css = "div { width: 100px; }";
    css_parsed_stylesheet_t *sheet = css_parse_stylesheet(arena, css, strlen(css));

    ASSERT(sheet != NULL, "sheet not null");
    ASSERT(sheet->rule_count == 1, "one rule");
    ASSERT(sheet->rules[0].declaration_count == 1, "one declaration");

    css_parsed_rule_t *rule = &sheet->rules[0];
    ASSERT(strncmp(rule->selector_text, "div", 3) == 0, "selector is div");

    css_parsed_declaration_t *decl = &rule->declarations[0];
    ASSERT(strncmp(decl->property, "width", 5) == 0, "property is width");
    ASSERT(decl->value.type == CSS_VAL_LENGTH, "value is length");
    ASSERT(fabs(decl->value.data.length.value - 100.0) < 0.001, "100px value");
    ASSERT(decl->value.data.length.unit_len == 2 &&
           strncmp(decl->value.data.length.unit, "px", 2) == 0, "px unit");

    silk_arena_destroy(arena);
    printf("  PASSED\n");
    return 1;
}

static int test_multiple_declarations(void) {
    printf("TEST: Multiple declarations\n");
    silk_arena_t *arena = silk_arena_create(8192);
    const char *css = "p { color: red; width: 200px; height: 50%; }";
    css_parsed_stylesheet_t *sheet = css_parse_stylesheet(arena, css, strlen(css));

    ASSERT(sheet != NULL, "sheet not null");
    ASSERT(sheet->rule_count == 1, "one rule");
    ASSERT(sheet->rules[0].declaration_count == 3, "three declarations");

    css_parsed_declaration_t *d0 = &sheet->rules[0].declarations[0];
    ASSERT(strncmp(d0->property, "color", 5) == 0, "first prop is color");
    ASSERT(d0->value.type == CSS_VAL_COLOR, "color type");
    ASSERT(d0->value.data.color == 0xFFFF0000, "red color");

    css_parsed_declaration_t *d1 = &sheet->rules[0].declarations[1];
    ASSERT(strncmp(d1->property, "width", 5) == 0, "second prop is width");
    ASSERT(d1->value.type == CSS_VAL_LENGTH, "length type");

    css_parsed_declaration_t *d2 = &sheet->rules[0].declarations[2];
    ASSERT(strncmp(d2->property, "height", 6) == 0, "third prop is height");
    ASSERT(d2->value.type == CSS_VAL_PERCENTAGE, "percentage type");
    ASSERT(fabs(d2->value.data.percentage - 50.0) < 0.001, "50% value");

    silk_arena_destroy(arena);
    printf("  PASSED\n");
    return 1;
}

static int test_multiple_rules(void) {
    printf("TEST: Multiple rules\n");
    silk_arena_t *arena = silk_arena_create(8192);
    const char *css = "body { margin: 0px; }\n"
                      "div { width: 100px; }\n"
                      "span { display: inline; }";
    css_parsed_stylesheet_t *sheet = css_parse_stylesheet(arena, css, strlen(css));

    ASSERT(sheet != NULL, "sheet not null");
    ASSERT(sheet->rule_count == 3, "three rules");
    ASSERT(sheet->rules[0].source_order == 0, "first rule order 0");
    ASSERT(sheet->rules[1].source_order == 1, "second rule order 1");
    ASSERT(sheet->rules[2].source_order == 2, "third rule order 2");

    ASSERT(strncmp(sheet->rules[0].selector_text, "body", 4) == 0, "body selector");
    ASSERT(strncmp(sheet->rules[1].selector_text, "div", 3) == 0, "div selector");
    ASSERT(strncmp(sheet->rules[2].selector_text, "span", 4) == 0, "span selector");

    silk_arena_destroy(arena);
    printf("  PASSED\n");
    return 1;
}

static int test_color_parsing(void) {
    printf("TEST: Color parsing\n");
    silk_arena_t *arena = silk_arena_create(8192);

    /* Hex colors */
    const char *css1 = "div { color: #ff0000; background-color: #0f0; }";
    css_parsed_stylesheet_t *s1 = css_parse_stylesheet(arena, css1, strlen(css1));
    ASSERT(s1 && s1->rule_count == 1, "hex color rule");
    ASSERT(s1->rules[0].declarations[0].value.data.color == 0xFFFF0000, "6-digit hex red");
    ASSERT(s1->rules[0].declarations[1].value.data.color == 0xFF00FF00, "3-digit hex green");

    /* Named colors */
    const char *css2 = "p { color: blue; }";
    css_parsed_stylesheet_t *s2 = css_parse_stylesheet(arena, css2, strlen(css2));
    ASSERT(s2 && s2->rule_count == 1, "named color rule");
    ASSERT(s2->rules[0].declarations[0].value.data.color == 0xFF0000FF, "named blue");

    silk_arena_destroy(arena);
    printf("  PASSED\n");
    return 1;
}

static int test_class_and_id_selectors(void) {
    printf("TEST: Class and ID selectors\n");
    silk_arena_t *arena = silk_arena_create(8192);
    const char *css = ".highlight { color: red; }\n"
                      "#main { width: 960px; }\n"
                      "div.container { margin: 0px; }";
    css_parsed_stylesheet_t *sheet = css_parse_stylesheet(arena, css, strlen(css));

    ASSERT(sheet != NULL, "sheet not null");
    ASSERT(sheet->rule_count == 3, "three rules");
    ASSERT(strncmp(sheet->rules[0].selector_text, ".highlight", 10) == 0, ".highlight selector");
    ASSERT(strncmp(sheet->rules[1].selector_text, "#main", 5) == 0, "#main selector");
    ASSERT(strncmp(sheet->rules[2].selector_text, "div.container", 13) == 0, "compound selector");

    silk_arena_destroy(arena);
    printf("  PASSED\n");
    return 1;
}

static int test_important(void) {
    printf("TEST: !important\n");
    silk_arena_t *arena = silk_arena_create(8192);
    const char *css = "div { color: red !important; width: 100px; }";
    css_parsed_stylesheet_t *sheet = css_parse_stylesheet(arena, css, strlen(css));

    ASSERT(sheet && sheet->rule_count == 1, "one rule");
    ASSERT(sheet->rules[0].declaration_count == 2, "two declarations");
    ASSERT(sheet->rules[0].declarations[0].important == true, "color is important");
    ASSERT(sheet->rules[0].declarations[1].important == false, "width is not important");

    silk_arena_destroy(arena);
    printf("  PASSED\n");
    return 1;
}

static int test_inline_style(void) {
    printf("TEST: Inline style parsing\n");
    silk_arena_t *arena = silk_arena_create(8192);
    const char *style = "color: red; width: 100px; background-color: #00ff00";
    css_parsed_declaration_t decls[8];
    uint32_t count = css_parse_inline_style(arena, style, strlen(style), decls, 8);

    ASSERT(count == 3, "three inline declarations");
    ASSERT(strncmp(decls[0].property, "color", 5) == 0, "first is color");
    ASSERT(decls[0].value.data.color == 0xFFFF0000, "red");
    ASSERT(strncmp(decls[1].property, "width", 5) == 0, "second is width");
    ASSERT(strncmp(decls[2].property, "background-color", 16) == 0, "third is bg");

    silk_arena_destroy(arena);
    printf("  PASSED\n");
    return 1;
}

static int test_comments_in_css(void) {
    printf("TEST: Comments in CSS\n");
    silk_arena_t *arena = silk_arena_create(8192);
    const char *css = "/* header styles */\n"
                      "h1 { color: black; /* text color */ }\n"
                      "/* footer */ p { margin: 5px; }";
    css_parsed_stylesheet_t *sheet = css_parse_stylesheet(arena, css, strlen(css));

    ASSERT(sheet && sheet->rule_count == 2, "two rules after comments");
    ASSERT(strncmp(sheet->rules[0].selector_text, "h1", 2) == 0, "h1 selector");
    ASSERT(strncmp(sheet->rules[1].selector_text, "p", 1) == 0, "p selector");

    silk_arena_destroy(arena);
    printf("  PASSED\n");
    return 1;
}

static int test_keyword_values(void) {
    printf("TEST: Keyword values\n");
    silk_arena_t *arena = silk_arena_create(8192);
    const char *css = "div { display: block; position: absolute; width: auto; }";
    css_parsed_stylesheet_t *sheet = css_parse_stylesheet(arena, css, strlen(css));

    ASSERT(sheet && sheet->rule_count == 1, "one rule");
    ASSERT(sheet->rules[0].declaration_count == 3, "three declarations");

    css_parsed_declaration_t *d0 = &sheet->rules[0].declarations[0];
    ASSERT(d0->value.type == CSS_VAL_KEYWORD, "display is keyword");
    ASSERT(strcmp(d0->value.data.keyword, "block") == 0, "block keyword");

    css_parsed_declaration_t *d1 = &sheet->rules[0].declarations[1];
    ASSERT(d1->value.type == CSS_VAL_KEYWORD, "position is keyword");
    ASSERT(strcmp(d1->value.data.keyword, "absolute") == 0, "absolute keyword");

    css_parsed_declaration_t *d2 = &sheet->rules[0].declarations[2];
    ASSERT(d2->value.type == CSS_VAL_KEYWORD, "width auto is keyword");
    ASSERT(strcmp(d2->value.data.keyword, "auto") == 0, "auto keyword");

    silk_arena_destroy(arena);
    printf("  PASSED\n");
    return 1;
}

static int test_error_recovery(void) {
    printf("TEST: Error recovery\n");
    silk_arena_t *arena = silk_arena_create(8192);
    /* Malformed declaration should be skipped */
    const char *css = "div { !invalid; color: red; width px; height: 50px; }";
    css_parsed_stylesheet_t *sheet = css_parse_stylesheet(arena, css, strlen(css));

    ASSERT(sheet && sheet->rule_count == 1, "one rule despite errors");
    /* Should have at least color and height */
    ASSERT(sheet->rules[0].declaration_count >= 2, "at least 2 valid declarations");

    silk_arena_destroy(arena);
    printf("  PASSED\n");
    return 1;
}

int main(void) {
    printf("CSS Parser Test Suite\n");
    printf("=====================\n\n");

    int passed = 0;
    passed += test_single_rule();
    passed += test_multiple_declarations();
    passed += test_multiple_rules();
    passed += test_color_parsing();
    passed += test_class_and_id_selectors();
    passed += test_important();
    passed += test_inline_style();
    passed += test_comments_in_css();
    passed += test_keyword_values();
    passed += test_error_recovery();

    printf("\n=====================\n");
    printf("Test functions: %d/10 passed\n", passed);
    printf("Assertions: %d/%d passed\n", tests_passed, tests_total);

    return (passed == 10) ? 0 : 1;
}
