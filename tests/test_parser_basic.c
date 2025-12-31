#include <stdio.h>
#include <hubbub/parser.h>

int main(void) {
    printf("Testing basic parser creation...\n");
    fflush(stdout);

    hubbub_parser *parser;
    hubbub_error err = hubbub_parser_create("UTF-8", false, &parser);

    printf("Parser creation returned: %d\n", err);
    printf("Parser pointer: %p\n", (void *)parser);
    fflush(stdout);

    if (err != HUBBUB_OK) {
        printf("FAILED: Parser creation failed\n");
        return 1;
    }

    printf("SUCCESS: Parser created\n");

    hubbub_parser_destroy(parser);
    printf("Parser destroyed\n");

    return 0;
}
