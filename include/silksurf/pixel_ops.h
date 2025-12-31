#ifndef SILKSURF_PIXEL_OPS_H
#define SILKSURF_PIXEL_OPS_H

#include <stdint.h>
#include <stddef.h>

/* SIMD pixel operations - SSE2/AVX2 with C fallback */

/* Color format: ARGB32 (0xAARRGGBB) */
typedef uint32_t silk_color_t;

/* Common colors */
#define SILK_COLOR_TRANSPARENT  0x00000000
#define SILK_COLOR_BLACK        0xFF000000
#define SILK_COLOR_WHITE        0xFFFFFFFF
#define SILK_COLOR_RED          0xFFFF0000
#define SILK_COLOR_GREEN        0xFF00FF00
#define SILK_COLOR_BLUE         0xFF0000FF

/* Create color from components */
static inline silk_color_t silk_color(uint8_t a, uint8_t r, uint8_t g,
                                       uint8_t b) {
    return ((uint32_t)a << 24) | ((uint32_t)r << 16) |
           ((uint32_t)g << 8) | (uint32_t)b;
}

/* Pixel buffer operations */
void silk_fill_rect(silk_color_t *buffer, int buffer_width,
                     int x, int y, int width, int height,
                     silk_color_t color);

void silk_copy_pixels(const silk_color_t *src, int src_width,
                       silk_color_t *dst, int dst_width,
                       int x, int y, int width, int height);

void silk_blend_pixels(const silk_color_t *src, int src_width,
                        silk_color_t *dst, int dst_width,
                        int x, int y, int width, int height,
                        uint8_t alpha);

void silk_clear_buffer(silk_color_t *buffer, size_t pixel_count,
                        silk_color_t color);

/* Memcpy equivalent for pixel data (fast path) */
void silk_memcpy_pixels(const silk_color_t *src, silk_color_t *dst,
                         size_t pixel_count);

/* Statistics */
int silk_cpu_has_sse2(void);
int silk_cpu_has_avx2(void);
const char *silk_pixel_ops_backend(void);

#endif
