#include <unistd.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include "silksurf/config.h"

#ifndef __AFL_LOOP
#define __AFL_LOOP(x) (0)
#endif

#include "silksurf/css_tokenizer.h"
#include "silksurf/allocator.h"

/* AFL++ Persistent Mode */
#ifdef __AFL_HAVE_MANUAL_CONTROL
__AFL_FUZZ_INIT();
#endif

int main(void) {
#ifdef __AFL_HAVE_MANUAL_CONTROL
    __AFL_INIT();
#endif

    unsigned char *buf = NULL;
#ifdef __AFL_HAVE_MANUAL_CONTROL
    buf = __AFL_FUZZ_TESTCASE_BUF;
#else
    /* Fallback for non-AFL compilation */
    static unsigned char fallback_buf[1024*1024];
    size_t read_len = fread(fallback_buf, 1, sizeof(fallback_buf), stdin);
    buf = fallback_buf;
#endif

    /* 1. Create a persistent arena for the fuzzing session */
    silk_arena_t *arena = silk_arena_create(1024 * 1024);
    if (!arena) return 1;

    while (__AFL_LOOP(10000)) {
        size_t len;
#ifdef __AFL_HAVE_MANUAL_CONTROL
        len = __AFL_FUZZ_TESTCASE_LEN;
#else
        len = read_len;
#endif

        if (len == 0) continue;

        /* 2. Zero-cost reset for reuse */
        silk_arena_reset(arena);

        /* 3. Initialize native tokenizer */
        silk_css_tokenizer_t *tok = silk_css_tokenizer_create(arena, (const char *)buf, len);
        if (tok) {
            /* 4. Consume all tokens to exercise the state machine */
            silk_css_token_t *token;
            while ((token = silk_css_tokenizer_next_token(tok))) {
                if (token->type == CSS_TOK_EOF) break;
            }
        }
        
#ifndef __AFL_HAVE_MANUAL_CONTROL
        break;
#endif
    }

    /* 5. Final cleanup */
    silk_arena_destroy(arena);

    return 0;
}
