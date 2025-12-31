#ifndef SILKSURF_CONFIG_H
#define SILKSURF_CONFIG_H

/* SilkSurf Configuration */

/* Memory targets */
#define SILK_TARGET_MEMORY_MB 10
#define SILK_ARENA_SIZE (1024*1024*20)  /* 20MB arena for all allocations */

/* Performance targets */
#define SILK_TARGET_STARTUP_MS 500
#define SILK_TARGET_FPS 60
#define SILK_TARGET_IDLE_CPU_PCT 5

/* Rendering */
#define SILK_SCREEN_WIDTH 1024
#define SILK_SCREEN_HEIGHT 768
#define SILK_PIXEL_FORMAT SILK_RGBA32

/* Enable optimization features */
#define SILK_USE_DAMAGE_TRACKING 1
#define SILK_USE_PIXMAP_CACHE 1
#define SILK_USE_SIMD 1
#define SILK_USE_XSHM 1

/* Debug flags */
#define SILK_DEBUG 0
#define SILK_PROFILE 1

#endif
