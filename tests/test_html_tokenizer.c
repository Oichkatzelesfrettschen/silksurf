/*
 * SilkSurf HTML5 Tokenizer Tests
 *
 * Unit tests for tokenizer foundation:
 * - UTF-8 input stream
 * - Character reference decoding
 * - Basic tokenizer setup
 *
 * Copyright (c) 2025 SilkSurf Project
 * SPDX-License-Identifier: MIT
 */

#include "src/document/html_tokenizer.h"
#include "silksurf/allocator.h"
#include <stdio.h>
#include <string.h>
#include <assert.h>

/* Test counters */
static int tests_run = 0;
static int tests_passed = 0;

/* Assertion macro with line reporting */
#define TEST_ASSERT(condition, message) do { \
    tests_run++; \
    if (!(condition)) { \
        printf("  FAIL: %s (line %d): %s\n", __func__, __LINE__, message); \
    } else { \
        tests_passed++; \
    } \
} while(0)

#define TEST_ASSERT_EQ(expected, actual, message) do { \
    tests_run++; \
    if ((expected) != (actual)) { \
        printf("  FAIL: %s (line %d): %s (expected %d, got %d)\n", \
               __func__, __LINE__, message, (int)(expected), (int)(actual)); \
    } else { \
        tests_passed++; \
    } \
} while(0)

/* ============================================================================
 * UTF-8 Input Stream Tests
 * ============================================================================ */

void test_input_stream_ascii(void) {
    silk_arena_t *arena = silk_arena_create(4096);
    const char *input = "Hello, World!";
    size_t input_len = strlen(input);

    silk_html_input_stream_t *stream =
        silk_html_input_stream_create(arena, input, input_len);

    TEST_ASSERT(stream != NULL, "Stream created");
    TEST_ASSERT(!silk_html_input_stream_is_eof(stream), "Not at EOF initially");

    /* Read ASCII characters */
    TEST_ASSERT_EQ('H', silk_html_input_stream_next(stream), "First char is 'H'");
    TEST_ASSERT_EQ('e', silk_html_input_stream_next(stream), "Second char is 'e'");
    TEST_ASSERT_EQ('l', silk_html_input_stream_next(stream), "Third char is 'l'");

    /* Skip to end */
    while (!silk_html_input_stream_is_eof(stream)) {
        silk_html_input_stream_next(stream);
    }

    TEST_ASSERT(silk_html_input_stream_is_eof(stream), "At EOF after reading all");
    TEST_ASSERT_EQ(0xFFFFFFFF, silk_html_input_stream_next(stream), "EOF returns 0xFFFFFFFF");

    silk_arena_destroy(arena);
}

void test_input_stream_peek(void) {
    silk_arena_t *arena = silk_arena_create(4096);
    const char *input = "ABC";

    silk_html_input_stream_t *stream =
        silk_html_input_stream_create(arena, input, strlen(input));

    TEST_ASSERT(stream != NULL, "Stream created");

    /* Peek without consuming */
    TEST_ASSERT_EQ('A', silk_html_input_stream_peek(stream, 0), "Peek at offset 0");
    TEST_ASSERT_EQ('B', silk_html_input_stream_peek(stream, 1), "Peek at offset 1");
    TEST_ASSERT_EQ('C', silk_html_input_stream_peek(stream, 2), "Peek at offset 2");

    /* Verify peek didn't consume */
    TEST_ASSERT_EQ('A', silk_html_input_stream_next(stream), "First char still 'A'");

    /* Peek again */
    TEST_ASSERT_EQ('B', silk_html_input_stream_peek(stream, 0), "Peek at new position");
    TEST_ASSERT_EQ('C', silk_html_input_stream_peek(stream, 1), "Peek ahead");

    silk_arena_destroy(arena);
}

void test_input_stream_utf8_2byte(void) {
    silk_arena_t *arena = silk_arena_create(4096);
    /* "café" in UTF-8: c3 a9 = é (U+00E9) */
    const char input[] = "caf\xC3\xA9";
    size_t input_len = strlen(input);

    silk_html_input_stream_t *stream =
        silk_html_input_stream_create(arena, input, input_len);

    TEST_ASSERT(stream != NULL, "Stream created");

    TEST_ASSERT_EQ('c', silk_html_input_stream_next(stream), "First char 'c'");
    TEST_ASSERT_EQ('a', silk_html_input_stream_next(stream), "Second char 'a'");
    TEST_ASSERT_EQ('f', silk_html_input_stream_next(stream), "Third char 'f'");
    TEST_ASSERT_EQ(0x00E9, silk_html_input_stream_next(stream), "Fourth char é (U+00E9)");

    silk_arena_destroy(arena);
}

void test_input_stream_utf8_3byte(void) {
    silk_arena_t *arena = silk_arena_create(4096);
    /* "€" (Euro sign) = E2 82 AC (U+20AC) */
    const char input[] = "\xE2\x82\xAC";
    size_t input_len = strlen(input);

    silk_html_input_stream_t *stream =
        silk_html_input_stream_create(arena, input, input_len);

    TEST_ASSERT(stream != NULL, "Stream created");
    TEST_ASSERT_EQ(0x20AC, silk_html_input_stream_next(stream), "Euro sign (U+20AC)");

    silk_arena_destroy(arena);
}

void test_input_stream_utf8_4byte(void) {
    silk_arena_t *arena = silk_arena_create(4096);
    /* "𝄞" (Musical symbol) = F0 9D 84 9E (U+1D11E) */
    const char input[] = "\xF0\x9D\x84\x9E";
    size_t input_len = strlen(input);

    silk_html_input_stream_t *stream =
        silk_html_input_stream_create(arena, input, input_len);

    TEST_ASSERT(stream != NULL, "Stream created");
    TEST_ASSERT_EQ(0x1D11E, silk_html_input_stream_next(stream), "Musical symbol (U+1D11E)");

    silk_arena_destroy(arena);
}

void test_input_stream_crlf_normalization(void) {
    silk_arena_t *arena = silk_arena_create(4096);

    /* Test CR -> LF */
    const char input_cr[] = "A\rB";
    silk_html_input_stream_t *stream_cr =
        silk_html_input_stream_create(arena, input_cr, strlen(input_cr));

    TEST_ASSERT_EQ('A', silk_html_input_stream_next(stream_cr), "First char 'A'");
    TEST_ASSERT_EQ('\n', silk_html_input_stream_next(stream_cr), "CR normalized to LF");
    TEST_ASSERT_EQ('B', silk_html_input_stream_next(stream_cr), "Third char 'B'");

    /* Test CRLF -> LF */
    const char input_crlf[] = "A\r\nB";
    silk_html_input_stream_t *stream_crlf =
        silk_html_input_stream_create(arena, input_crlf, strlen(input_crlf));

    TEST_ASSERT_EQ('A', silk_html_input_stream_next(stream_crlf), "First char 'A'");
    TEST_ASSERT_EQ('\n', silk_html_input_stream_next(stream_crlf), "CRLF normalized to LF");
    TEST_ASSERT_EQ('B', silk_html_input_stream_next(stream_crlf), "Third char 'B'");

    /* Test plain LF is unchanged */
    const char input_lf[] = "A\nB";
    silk_html_input_stream_t *stream_lf =
        silk_html_input_stream_create(arena, input_lf, strlen(input_lf));

    TEST_ASSERT_EQ('A', silk_html_input_stream_next(stream_lf), "First char 'A'");
    TEST_ASSERT_EQ('\n', silk_html_input_stream_next(stream_lf), "LF unchanged");
    TEST_ASSERT_EQ('B', silk_html_input_stream_next(stream_lf), "Third char 'B'");

    silk_arena_destroy(arena);
}

void test_input_stream_line_column_tracking(void) {
    silk_arena_t *arena = silk_arena_create(4096);
    const char *input = "Line 1\nLine 2\nLine 3";

    silk_html_input_stream_t *stream =
        silk_html_input_stream_create(arena, input, strlen(input));

    size_t line, column;

    /* Initial position */
    silk_html_input_stream_get_position(stream, &line, &column);
    TEST_ASSERT_EQ(1, line, "Initial line is 1");
    TEST_ASSERT_EQ(1, column, "Initial column is 1");

    /* Read "Line " */
    for (int i = 0; i < 5; i++) {
        silk_html_input_stream_next(stream);
    }

    silk_html_input_stream_get_position(stream, &line, &column);
    TEST_ASSERT_EQ(1, line, "Still on line 1");
    TEST_ASSERT_EQ(6, column, "Column advanced to 6");

    /* Read "1\n" */
    silk_html_input_stream_next(stream);  /* '1' */
    silk_html_input_stream_next(stream);  /* '\n' */

    silk_html_input_stream_get_position(stream, &line, &column);
    TEST_ASSERT_EQ(2, line, "Advanced to line 2");
    TEST_ASSERT_EQ(1, column, "Column reset to 1");

    silk_arena_destroy(arena);
}

void test_input_stream_empty(void) {
    silk_arena_t *arena = silk_arena_create(4096);
    const char *input = "";

    silk_html_input_stream_t *stream =
        silk_html_input_stream_create(arena, input, 0);

    TEST_ASSERT(stream != NULL, "Stream created");
    TEST_ASSERT(silk_html_input_stream_is_eof(stream), "Empty input is EOF");
    TEST_ASSERT_EQ(0xFFFFFFFF, silk_html_input_stream_next(stream), "EOF on next");

    silk_arena_destroy(arena);
}

/* ============================================================================
 * Character Reference Decoding Tests
 * ============================================================================ */

void test_named_char_ref_common(void) {
    uint32_t code_points[2];
    size_t consumed;

    /* Test common entities */
    int result = silk_html_decode_named_char_ref("lt", 2, code_points, &consumed);
    TEST_ASSERT_EQ(1, result, "lt decoded to 1 code point");
    TEST_ASSERT_EQ('<', code_points[0], "lt is '<'");
    TEST_ASSERT_EQ(2, consumed, "Consumed 2 chars");

    result = silk_html_decode_named_char_ref("gt", 2, code_points, &consumed);
    TEST_ASSERT_EQ(1, result, "gt decoded");
    TEST_ASSERT_EQ('>', code_points[0], "gt is '>'");

    result = silk_html_decode_named_char_ref("amp", 3, code_points, &consumed);
    TEST_ASSERT_EQ(1, result, "amp decoded");
    TEST_ASSERT_EQ('&', code_points[0], "amp is '&'");

    result = silk_html_decode_named_char_ref("quot", 4, code_points, &consumed);
    TEST_ASSERT_EQ(1, result, "quot decoded");
    TEST_ASSERT_EQ('"', code_points[0], "quot is '\"'");

    result = silk_html_decode_named_char_ref("nbsp", 4, code_points, &consumed);
    TEST_ASSERT_EQ(1, result, "nbsp decoded");
    TEST_ASSERT_EQ(0xA0, code_points[0], "nbsp is U+00A0");
}

void test_named_char_ref_unknown(void) {
    uint32_t code_points[2];
    size_t consumed;

    int result = silk_html_decode_named_char_ref("unknown", 7, code_points, &consumed);
    TEST_ASSERT_EQ(0, result, "Unknown entity returns 0");
}

void test_numeric_char_ref_decimal(void) {
    uint32_t cp;

    cp = silk_html_decode_numeric_char_ref("65", 2, false);
    TEST_ASSERT_EQ('A', cp, "Decimal 65 is 'A'");

    cp = silk_html_decode_numeric_char_ref("8364", 4, false);
    TEST_ASSERT_EQ(0x20AC, cp, "Decimal 8364 is Euro sign");

    cp = silk_html_decode_numeric_char_ref("0", 1, false);
    TEST_ASSERT_EQ(0xFFFD, cp, "Decimal 0 is invalid (replacement char)");
}

void test_numeric_char_ref_hexadecimal(void) {
    uint32_t cp;

    cp = silk_html_decode_numeric_char_ref("41", 2, true);
    TEST_ASSERT_EQ('A', cp, "Hex 41 is 'A'");

    cp = silk_html_decode_numeric_char_ref("20AC", 4, true);
    TEST_ASSERT_EQ(0x20AC, cp, "Hex 20AC is Euro sign");

    cp = silk_html_decode_numeric_char_ref("1D11E", 5, true);
    TEST_ASSERT_EQ(0x1D11E, cp, "Hex 1D11E is musical symbol");

    /* Invalid: surrogate range */
    cp = silk_html_decode_numeric_char_ref("D800", 4, true);
    TEST_ASSERT_EQ(0xFFFD, cp, "Surrogate D800 is invalid");
}

/* ============================================================================
 * Tokenizer Setup Tests
 * ============================================================================ */

void test_tokenizer_create(void) {
    silk_arena_t *arena = silk_arena_create(4096);
    const char *input = "<html></html>";

    silk_html_tokenizer_t *tokenizer =
        silk_html_tokenizer_create(arena, input, strlen(input));

    TEST_ASSERT(tokenizer != NULL, "Tokenizer created");
    TEST_ASSERT(tokenizer->arena == arena, "Arena set");
    TEST_ASSERT(tokenizer->stream != NULL, "Stream created");

    silk_arena_destroy(arena);
}

void test_tokenizer_set_state(void) {
    silk_arena_t *arena = silk_arena_create(4096);
    const char *input = "<html>";

    silk_html_tokenizer_t *tokenizer =
        silk_html_tokenizer_create(arena, input, strlen(input));

    TEST_ASSERT(tokenizer != NULL, "Tokenizer created");

    /* Initial state should be DATA */
    /* (Can't directly access state from here, but we set it) */

    silk_html_tokenizer_set_state(tokenizer, HTML_TOK_RCDATA);
    /* State changed (no way to verify from here, but function called) */

    silk_arena_destroy(arena);
}

/* Error callback for testing */
static int error_count = 0;
static void test_error_cb(void *ctx, const char *msg, size_t line, size_t col) {
    (void)ctx;
    (void)msg;
    (void)line;
    (void)col;
    error_count++;
}

void test_tokenizer_error_callback(void) {
    silk_arena_t *arena = silk_arena_create(4096);
    const char *input = "<html>";

    silk_html_tokenizer_t *tokenizer =
        silk_html_tokenizer_create(arena, input, strlen(input));

    TEST_ASSERT(tokenizer != NULL, "Tokenizer created");

    silk_html_tokenizer_set_error_callback(tokenizer, test_error_cb, NULL);
    /* Callback set (will be tested when errors are actually emitted) */

    silk_arena_destroy(arena);
}

/* ============================================================================
 * Utility Function Tests
 * ============================================================================ */

void test_utility_is_whitespace(void) {
    TEST_ASSERT(silk_html_is_whitespace(0x09), "Tab is whitespace");
    TEST_ASSERT(silk_html_is_whitespace(0x0A), "LF is whitespace");
    TEST_ASSERT(silk_html_is_whitespace(0x0C), "FF is whitespace");
    TEST_ASSERT(silk_html_is_whitespace(0x0D), "CR is whitespace");
    TEST_ASSERT(silk_html_is_whitespace(0x20), "Space is whitespace");
    TEST_ASSERT(!silk_html_is_whitespace('A'), "A is not whitespace");
}

void test_utility_is_alpha(void) {
    TEST_ASSERT(silk_html_is_alpha('A'), "A is alpha");
    TEST_ASSERT(silk_html_is_alpha('Z'), "Z is alpha");
    TEST_ASSERT(silk_html_is_alpha('a'), "a is alpha");
    TEST_ASSERT(silk_html_is_alpha('z'), "z is alpha");
    TEST_ASSERT(!silk_html_is_alpha('0'), "0 is not alpha");
    TEST_ASSERT(!silk_html_is_alpha(' '), "Space is not alpha");
}

void test_utility_is_digit(void) {
    TEST_ASSERT(silk_html_is_digit('0'), "0 is digit");
    TEST_ASSERT(silk_html_is_digit('9'), "9 is digit");
    TEST_ASSERT(!silk_html_is_digit('A'), "A is not digit");
}

void test_utility_to_lower(void) {
    TEST_ASSERT_EQ('a', silk_html_to_lower('A'), "A -> a");
    TEST_ASSERT_EQ('z', silk_html_to_lower('Z'), "Z -> z");
    TEST_ASSERT_EQ('a', silk_html_to_lower('a'), "a -> a (unchanged)");
    TEST_ASSERT_EQ('0', silk_html_to_lower('0'), "0 -> 0 (unchanged)");
}

void test_state_names(void) {
    const char *name = silk_html_tokenizer_state_name(HTML_TOK_DATA);
    TEST_ASSERT(strcmp(name, "Data") == 0, "DATA state name is 'Data'");

    name = silk_html_tokenizer_state_name(HTML_TOK_TAG_OPEN);
    TEST_ASSERT(strcmp(name, "TagOpen") == 0, "TAG_OPEN state name is 'TagOpen'");
}

void test_token_type_names(void) {
    const char *name = silk_html_token_type_name(HTML_TOKEN_START_TAG);
    TEST_ASSERT(strcmp(name, "StartTag") == 0, "START_TAG type name is 'StartTag'");

    name = silk_html_token_type_name(HTML_TOKEN_CHARACTER);
    TEST_ASSERT(strcmp(name, "Character") == 0, "CHARACTER type name is 'Character'");
}

/* ============================================================================
 * Tag Parsing Tests
 * ============================================================================ */

void test_parse_simple_start_tag(void) {
    silk_arena_t *arena = silk_arena_create(4096);
    const char *input = "<div>";

    silk_html_tokenizer_t *tokenizer =
        silk_html_tokenizer_create(arena, input, strlen(input));

    silk_html_token_t *token = silk_html_tokenizer_next_token(tokenizer);

    printf("DEBUG: token=%p\n", (void*)token);
    if (token) {
        printf("DEBUG: token->type=%d\n", token->type);
        printf("DEBUG: token->tag_name=%p\n", (void*)token->tag_name);
        if (token->tag_name) {
            printf("DEBUG: tag_name='%s'\n", token->tag_name);
        }
    }

    TEST_ASSERT(token != NULL, "Token should not be NULL");
    TEST_ASSERT_EQ(HTML_TOKEN_START_TAG, token->type, "Token type should be START_TAG");
    TEST_ASSERT(token->tag_name != NULL, "Tag name should not be NULL");
    if (token->tag_name != NULL) {
        TEST_ASSERT(strcmp(token->tag_name, "div") == 0, "Tag name should be 'div'");
    }
    TEST_ASSERT_EQ(0, token->attribute_count, "Should have 0 attributes");

    silk_arena_destroy(arena);
}

void test_parse_simple_end_tag(void) {
    silk_arena_t *arena = silk_arena_create(4096);
    const char *input = "</div>";

    silk_html_tokenizer_t *tokenizer =
        silk_html_tokenizer_create(arena, input, strlen(input));

    silk_html_token_t *token = silk_html_tokenizer_next_token(tokenizer);

    TEST_ASSERT(token != NULL, "Token should not be NULL");
    TEST_ASSERT_EQ(HTML_TOKEN_END_TAG, token->type, "Token type should be END_TAG");
    TEST_ASSERT(token->tag_name != NULL, "Tag name should not be NULL");
    TEST_ASSERT(strcmp(token->tag_name, "div") == 0, "Tag name should be 'div'");

    silk_arena_destroy(arena);
}

void test_parse_tag_case_normalization(void) {
    silk_arena_t *arena = silk_arena_create(4096);
    const char *input = "<DIV>";

    silk_html_tokenizer_t *tokenizer =
        silk_html_tokenizer_create(arena, input, strlen(input));

    silk_html_token_t *token = silk_html_tokenizer_next_token(tokenizer);

    TEST_ASSERT(token != NULL, "Token should not be NULL");
    TEST_ASSERT_EQ(HTML_TOKEN_START_TAG, token->type, "Token type should be START_TAG");
    TEST_ASSERT(token->tag_name != NULL, "Tag name should not be NULL");
    TEST_ASSERT(strcmp(token->tag_name, "div") == 0, "Tag name should be lowercased to 'div'");

    silk_arena_destroy(arena);
}

void test_parse_tag_with_single_attribute(void) {
    silk_arena_t *arena = silk_arena_create(4096);
    const char *input = "<div class>";

    silk_html_tokenizer_t *tokenizer =
        silk_html_tokenizer_create(arena, input, strlen(input));

    silk_html_token_t *token = silk_html_tokenizer_next_token(tokenizer);

    TEST_ASSERT(token != NULL, "Token should not be NULL");
    TEST_ASSERT_EQ(HTML_TOKEN_START_TAG, token->type, "Token type should be START_TAG");
    TEST_ASSERT(strcmp(token->tag_name, "div") == 0, "Tag name should be 'div'");
    TEST_ASSERT_EQ(1, token->attribute_count, "Should have 1 attribute");
    TEST_ASSERT(token->attributes != NULL, "Attributes should not be NULL");
    TEST_ASSERT(strcmp(token->attributes[0].name, "class") == 0, "Attribute name should be 'class'");

    silk_arena_destroy(arena);
}

void test_parse_tag_with_attribute_case_normalization(void) {
    silk_arena_t *arena = silk_arena_create(4096);
    const char *input = "<div CLASS>";

    silk_html_tokenizer_t *tokenizer =
        silk_html_tokenizer_create(arena, input, strlen(input));

    silk_html_token_t *token = silk_html_tokenizer_next_token(tokenizer);

    TEST_ASSERT(token != NULL, "Token should not be NULL");
    TEST_ASSERT_EQ(HTML_TOKEN_START_TAG, token->type, "Token type should be START_TAG");
    TEST_ASSERT_EQ(1, token->attribute_count, "Should have 1 attribute");
    TEST_ASSERT(strcmp(token->attributes[0].name, "class") == 0, "Attribute name should be lowercased to 'class'");

    silk_arena_destroy(arena);
}

void test_parse_tag_with_multiple_attributes(void) {
    silk_arena_t *arena = silk_arena_create(4096);
    const char *input = "<div id class>";

    silk_html_tokenizer_t *tokenizer =
        silk_html_tokenizer_create(arena, input, strlen(input));

    silk_html_token_t *token = silk_html_tokenizer_next_token(tokenizer);

    TEST_ASSERT(token != NULL, "Token should not be NULL");
    TEST_ASSERT_EQ(HTML_TOKEN_START_TAG, token->type, "Token type should be START_TAG");
    TEST_ASSERT_EQ(2, token->attribute_count, "Should have 2 attributes");
    TEST_ASSERT(strcmp(token->attributes[0].name, "id") == 0, "First attribute should be 'id'");
    TEST_ASSERT(strcmp(token->attributes[1].name, "class") == 0, "Second attribute should be 'class'");

    silk_arena_destroy(arena);
}

void test_parse_text_content(void) {
    silk_arena_t *arena = silk_arena_create(4096);
    const char *input = "hello";

    silk_html_tokenizer_t *tokenizer =
        silk_html_tokenizer_create(arena, input, strlen(input));

    /* Should get 5 character tokens */
    for (int i = 0; i < 5; i++) {
        silk_html_token_t *token = silk_html_tokenizer_next_token(tokenizer);
        TEST_ASSERT(token != NULL, "Token should not be NULL");
        TEST_ASSERT_EQ(HTML_TOKEN_CHARACTER, token->type, "Token should be CHARACTER");
        TEST_ASSERT_EQ((uint32_t)input[i], token->code_point, "Character should match input");
    }

    /* Should get EOF */
    silk_html_token_t *token = silk_html_tokenizer_next_token(tokenizer);
    TEST_ASSERT_EQ(HTML_TOKEN_EOF, token->type, "Final token should be EOF");

    silk_arena_destroy(arena);
}

void test_parse_tag_with_whitespace(void) {
    silk_arena_t *arena = silk_arena_create(4096);
    const char *input = "<  div  >";

    silk_html_tokenizer_t *tokenizer =
        silk_html_tokenizer_create(arena, input, strlen(input));

    silk_html_token_t *token = silk_html_tokenizer_next_token(tokenizer);

    TEST_ASSERT(token != NULL, "Token should not be NULL");
    TEST_ASSERT_EQ(HTML_TOKEN_START_TAG, token->type, "Token type should be START_TAG");
    TEST_ASSERT(strcmp(token->tag_name, "div") == 0, "Tag name should be 'div'");

    silk_arena_destroy(arena);
}

/* ============================================================================
 * Test Runner
 * ============================================================================ */

int main(void) {
    printf("SilkSurf HTML5 Tokenizer Tests\n");
    printf("================================\n\n");

    /* UTF-8 Input Stream Tests */
    printf("UTF-8 Input Stream Tests:\n");
    test_input_stream_ascii();
    test_input_stream_peek();
    test_input_stream_utf8_2byte();
    test_input_stream_utf8_3byte();
    test_input_stream_utf8_4byte();
    test_input_stream_crlf_normalization();
    test_input_stream_line_column_tracking();
    test_input_stream_empty();

    /* Character Reference Tests */
    printf("\nCharacter Reference Tests:\n");
    test_named_char_ref_common();
    test_named_char_ref_unknown();
    test_numeric_char_ref_decimal();
    test_numeric_char_ref_hexadecimal();

    /* Tokenizer Setup Tests */
    printf("\nTokenizer Setup Tests:\n");
    test_tokenizer_create();
    test_tokenizer_set_state();
    test_tokenizer_error_callback();

    /* Utility Function Tests */
    printf("\nUtility Function Tests:\n");
    test_utility_is_whitespace();
    test_utility_is_alpha();
    test_utility_is_digit();
    test_utility_to_lower();
    test_state_names();
    test_token_type_names();

    /* Tag Parsing Tests */
    printf("\nTag Parsing Tests:\n");
    test_parse_simple_start_tag();
    test_parse_simple_end_tag();
    test_parse_tag_case_normalization();
    test_parse_tag_with_single_attribute();
    test_parse_tag_with_attribute_case_normalization();
    test_parse_tag_with_multiple_attributes();
    test_parse_text_content();
    // test_parse_tag_with_whitespace(); // Removed: invalid HTML5, behavior verified elsewhere

    /* New Tests for Attribute Values */
    printf("\nAttribute Value Tests:\n");
    
    /* Double quoted */
    {
        silk_arena_t *arena = silk_arena_create(4096);
        const char *input = "<div id=\"test\">";
        silk_html_tokenizer_t *tokenizer = silk_html_tokenizer_create(arena, input, strlen(input));
        silk_html_token_t *token = silk_html_tokenizer_next_token(tokenizer);
        TEST_ASSERT_EQ(HTML_TOKEN_START_TAG, token->type, "Double quoted attr");
        TEST_ASSERT_EQ(1, token->attribute_count, "1 attribute");
        TEST_ASSERT(strcmp(token->attributes[0].value, "test") == 0, "Value is 'test'");
        silk_arena_destroy(arena);
    }

    /* Single quoted */
    {
        silk_arena_t *arena = silk_arena_create(4096);
        const char *input = "<div id='test'>";
        silk_html_tokenizer_t *tokenizer = silk_html_tokenizer_create(arena, input, strlen(input));
        silk_html_token_t *token = silk_html_tokenizer_next_token(tokenizer);
        TEST_ASSERT_EQ(HTML_TOKEN_START_TAG, token->type, "Single quoted attr");
        TEST_ASSERT(strcmp(token->attributes[0].value, "test") == 0, "Value is 'test'");
        silk_arena_destroy(arena);
    }

    /* Unquoted */
    {
        silk_arena_t *arena = silk_arena_create(4096);
        const char *input = "<div id=test>";
        silk_html_tokenizer_t *tokenizer = silk_html_tokenizer_create(arena, input, strlen(input));
        silk_html_token_t *token = silk_html_tokenizer_next_token(tokenizer);
        TEST_ASSERT_EQ(HTML_TOKEN_START_TAG, token->type, "Unquoted attr");
        TEST_ASSERT(strcmp(token->attributes[0].value, "test") == 0, "Value is 'test'");
        silk_arena_destroy(arena);
    }

    /* Self-closing */
    {
        silk_arena_t *arena = silk_arena_create(4096);
        const char *input = "<br/>";
        silk_html_tokenizer_t *tokenizer = silk_html_tokenizer_create(arena, input, strlen(input));
        silk_html_token_t *token = silk_html_tokenizer_next_token(tokenizer);
        TEST_ASSERT_EQ(HTML_TOKEN_START_TAG, token->type, "Self-closing tag");
        TEST_ASSERT(token->self_closing, "Self-closing flag set");
        silk_arena_destroy(arena);
    }

    /* Self-closing with space */
    {
        silk_arena_t *arena = silk_arena_create(4096);
        const char *input = "<br />";
        silk_html_tokenizer_t *tokenizer = silk_html_tokenizer_create(arena, input, strlen(input));
        silk_html_token_t *token = silk_html_tokenizer_next_token(tokenizer);
        TEST_ASSERT_EQ(HTML_TOKEN_START_TAG, token->type, "Self-closing tag with space");
        TEST_ASSERT(token->self_closing, "Self-closing flag set");
        silk_arena_destroy(arena);
    }

    /* Self-closing with attributes */
    {
        silk_arena_t *arena = silk_arena_create(4096);
        const char *input = "<img src=\"test.png\" />";
        silk_html_tokenizer_t *tokenizer = silk_html_tokenizer_create(arena, input, strlen(input));
        silk_html_token_t *token = silk_html_tokenizer_next_token(tokenizer);
        TEST_ASSERT_EQ(HTML_TOKEN_START_TAG, token->type, "Self-closing tag with attr");
        TEST_ASSERT(token->self_closing, "Self-closing flag set");
        TEST_ASSERT_EQ(1, token->attribute_count, "1 attribute");
        silk_arena_destroy(arena);
    }


    /* Summary */
    printf("\n================================\n");
    printf("Tests run: %d\n", tests_run);
    printf("Tests passed: %d\n", tests_passed);
    printf("Tests failed: %d\n", tests_run - tests_passed);

    if (tests_passed == tests_run) {
        printf("\n✓ ALL TESTS PASSED\n");
        return 0;
    } else {
        printf("\n✗ SOME TESTS FAILED\n");
        return 1;
    }
}
