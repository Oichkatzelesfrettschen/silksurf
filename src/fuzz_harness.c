#include <unistd.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include "silksurf/html_tokenizer.h"
#include "silksurf/allocator.h"

/* AFL++ Persistent Mode */
#ifdef __AFL_HAVE_MANUAL_CONTROL
__AFL_FUZZ_INIT();
#endif

#ifndef __AFL_LOOP
#define __AFL_LOOP(x) ((x)--)
#endif

int main(void) {
    /* Setup arena - reuse or recreate? 
       For fuzzing, recreate per loop is safer to catch leaks, 
       but reuse is faster. We recreate to be thorough. */
    
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

    int loop_count = 1;
    while (__AFL_LOOP(loop_count)) {
        size_t len;
#ifdef __AFL_HAVE_MANUAL_CONTROL
        len = __AFL_FUZZ_TESTCASE_LEN;
#else
        len = read_len;
#endif

        if (len == 0) continue;

        /* 1. Create a transient arena for this parse run */
        silk_arena_t *arena = silk_arena_create(len * 4 + 4096);
        if (!arena) continue;

        /* 2. Initialize tokenizer */
        silk_html_tokenizer_t *tok = silk_html_tokenizer_create(arena, (const char *)buf, len);
        if (tok) {
            /* 3. Consume all tokens to exercise the full state machine */
            silk_html_token_t *token;
            while ((token = silk_html_tokenizer_next_token(tok))) {
                if (token->type == HTML_TOKEN_EOF) break;
            }
            
            /* 4. Cleanup tokenizer context (arena handle) */
            silk_html_tokenizer_destroy(tok);
        }

        /* 5. Print memory report for the 'Weigh-in' pass */
        silk_arena_stats(arena);

        /* 6. Destroy arena - frees everything at once */
        silk_arena_destroy(arena);
        
#ifndef __AFL_HAVE_MANUAL_CONTROL
        break; /* Only one loop if not in AFL */
#endif
    }

    return 0;
}
