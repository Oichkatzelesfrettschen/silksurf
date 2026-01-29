#include <string.h>
#include <stdlib.h>
#include "silksurf/pixel_ops.h"

/* SIMD pixel operations - fast rendering with fallback to C */

/* CPU feature detection */
static int detected_sse2 = -1;
static int detected_avx2 = -1;

/* CPUID intrinsic wrapper for x86/x86_64 CPU feature detection */
#if defined(__x86_64__) || defined(__i386__)
#include <cpuid.h>

static void cpuid(unsigned int leaf, unsigned int *eax, unsigned int *ebx,
                   unsigned int *ecx, unsigned int *edx) {
    __cpuid_count(leaf, 0, *eax, *ebx, *ecx, *edx);
}

static void detect_cpu_features(void) {
    if (detected_sse2 != -1)
        return;  /* Already detected */

    detected_sse2 = 0;
    detected_avx2 = 0;

    /* Query CPU feature flags via CPUID instruction
     * Leaf 0x1: Processor Info and Feature Bits
     * EDX bit 26: SSE2 support
     * Leaf 0x7: Extended Features (subleaf 0)
     * EBX bit 5: AVX2 support
     */

    unsigned int eax, ebx, ecx, edx;

    /* Get maximum supported leaf */
    cpuid(0, &eax, &ebx, &ecx, &edx);
    unsigned int max_leaf = eax;

    if (max_leaf >= 1) {
        /* Check SSE2 support (leaf 1, EDX bit 26) */
        cpuid(1, &eax, &ebx, &ecx, &edx);
        if (edx & (1 << 26)) {
            detected_sse2 = 1;
        }
    }

    if (max_leaf >= 7) {
        /* Check AVX2 support (leaf 7, subleaf 0, EBX bit 5) */
        cpuid(7, &eax, &ebx, &ecx, &edx);
        if (ebx & (1 << 5)) {
            detected_avx2 = 1;
        }
    }
}

#else
/* Non-x86 architectures: disable SIMD */
static void detect_cpu_features(void) {
    if (detected_sse2 != -1)
        return;

    detected_sse2 = 0;
    detected_avx2 = 0;
}
#endif

int silk_cpu_has_sse2(void) {
    detect_cpu_features();
    return detected_sse2;
}

int silk_cpu_has_avx2(void) {
    detect_cpu_features();
    return detected_avx2;
}

const char *silk_pixel_ops_backend(void) {
    detect_cpu_features();
    if (detected_avx2)
        return "AVX2";
    if (detected_sse2)
        return "SSE2";
    return "C";
}

/* ============================================================
   FALLBACK C IMPLEMENTATIONS (portable, works everywhere)
   ============================================================ */

/* Fill rectangle with solid color */
void silk_fill_rect(silk_color_t *buffer, int buffer_width,
                     int x, int y, int width, int height,
                     silk_color_t color) {
    if (!buffer || width <= 0 || height <= 0 || x < 0 || y < 0)
        return;

    for (int row = 0; row < height; row++) {
        silk_color_t *row_ptr = buffer + (y + row) * buffer_width + x;
        for (int col = 0; col < width; col++) {
            row_ptr[col] = color;
        }
    }
}

/* Copy pixels from source to destination */
void silk_copy_pixels(const silk_color_t *src, int src_width,
                       silk_color_t *dst, int dst_width,
                       int x, int y, int width, int height) {
    if (!src || !dst || width <= 0 || height <= 0)
        return;

    for (int row = 0; row < height; row++) {
        const silk_color_t *src_ptr = src + row * src_width;
        silk_color_t *dst_ptr = dst + (y + row) * dst_width + x;
        memcpy(dst_ptr, src_ptr, width * sizeof(silk_color_t));
    }
}

/* Blend pixels with alpha blending */
void silk_blend_pixels(const silk_color_t *src, int src_width,
                        silk_color_t *dst, int dst_width,
                        int x, int y, int width, int height,
                        uint8_t alpha) {
    if (!src || !dst || width <= 0 || height <= 0)
        return;

    if (alpha == 255) {
        /* Full opacity - just copy */
        silk_copy_pixels(src, src_width, dst, dst_width, x, y, width, height);
        return;
    }

    if (alpha == 0)
        return;  /* Fully transparent - no-op */

    /* Blend with alpha */
    uint32_t blend_factor = alpha;
    uint32_t inv_factor = 255 - alpha;

    for (int row = 0; row < height; row++) {
        const silk_color_t *src_ptr = src + row * src_width;
        silk_color_t *dst_ptr = dst + (y + row) * dst_width + x;

        for (int col = 0; col < width; col++) {
            silk_color_t s = src_ptr[col];
            silk_color_t d = dst_ptr[col];

            /* Unpack colors */
            uint32_t sr = (s >> 16) & 0xFF;
            uint32_t sg = (s >> 8) & 0xFF;
            uint32_t sb = s & 0xFF;
            uint32_t sa = (s >> 24) & 0xFF;

            uint32_t dr = (d >> 16) & 0xFF;
            uint32_t dg = (d >> 8) & 0xFF;
            uint32_t db = d & 0xFF;
            uint32_t da = (d >> 24) & 0xFF;

            /* Blend components */
            uint32_t r = (sr * blend_factor + dr * inv_factor) / 255;
            uint32_t g = (sg * blend_factor + dg * inv_factor) / 255;
            uint32_t b = (sb * blend_factor + db * inv_factor) / 255;
            uint32_t a = (sa * blend_factor + da * inv_factor) / 255;

            /* Repack */
            dst_ptr[col] = (a << 24) | (r << 16) | (g << 8) | b;
        }
    }
}

/* Clear entire buffer */
void silk_clear_buffer(silk_color_t *buffer, size_t pixel_count,
                        silk_color_t color) {
    if (!buffer || pixel_count == 0)
        return;

    /* Fast path for black (common case) */
    if (color == 0) {
        memset(buffer, 0, pixel_count * sizeof(silk_color_t));
        return;
    }

    /* Fallback: fill loop */
    for (size_t i = 0; i < pixel_count; i++)
        buffer[i] = color;
}

/* Fast memcpy for pixel data */
void silk_memcpy_pixels(const silk_color_t *src, silk_color_t *dst,
                         size_t pixel_count) {
    if (src && dst && pixel_count > 0)
        memcpy(dst, src, pixel_count * sizeof(silk_color_t));
}

/* ============================================================
   SSE2 OPTIMIZATIONS (when available)
   ============================================================ */

#ifdef __SSE2__
#include <emmintrin.h>

/* SSE2 optimized fill rectangle - 4x speedup */
void silk_fill_rect_sse2(silk_color_t *buffer, int buffer_width,
                          int x, int y, int width, int height,
                          silk_color_t color) {
    if (!buffer || width <= 0 || height <= 0)
        return;

    __m128i color_vec = _mm_set1_epi32(color);

    for (int row = 0; row < height; row++) {
        silk_color_t *row_ptr = buffer + (y + row) * buffer_width + x;

        /* Process 4 pixels at a time */
        int col = 0;
        for (; col + 4 <= width; col += 4) {
            _mm_storeu_si128((__m128i *)(row_ptr + col), color_vec);
        }

        /* Handle remainder */
        for (; col < width; col++) {
            row_ptr[col] = color;
        }
    }
}

#endif  /* __SSE2__ */

/* ============================================================
   AVX2 OPTIMIZATIONS (when available)
   ============================================================ */

#ifdef __AVX2__
#include <immintrin.h>

/* AVX2 optimized clear buffer - 8x speedup */
void silk_clear_buffer_avx2(silk_color_t *buffer, size_t pixel_count,
                             silk_color_t color) {
    if (!buffer || pixel_count == 0)
        return;

    if (color == 0) {
        memset(buffer, 0, pixel_count * sizeof(silk_color_t));
        return;
    }

    __m256i color_vec = _mm256_set1_epi32(color);

    size_t i = 0;
    for (; i + 8 <= pixel_count; i += 8) {
        _mm256_storeu_si256((__m256i *)(buffer + i), color_vec);
    }

    /* Handle remainder */
    for (; i < pixel_count; i++) {
        buffer[i] = color;
    }
}

#endif  /* __AVX2__ */
