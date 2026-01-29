/* Test CPUID detection for SIMD features */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "silksurf/pixel_ops.h"

int main(void) {
    printf("=== SilkSurf SIMD Feature Detection Test ===\n\n");

    /* Detect CPU features */
    int has_sse2 = silk_cpu_has_sse2();
    int has_avx2 = silk_cpu_has_avx2();
    const char *backend = silk_pixel_ops_backend();

    printf("CPU Features:\n");
    printf("  SSE2: %s\n", has_sse2 ? "YES" : "NO");
    printf("  AVX2: %s\n", has_avx2 ? "YES" : "NO");
    printf("\nSelected Backend: %s\n", backend);

    /* Verify backend selection is consistent */
    if (has_avx2 && strcmp(backend, "AVX2") != 0) {
        fprintf(stderr, "ERROR: AVX2 detected but backend is %s\n", backend);
        return 1;
    }

    if (!has_avx2 && has_sse2 && strcmp(backend, "SSE2") != 0) {
        fprintf(stderr, "ERROR: SSE2 detected but backend is %s\n", backend);
        return 1;
    }

    if (!has_sse2 && !has_avx2 && strcmp(backend, "C") != 0) {
        fprintf(stderr, "ERROR: No SIMD detected but backend is %s\n", backend);
        return 1;
    }

    printf("\n[PASS] CPU feature detection working correctly\n");
    return 0;
}
