#ifndef SILKSURF_JS_ENGINE_H
#define SILKSURF_JS_ENGINE_H

#ifdef __cplusplus
extern "C" {
#endif

// Opaque context pointer
typedef void* silk_js_context_t;

// Initialize the engine
silk_js_context_t silk_js_init(void);

// Destroy the engine
void silk_js_destroy(silk_js_context_t ctx);

// Evaluate code (returns 0 on success)
int silk_js_eval(silk_js_context_t ctx, const char* code);

#ifdef __cplusplus
}
#endif

#endif // SILKSURF_JS_ENGINE_H
