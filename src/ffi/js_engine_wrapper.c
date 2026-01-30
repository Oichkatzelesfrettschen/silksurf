#include <silksurf/js_engine.h>

/*
 * C Wrapper Layer for Rust JS Engine FFI
 *
 * Bridges the C header interface (silk_js_*) to the Rust FFI implementation
 * (silksurf_*). This allows the Rust crate to maintain a consistent public
 * API while the C code uses the silk_js_* naming convention established in
 * the project.
 */

/* Rust FFI function declarations */
extern void* silksurf_engine_new(void);
extern void silksurf_engine_free(void* engine);
extern int silksurf_eval(void* engine, const char* code);

/* C API wrapper functions */

silk_js_context_t silk_js_init(void) {
    return silksurf_engine_new();
}

void silk_js_destroy(silk_js_context_t ctx) {
    silksurf_engine_free(ctx);
}

int silk_js_eval(silk_js_context_t ctx, const char* code) {
    return silksurf_eval(ctx, code);
}
