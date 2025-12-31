#include <stdio.h>
#include <string.h>
#include <time.h>
#include "silksurf/neural_bpe.h"
#include "silksurf/allocator.h"

int main(void) {
    silk_arena_t *arena = silk_arena_create(1024 * 1024);
    SilkBPETokenizer *bpe = silk_bpe_create(arena);

    /* 1. High-Density Vocab Training (Mock) */
    silk_bpe_add_merge(bpe, "<!DOCTYPE html>", 256);
    silk_bpe_add_merge(bpe, "<html>", 257);
    silk_bpe_add_merge(bpe, "<body>", 258);
    silk_bpe_add_merge(bpe, "</div>", 259);
    silk_bpe_add_merge(bpe, "</span>", 260);
    silk_bpe_add_merge(bpe, " class=\"", 261);
    silk_bpe_add_merge(bpe, " id=\"", 262);
    silk_bpe_add_merge(bpe, "<div>", 263);

    const char *html = "<!DOCTYPE html><html><body><div class=\"test\">Hello</div></body></html>";
    size_t raw_len = strlen(html);

    /* 2. Benchmark the Encode */
    clock_t start = clock();
    SilkBPEOutput out = silk_bpe_encode(bpe, html, raw_len);
    clock_t end = clock();

    double cpu_time = ((double)(end - start)) / CLOCKS_PER_SEC;

    printf("[NEURAL BPE BENCHMARK]\n");
    printf("-------------------------------\n");
    printf("  Raw Bytes:      %zu\n", raw_len);
    printf("  BPE Tokens:     %zu\n", out.count);
    printf("  Compression:    %.2fx\n", (double)raw_len / out.count);
    printf("  Processing Time: %.8f s\n", cpu_time);
    printf("-------------------------------\n");

    /* Print first 5 tokens for verification */
    printf("  Tokens: ");
    for (size_t i = 0; i < (out.count < 10 ? out.count : 10); i++) {
        printf("%u ", out.tokens[i]);
    }
    printf("...\n\n");

    silk_arena_destroy(arena);
    return 0;
}
