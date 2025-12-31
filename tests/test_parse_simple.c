#include <stdio.h>
#include <string.h>
#include <hubbub/parser.h>

int main(void) {
    printf("Testing simple HTML parsing...\n");
    fflush(stdout);

    hubbub_parser *parser;
    hubbub_error err = hubbub_parser_create("UTF-8", false, &parser);
    printf("Parser created: %d\n", err);
    fflush(stdout);

    if (err != HUBBUB_OK) {
        printf("FAILED: Parser creation\n");
        return 1;
    }

    /* Try to parse without a tree handler */
    const char *html = "<html><body>Test</body></html>";
    printf("Calling parse_chunk...\n");
    fflush(stdout);

    err = hubbub_parser_parse_chunk(parser, (const uint8_t *)html, strlen(html));
    printf("parse_chunk returned: %d\n", err);
    fflush(stdout);

    printf("Calling parser_completed...\n");
    fflush(stdout);

    err = hubbub_parser_completed(parser);
    printf("parser_completed returned: %d\n", err);
    fflush(stdout);

    hubbub_parser_destroy(parser);
    printf("Parser destroyed - SUCCESS\n");

    return 0;
}
