#include <stdio.h>
#include <string.h>
#include <math.h>
#include "../include/silksurf/css_tokenizer.h"
#include "../include/silksurf/allocator.h"

static int tests_passed = 0;
static int tests_total = 0;

#define ASSERT(cond, msg) do { \
    tests_total++; \
    if (!(cond)) { printf("  FAIL: %s\n", msg); return 0; } \
    tests_passed++; \
} while(0)

static int test_basic_delimiters(void) {
    printf("TEST: Basic delimiters\n");
    silk_arena_t *arena = silk_arena_create(4096);
    silk_css_tokenizer_t *tok = silk_css_tokenizer_create(arena, "{};:,()", 7);

    ASSERT(silk_css_tokenizer_next_token(tok)->type == CSS_TOK_LEFT_CURLY, "{ token");
    ASSERT(silk_css_tokenizer_next_token(tok)->type == CSS_TOK_RIGHT_CURLY, "} token");
    ASSERT(silk_css_tokenizer_next_token(tok)->type == CSS_TOK_SEMICOLON, "; token");
    ASSERT(silk_css_tokenizer_next_token(tok)->type == CSS_TOK_COLON, ": token");
    ASSERT(silk_css_tokenizer_next_token(tok)->type == CSS_TOK_COMMA, ", token");
    ASSERT(silk_css_tokenizer_next_token(tok)->type == CSS_TOK_LEFT_PAREN, "( token");
    ASSERT(silk_css_tokenizer_next_token(tok)->type == CSS_TOK_RIGHT_PAREN, ") token");
    ASSERT(silk_css_tokenizer_next_token(tok)->type == CSS_TOK_EOF, "EOF token");

    silk_arena_destroy(arena);
    printf("  PASSED\n");
    return 1;
}

static int test_ident_tokens(void) {
    printf("TEST: Ident tokens\n");
    silk_arena_t *arena = silk_arena_create(4096);
    silk_css_tokenizer_t *tok = silk_css_tokenizer_create(arena, "color background-color _private", 31);

    silk_css_token_t *t1 = silk_css_tokenizer_next_token(tok);
    ASSERT(t1->type == CSS_TOK_IDENT, "first ident");
    ASSERT(t1->value_len == 5 && strncmp(t1->value, "color", 5) == 0, "color value");

    silk_css_tokenizer_next_token(tok); /* whitespace */

    silk_css_token_t *t2 = silk_css_tokenizer_next_token(tok);
    ASSERT(t2->type == CSS_TOK_IDENT, "hyphenated ident");
    ASSERT(strncmp(t2->value, "background-color", 16) == 0, "background-color value");

    silk_css_tokenizer_next_token(tok); /* whitespace */

    silk_css_token_t *t3 = silk_css_tokenizer_next_token(tok);
    ASSERT(t3->type == CSS_TOK_IDENT, "underscore ident");
    ASSERT(strncmp(t3->value, "_private", 8) == 0, "_private value");

    silk_arena_destroy(arena);
    printf("  PASSED\n");
    return 1;
}

static int test_number_tokens(void) {
    printf("TEST: Number tokens\n");
    silk_arena_t *arena = silk_arena_create(4096);
    silk_css_tokenizer_t *tok = silk_css_tokenizer_create(arena, "42 3.14 -10 +5", 14);

    silk_css_token_t *t1 = silk_css_tokenizer_next_token(tok);
    ASSERT(t1->type == CSS_TOK_NUMBER, "integer");
    ASSERT(fabs(t1->numeric_value - 42.0) < 0.001, "42 value");

    silk_css_tokenizer_next_token(tok); /* ws */

    silk_css_token_t *t2 = silk_css_tokenizer_next_token(tok);
    ASSERT(t2->type == CSS_TOK_NUMBER, "float");
    ASSERT(fabs(t2->numeric_value - 3.14) < 0.001, "3.14 value");

    silk_css_tokenizer_next_token(tok); /* ws */

    silk_css_token_t *t3 = silk_css_tokenizer_next_token(tok);
    ASSERT(t3->type == CSS_TOK_NUMBER, "negative");
    ASSERT(fabs(t3->numeric_value - (-10.0)) < 0.001, "-10 value");

    silk_css_tokenizer_next_token(tok); /* ws */

    silk_css_token_t *t4 = silk_css_tokenizer_next_token(tok);
    ASSERT(t4->type == CSS_TOK_NUMBER, "positive");
    ASSERT(fabs(t4->numeric_value - 5.0) < 0.001, "+5 value");

    silk_arena_destroy(arena);
    printf("  PASSED\n");
    return 1;
}

static int test_dimension_tokens(void) {
    printf("TEST: Dimension tokens\n");
    silk_arena_t *arena = silk_arena_create(4096);
    silk_css_tokenizer_t *tok = silk_css_tokenizer_create(arena, "100px 2.5em 16rem", 17);

    silk_css_token_t *t1 = silk_css_tokenizer_next_token(tok);
    ASSERT(t1->type == CSS_TOK_DIMENSION, "100px type");
    ASSERT(fabs(t1->numeric_value - 100.0) < 0.001, "100px value");
    ASSERT(t1->unit_len == 2 && strncmp(t1->unit, "px", 2) == 0, "px unit");

    silk_css_tokenizer_next_token(tok); /* ws */

    silk_css_token_t *t2 = silk_css_tokenizer_next_token(tok);
    ASSERT(t2->type == CSS_TOK_DIMENSION, "2.5em type");
    ASSERT(fabs(t2->numeric_value - 2.5) < 0.001, "2.5em value");
    ASSERT(t2->unit_len == 2 && strncmp(t2->unit, "em", 2) == 0, "em unit");

    silk_css_tokenizer_next_token(tok); /* ws */

    silk_css_token_t *t3 = silk_css_tokenizer_next_token(tok);
    ASSERT(t3->type == CSS_TOK_DIMENSION, "16rem type");
    ASSERT(fabs(t3->numeric_value - 16.0) < 0.001, "16rem value");
    ASSERT(t3->unit_len == 3 && strncmp(t3->unit, "rem", 3) == 0, "rem unit");

    silk_arena_destroy(arena);
    printf("  PASSED\n");
    return 1;
}

static int test_percentage_tokens(void) {
    printf("TEST: Percentage tokens\n");
    silk_arena_t *arena = silk_arena_create(4096);
    silk_css_tokenizer_t *tok = silk_css_tokenizer_create(arena, "50% 100%", 8);

    silk_css_token_t *t1 = silk_css_tokenizer_next_token(tok);
    ASSERT(t1->type == CSS_TOK_PERCENTAGE, "50% type");
    ASSERT(fabs(t1->numeric_value - 50.0) < 0.001, "50% value");

    silk_css_tokenizer_next_token(tok); /* ws */

    silk_css_token_t *t2 = silk_css_tokenizer_next_token(tok);
    ASSERT(t2->type == CSS_TOK_PERCENTAGE, "100% type");
    ASSERT(fabs(t2->numeric_value - 100.0) < 0.001, "100% value");

    silk_arena_destroy(arena);
    printf("  PASSED\n");
    return 1;
}

static int test_hash_tokens(void) {
    printf("TEST: Hash tokens\n");
    silk_arena_t *arena = silk_arena_create(4096);
    silk_css_tokenizer_t *tok = silk_css_tokenizer_create(arena, "#ff0000 #myid", 13);

    silk_css_token_t *t1 = silk_css_tokenizer_next_token(tok);
    ASSERT(t1->type == CSS_TOK_HASH, "hex color hash");
    ASSERT(t1->value_len == 6 && strncmp(t1->value, "ff0000", 6) == 0, "ff0000 value");

    silk_css_tokenizer_next_token(tok); /* ws */

    silk_css_token_t *t2 = silk_css_tokenizer_next_token(tok);
    ASSERT(t2->type == CSS_TOK_HASH, "id hash");
    ASSERT(t2->value_len == 4 && strncmp(t2->value, "myid", 4) == 0, "myid value");

    silk_arena_destroy(arena);
    printf("  PASSED\n");
    return 1;
}

static int test_string_tokens(void) {
    printf("TEST: String tokens\n");
    silk_arena_t *arena = silk_arena_create(4096);
    const char *input = "\"hello world\" 'single'";
    silk_css_tokenizer_t *tok = silk_css_tokenizer_create(arena, input, strlen(input));

    silk_css_token_t *t1 = silk_css_tokenizer_next_token(tok);
    ASSERT(t1->type == CSS_TOK_STRING, "double-quoted string");
    ASSERT(t1->value_len == 11 && strncmp(t1->value, "hello world", 11) == 0, "hello world value");

    silk_css_tokenizer_next_token(tok); /* ws */

    silk_css_token_t *t2 = silk_css_tokenizer_next_token(tok);
    ASSERT(t2->type == CSS_TOK_STRING, "single-quoted string");
    ASSERT(t2->value_len == 6 && strncmp(t2->value, "single", 6) == 0, "single value");

    silk_arena_destroy(arena);
    printf("  PASSED\n");
    return 1;
}

static int test_function_tokens(void) {
    printf("TEST: Function tokens\n");
    silk_arena_t *arena = silk_arena_create(4096);
    silk_css_tokenizer_t *tok = silk_css_tokenizer_create(arena, "rgb(255, 0, 0)", 14);

    silk_css_token_t *t1 = silk_css_tokenizer_next_token(tok);
    ASSERT(t1->type == CSS_TOK_FUNCTION, "function token");
    ASSERT(t1->value_len == 3 && strncmp(t1->value, "rgb", 3) == 0, "rgb function name");

    silk_css_token_t *t2 = silk_css_tokenizer_next_token(tok);
    ASSERT(t2->type == CSS_TOK_NUMBER, "first arg");
    ASSERT(fabs(t2->numeric_value - 255.0) < 0.001, "255 value");

    silk_arena_destroy(arena);
    printf("  PASSED\n");
    return 1;
}

static int test_comments(void) {
    printf("TEST: Comment handling\n");
    silk_arena_t *arena = silk_arena_create(4096);
    const char *input_c = "color /* comment */ red";
    silk_css_tokenizer_t *tok = silk_css_tokenizer_create(arena, input_c, strlen(input_c));

    silk_css_token_t *t1 = silk_css_tokenizer_next_token(tok);
    ASSERT(t1->type == CSS_TOK_IDENT, "ident before comment");

    /* Skip whitespace tokens (comment produces adjacent ws) */
    silk_css_token_t *t = silk_css_tokenizer_next_token(tok);
    while (t->type == CSS_TOK_WHITESPACE)
        t = silk_css_tokenizer_next_token(tok);
    ASSERT(t->type == CSS_TOK_IDENT, "ident after comment");
    ASSERT(strncmp(t->value, "red", 3) == 0, "red value");

    silk_arena_destroy(arena);
    printf("  PASSED\n");
    return 1;
}

static int test_full_rule(void) {
    printf("TEST: Full CSS rule tokenization\n");
    silk_arena_t *arena = silk_arena_create(4096);
    const char *css = "div.main { color: #ff0000; width: 100px; }";
    silk_css_tokenizer_t *tok = silk_css_tokenizer_create(arena, css, strlen(css));

    silk_css_token_t *t;
    t = silk_css_tokenizer_next_token(tok); ASSERT(t->type == CSS_TOK_IDENT, "div");
    t = silk_css_tokenizer_next_token(tok); ASSERT(t->type == CSS_TOK_DELIM && t->delim == '.', "dot");
    t = silk_css_tokenizer_next_token(tok); ASSERT(t->type == CSS_TOK_IDENT, "main");
    t = silk_css_tokenizer_next_token(tok); ASSERT(t->type == CSS_TOK_WHITESPACE, "ws");
    t = silk_css_tokenizer_next_token(tok); ASSERT(t->type == CSS_TOK_LEFT_CURLY, "{");
    t = silk_css_tokenizer_next_token(tok); ASSERT(t->type == CSS_TOK_WHITESPACE, "ws");
    t = silk_css_tokenizer_next_token(tok); ASSERT(t->type == CSS_TOK_IDENT, "color");
    t = silk_css_tokenizer_next_token(tok); ASSERT(t->type == CSS_TOK_COLON, ":");
    t = silk_css_tokenizer_next_token(tok); ASSERT(t->type == CSS_TOK_WHITESPACE, "ws");
    t = silk_css_tokenizer_next_token(tok); ASSERT(t->type == CSS_TOK_HASH, "#ff0000");
    t = silk_css_tokenizer_next_token(tok); ASSERT(t->type == CSS_TOK_SEMICOLON, ";");
    t = silk_css_tokenizer_next_token(tok); ASSERT(t->type == CSS_TOK_WHITESPACE, "ws");
    t = silk_css_tokenizer_next_token(tok); ASSERT(t->type == CSS_TOK_IDENT, "width");
    t = silk_css_tokenizer_next_token(tok); ASSERT(t->type == CSS_TOK_COLON, ":");
    t = silk_css_tokenizer_next_token(tok); ASSERT(t->type == CSS_TOK_WHITESPACE, "ws");
    t = silk_css_tokenizer_next_token(tok); ASSERT(t->type == CSS_TOK_DIMENSION, "100px");
    t = silk_css_tokenizer_next_token(tok); ASSERT(t->type == CSS_TOK_SEMICOLON, ";");
    t = silk_css_tokenizer_next_token(tok); ASSERT(t->type == CSS_TOK_WHITESPACE, "ws");
    t = silk_css_tokenizer_next_token(tok); ASSERT(t->type == CSS_TOK_RIGHT_CURLY, "}");
    t = silk_css_tokenizer_next_token(tok); ASSERT(t->type == CSS_TOK_EOF, "EOF");

    silk_arena_destroy(arena);
    printf("  PASSED\n");
    return 1;
}

int main(void) {
    printf("CSS Tokenizer Test Suite\n");
    printf("========================\n\n");

    int passed = 0;
    passed += test_basic_delimiters();
    passed += test_ident_tokens();
    passed += test_number_tokens();
    passed += test_dimension_tokens();
    passed += test_percentage_tokens();
    passed += test_hash_tokens();
    passed += test_string_tokens();
    passed += test_function_tokens();
    passed += test_comments();
    passed += test_full_rule();

    printf("\n========================\n");
    printf("Test functions: %d/10 passed\n", passed);
    printf("Assertions: %d/%d passed\n", tests_passed, tests_total);

    return (passed == 10) ? 0 : 1;
}
