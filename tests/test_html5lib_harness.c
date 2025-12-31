/*
 * SilkSurf HTML5 Tokenizer Test Harness
 *
 * Parses html5lib JSON tests and runs them against the SilkSurf tokenizer.
 *
 * Copyright (c) 2025 SilkSurf Project
 * SPDX-License-Identifier: MIT
 */

#include "src/document/html_tokenizer.h"
#include "silksurf/allocator.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <ctype.h>
#include <stdbool.h>

/* ============================================================================
 * Minimal JSON Parser
 * ============================================================================ */

typedef enum {
    JSON_NULL,
    JSON_BOOL,
    JSON_NUMBER,
    JSON_STRING,
    JSON_ARRAY,
    JSON_OBJECT
} json_type_t;

typedef struct json_value_t json_value_t;

struct json_value_t {
    json_type_t type;
    union {
        bool boolean;
        double number;
        char *string;
        struct {
            json_value_t **values;
            size_t count;
        } array;
        struct {
            char **keys;
            json_value_t **values;
            size_t count;
        } object;
    } u;
};

/* Forward declarations */
static json_value_t *parse_json_value(const char **ptr);
static void free_json_value(json_value_t *val);

static void skip_whitespace(const char **ptr) {
    while (**ptr && isspace(**ptr)) {
        (*ptr)++;
    }
}

static char *parse_json_string(const char **ptr) {
    if (**ptr != '"') return NULL;
    (*ptr)++; /* Skip opening quote */

    const char *start = *ptr;
    char *result = malloc(4096); /* Adequate buffer for test strings */
    char *out = result;

    while (**ptr && **ptr != '"') {
        if (**ptr == '\\') {
            (*ptr)++;
            switch (**ptr) {
                case '"': *out++ = '"'; break;
                case '\\': *out++ = '\\'; break;
                case '/': *out++ = '/'; break;
                case 'b': *out++ = '\b'; break;
                case 'f': *out++ = '\f'; break;
                case 'n': *out++ = '\n'; break;
                case 'r': *out++ = '\r'; break;
                case 't': *out++ = '\t'; break;
                case 'u': {
                    /* Parse unicode hex */
                    (*ptr)++;
                    char hex[5] = {0};
                    memcpy(hex, *ptr, 4);
                    (*ptr) += 3;
                    unsigned int cp;
                    sscanf(hex, "%x", &cp);
                    /* Convert to UTF-8 */
                    if (cp < 0x80) {
                        *out++ = (char)cp;
                    } else if (cp < 0x800) {
                        *out++ = (char)(0xC0 | (cp >> 6));
                        *out++ = (char)(0x80 | (cp & 0x3F));
                    } else {
                        *out++ = (char)(0xE0 | (cp >> 12));
                        *out++ = (char)(0x80 | ((cp >> 6) & 0x3F));
                        *out++ = (char)(0x80 | (cp & 0x3F));
                    }
                    break;
                }
                default: *out++ = **ptr; break;
            }
        } else {
            *out++ = **ptr;
        }
        (*ptr)++;
    }

    if (**ptr == '"') (*ptr)++;
    *out = '\0';
    return result;
}

static json_value_t *parse_json_array(const char **ptr) {
    if (**ptr != '[') return NULL;
    (*ptr)++;

    json_value_t *val = calloc(1, sizeof(json_value_t));
    val->type = JSON_ARRAY;
    val->u.array.values = malloc(sizeof(json_value_t*) * 1024); /* Max 1024 items */

    skip_whitespace(ptr);
    if (**ptr == ']') {
        (*ptr)++;
        return val;
    }

    while (1) {
        val->u.array.values[val->u.array.count++] = parse_json_value(ptr);
        skip_whitespace(ptr);
        if (**ptr == ',') {
            (*ptr)++;
            skip_whitespace(ptr);
        } else if (**ptr == ']') {
            (*ptr)++;
            break;
        } else {
            break; /* Error */
        }
    }
    return val;
}

static json_value_t *parse_json_object(const char **ptr) {
    if (**ptr != '{') return NULL;
    (*ptr)++;

    json_value_t *val = calloc(1, sizeof(json_value_t));
    val->type = JSON_OBJECT;
    val->u.object.keys = malloc(sizeof(char*) * 256);
    val->u.object.values = malloc(sizeof(json_value_t*) * 256);

    skip_whitespace(ptr);
    if (**ptr == '}') {
        (*ptr)++;
        return val;
    }

    while (1) {
        val->u.object.keys[val->u.object.count] = parse_json_string(ptr);
        skip_whitespace(ptr);
        if (**ptr == ':') (*ptr)++;
        skip_whitespace(ptr);
        val->u.object.values[val->u.object.count] = parse_json_value(ptr);
        val->u.object.count++;

        skip_whitespace(ptr);
        if (**ptr == ',') {
            (*ptr)++;
            skip_whitespace(ptr);
        } else if (**ptr == '}') {
            (*ptr)++;
            break;
        } else {
            break; /* Error */
        }
    }
    return val;
}

static json_value_t *parse_json_value(const char **ptr) {
    skip_whitespace(ptr);
    json_value_t *val = calloc(1, sizeof(json_value_t));

    if (**ptr == '"') {
        val->type = JSON_STRING;
        val->u.string = parse_json_string(ptr);
    } else if (**ptr == '[') {
        free(val);
        return parse_json_array(ptr);
    } else if (**ptr == '{') {
        free(val);
        return parse_json_object(ptr);
    } else if (strncmp(*ptr, "true", 4) == 0) {
        val->type = JSON_BOOL;
        val->u.boolean = true;
        (*ptr) += 4;
    } else if (strncmp(*ptr, "false", 5) == 0) {
        val->type = JSON_BOOL;
        val->u.boolean = false;
        (*ptr) += 5;
    } else if (strncmp(*ptr, "null", 4) == 0) {
        val->type = JSON_NULL;
        (*ptr) += 4;
    } else {
        /* Number */
        val->type = JSON_NUMBER;
        char *end;
        val->u.number = strtod(*ptr, &end);
        *ptr = end;
    }
    return val;
}

static void free_json_value(json_value_t *val) {
    if (!val) return;
    if (val->type == JSON_STRING) {
        free(val->u.string);
    } else if (val->type == JSON_ARRAY) {
        for (size_t i = 0; i < val->u.array.count; i++) {
            free_json_value(val->u.array.values[i]);
        }
        free(val->u.array.values);
    } else if (val->type == JSON_OBJECT) {
        for (size_t i = 0; i < val->u.object.count; i++) {
            free(val->u.object.keys[i]);
            free_json_value(val->u.object.values[i]);
        }
        free(val->u.object.keys);
        free(val->u.object.values);
    }
    free(val);
}

static json_value_t *json_get_key(json_value_t *obj, const char *key) {
    if (obj->type != JSON_OBJECT) return NULL;
    for (size_t i = 0; i < obj->u.object.count; i++) {
        if (strcmp(obj->u.object.keys[i], key) == 0) {
            return obj->u.object.values[i];
        }
    }
    return NULL;
}

/* ============================================================================
 * Test Runner Logic
 * ============================================================================ */

char *read_file(const char *filename) {
    FILE *f = fopen(filename, "rb");
    if (!f) return NULL;
    fseek(f, 0, SEEK_END);
    long len = ftell(f);
    fseek(f, 0, SEEK_SET);
    char *buf = malloc(len + 1);
    fread(buf, 1, len, f);
    buf[len] = '\0';
    fclose(f);
    return buf;
}

void print_token(silk_html_token_t *token) {
    if (!token) {
        printf("NULL");
        return;
    }
    switch (token->type) {
        case HTML_TOKEN_DOCTYPE:
            printf("DOCTYPE %s", token->doctype_data ? token->doctype_data->name : "NULL");
            break;
        case HTML_TOKEN_START_TAG:
            printf("StartTag %s", token->tag_name);
            break;
        case HTML_TOKEN_END_TAG:
            printf("EndTag %s", token->tag_name);
            break;
        case HTML_TOKEN_COMMENT:
            printf("Comment %s", token->comment_data);
            break;
        case HTML_TOKEN_CHARACTER:
            printf("Character '%s'", token->character_data);
            break;
        case HTML_TOKEN_EOF:
            printf("EOF");
            break;
    }
}

int run_single_test(json_value_t *test, const char *initial_state_name) {
    json_value_t *desc = json_get_key(test, "description");
    json_value_t *input = json_get_key(test, "input");
    json_value_t *expected_output = json_get_key(test, "output");

    if (!input || !expected_output) return 0;

    silk_arena_t *arena = silk_arena_create(65536);
    silk_html_tokenizer_t *tokenizer = silk_html_tokenizer_create(
        arena, input->u.string, strlen(input->u.string)
    );

    if (initial_state_name) {
        silk_html_tokenizer_set_state(tokenizer, silk_html_tokenizer_state_from_name(initial_state_name));
    }

    int token_idx = 0;
    int passed = 1;
    char text_buffer[16384] = {0};
    bool collecting_text = false;

    while (1) {
        silk_html_token_t *token = silk_html_tokenizer_next_token(tokenizer);
        if (!token) {
            printf("  Got NULL token!\n");
            break;
        }
        printf("  Got token: %s\n", silk_html_token_type_name(token->type));
        if (collecting_text && (token->type != HTML_TOKEN_CHARACTER || token->type == HTML_TOKEN_EOF)) {
            if (token_idx >= expected_output->u.array.count) {
                printf("FAIL: %s [%s] - Extra character output '%s'\n", desc->u.string, initial_state_name ? initial_state_name : "Data", text_buffer);
                passed = 0;
                break;
            }
                        json_value_t *exp = expected_output->u.array.values[token_idx];
                        if (exp->type == JSON_ARRAY && strcmp(exp->u.array.values[0]->u.string, "Character") == 0) {
                            char *exp_text = exp->u.array.values[1]->u.string;
                            const char *got_text = text_buffer;
                            if (strcmp(got_text, exp_text) != 0) {
                                printf("FAIL: %s [%s] - Text mismatch. Expected '%s', got '%s'\n", desc->u.string, initial_state_name ? initial_state_name : "Data", exp_text, got_text);
                                passed = 0;
                            }
                            token_idx++;
                        }
             else {
                printf("FAIL: %s [%s] - Unexpected text output '%s'\n", desc->u.string, initial_state_name ? initial_state_name : "Data", text_buffer);
                passed = 0;
            }
            collecting_text = false;
        }

        if (token->type == HTML_TOKEN_EOF) break;

        if (token->type == HTML_TOKEN_CHARACTER) {
            if (!collecting_text) {
                collecting_text = true;
                text_buffer[0] = '\0';
            }
            if (token->character_data) {
                strcat(text_buffer, token->character_data);
            } else {
                printf("FAIL: %s [%s] - Character token has NULL data!\n", desc->u.string, initial_state_name ? initial_state_name : "Data");
                passed = 0;
            }
            continue;
        }

                /* Non-character token check */

                if (token_idx >= expected_output->u.array.count) {

                    printf("FAIL: %s [%s] - Extra token output ", desc->u.string, initial_state_name ? initial_state_name : "Data");

                    print_token(token); printf("\n");

                    passed = 0;

                    break;

                }

        

                json_value_t *exp = expected_output->u.array.values[token_idx];

                if (exp->type != JSON_ARRAY || exp->u.array.count == 0) {

                    printf("FAIL: %s [%s] - Expected token at index %d is invalid JSON\n", desc->u.string, initial_state_name ? initial_state_name : "Data", token_idx);

                    passed = 0;

                    break;

                }

                const char *exp_type = exp->u.array.values[0]->u.string;

        

                                if (token->type == HTML_TOKEN_START_TAG) {

        

                                    if (strcmp(exp_type, "StartTag") != 0) {

        

                                        printf("FAIL: %s [%s] - Expected %s, got StartTag\n", desc->u.string, initial_state_name ? initial_state_name : "Data", exp_type);

        

                                        passed = 0;

        

                                    } else if (exp->u.array.count > 1) {

        

                                        char *exp_name = exp->u.array.values[1]->u.string;

        

                                        const char *got_name = token->tag_name ? token->tag_name : "";

        

                                        if (strcmp(got_name, exp_name) != 0) {

        

                                            printf("FAIL: %s [%s] - Tag name mismatch. Exp %s, got %s\n", desc->u.string, initial_state_name ? initial_state_name : "Data", exp_name, got_name);

        

                                            passed = 0;

        

                                        }

        

                                        /* TODO: Verify attributes and self-closing flag */

        

                                    }

        

                                } else if (token->type == HTML_TOKEN_END_TAG) {

        

                                    if (strcmp(exp_type, "EndTag") != 0) {

        

                                        printf("FAIL: %s [%s] - Expected %s, got EndTag\n", desc->u.string, initial_state_name ? initial_state_name : "Data", exp_type);

        

                                        passed = 0;

        

                                    } else if (exp->u.array.count > 1) {

        

                                        char *exp_name = exp->u.array.values[1]->u.string;

        

                                        const char *got_name = token->tag_name ? token->tag_name : "";

        

                                        if (strcmp(got_name, exp_name) != 0) {

        

                                            printf("FAIL: %s [%s] - Tag name mismatch. Exp %s, got %s\n", desc->u.string, initial_state_name ? initial_state_name : "Data", exp_name, got_name);

        

                                            passed = 0;

        

                                        }

        

                                    }

                } else if (token->type == HTML_TOKEN_COMMENT) {

                     if (strcmp(exp_type, "Comment") != 0) {

                        printf("FAIL: %s [%s] - Expected %s, got Comment\n", desc->u.string, initial_state_name ? initial_state_name : "Data", exp_type);

                        passed = 0;

                     } else if (exp->u.array.count > 1) {

                        char *exp_data = exp->u.array.values[1]->u.string;

                        const char *got_data = token->comment_data ? token->comment_data : "";

                        if (strcmp(got_data, exp_data) != 0) {

                            printf("FAIL: %s [%s] - Comment mismatch. Exp '%s', got '%s'\n", desc->u.string, initial_state_name ? initial_state_name : "Data", exp_data, got_data);

                            passed = 0;

                        }

                     }

                } else if (token->type == HTML_TOKEN_DOCTYPE) {

                     if (strcmp(exp_type, "DOCTYPE") != 0) {

                        printf("FAIL: %s [%s] - Expected %s, got DOCTYPE\n", desc->u.string, initial_state_name ? initial_state_name : "Data", exp_type);

                        passed = 0;

                     }

                }

        

        token_idx++;
        if (!passed) break;
    }

    if (passed && token_idx < expected_output->u.array.count) {
        printf("FAIL: %s [%s] - Missing output tokens (got %d, expected %zu)\n", desc->u.string, initial_state_name ? initial_state_name : "Data", token_idx, expected_output->u.array.count);
        passed = 0;
    }

    silk_html_tokenizer_destroy(tokenizer);
    silk_arena_destroy(arena);
    return passed;
}

int run_test(json_value_t *test) {
    json_value_t *desc = json_get_key(test, "description");
    printf("Test: %s\n", desc ? desc->u.string : "Unknown");
    json_value_t *initial_states = json_get_key(test, "initialStates");
    int passed = 1;

    if (initial_states && initial_states->type == JSON_ARRAY) {
        for (size_t i = 0; i < initial_states->u.array.count; i++) {
            if (!run_single_test(test, initial_states->u.array.values[i]->u.string)) {
                passed = 0;
            }
        }
    } else {
        passed = run_single_test(test, NULL);
    }
    return passed;
}

int main(int argc, char **argv) {
    const char *file = "tests/html5lib_tokenizer_test1.json";
    char *json_str = read_file(file);
    if (!json_str) {
        printf("Failed to read %s\n", file);
        return 1;
    }

    const char *ptr = json_str;
    json_value_t *root = parse_json_value(&ptr);
    free(json_str);

    if (!root || root->type != JSON_OBJECT) {
        printf("Invalid JSON\n");
        return 1;
    }

    json_value_t *tests = json_get_key(root, "tests");
    if (!tests || tests->type != JSON_ARRAY) {
        printf("No tests array found\n");
        return 1;
    }

    printf("Running %zu tests from %s\n", tests->u.array.count, file);

    int passed = 0;
    int failed = 0;

    for (size_t i = 0; i < tests->u.array.count; i++) {
        if (run_test(tests->u.array.values[i])) {
            passed++;
        } else {
            failed++;
        }
    }

    printf("\nResults: %d passed, %d failed\n", passed, failed);
    free_json_value(root);
    return failed > 0 ? 1 : 0;
}
