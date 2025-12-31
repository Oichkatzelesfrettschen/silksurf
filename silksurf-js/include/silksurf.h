/**
 * SilkSurfJS - Pure Rust JavaScript Engine
 *
 * C API for embedding SilkSurfJS in non-Rust applications.
 *
 * Thread Safety: Currently single-threaded. Each engine instance
 * must be used from one thread only.
 *
 * Error Handling: Functions return status codes or null pointers.
 * Use silksurf_last_error() to get error details.
 *
 * Example:
 *   SilkSurfEngine* engine = silksurf_engine_new();
 *   SilkSurfStatus status = silksurf_eval(engine, "console.log('Hello')");
 *   silksurf_engine_free(engine);
 */


#ifndef SILKSURF_JS_H
#define SILKSURF_JS_H

#pragma once

/* Generated with cbindgen:0.29.2 */

#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

/**
 * Object header size in bytes
 */
#define HEADER_SIZE 16

/**
 * Minimum allocation size (header + minimum payload)
 */
#define MIN_ALLOC_SIZE 32

/**
 * Alignment for all allocations
 */
#define ALIGNMENT 8

/**
 * Large object threshold (objects >= this go to large object space)
 */
#define LARGE_OBJECT_THRESHOLD 4096

/**
 * Status codes
 */
typedef enum SilkSurfStatus {
  Ok = 0,
  ErrorParse = 1,
  ErrorCompile = 2,
  ErrorRuntime = 3,
  ErrorMemory = 4,
  ErrorInvalidArg = 5,
} SilkSurfStatus;

/**
 * Binding power for operators (higher = binds tighter)
 */
typedef struct BindingPower BindingPower;

/**
 * Property attributes (ECMA-262 property descriptor flags)
 */
typedef struct PropertyAttributes PropertyAttributes;

/**
 * Opaque engine handle
 */
typedef struct SilkSurfEngine SilkSurfEngine;

/**
 * Opaque compiled script handle
 */
typedef struct SilkSurfScript SilkSurfScript;

/**
 * Get heap statistics.
 */
typedef struct SilkSurfHeapStats {
  uintptr_t bytes_allocated;
  uintptr_t bytes_threshold;
  uintptr_t gc_count;
} SilkSurfHeapStats;

/**
 * Result value from script execution
 */
typedef struct SilkSurfValue {
  /**
   * Type tag: 0=undefined, 1=null, 2=bool, 3=number, 4=string, 5=object
   */
  int tag;
  /**
   * Numeric value (for bool: 0/1, for number: the value)
   */
  double number;
  /**
   * String value (null-terminated, owned by engine)
   */
  const char *string;
} SilkSurfValue;















































/**
 * Compile JavaScript source code to a script handle.
 * Returns null on parse/compile error.
 */
 struct SilkSurfScript *silksurf_compile(struct SilkSurfEngine *engine, const char *source);

/**
 * Destroy an engine instance.
 * Safe to call with null.
 */
 void silksurf_engine_free(struct SilkSurfEngine *engine);

/**
 * Create a new engine instance.
 * Returns null on failure.
 */
 struct SilkSurfEngine *silksurf_engine_new(void);

/**
 * Evaluate JavaScript source code directly.
 * Convenience wrapper around compile + run.
 */
 enum SilkSurfStatus silksurf_eval(struct SilkSurfEngine *engine, const char *source);

/**
 * Trigger garbage collection.
 * Currently a no-op; GC runs automatically when needed.
 */
 void silksurf_gc(struct SilkSurfEngine *_engine);


enum SilkSurfStatus silksurf_heap_stats(const struct SilkSurfEngine *_engine,
                                        struct SilkSurfHeapStats *stats);

/**
 * Get the last error message, or null if no error.
 * The returned string is valid until the next API call.
 */
 const char *silksurf_last_error(void);

/**
 * Execute a compiled script.
 * Returns status code.
 */
 enum SilkSurfStatus silksurf_run(struct SilkSurfEngine *engine, struct SilkSurfScript *script);

/**
 * Free a compiled script.
 * Safe to call with null.
 */
 void silksurf_script_free(struct SilkSurfScript *script);

/**
 * Get the number of instructions in a compiled script.
 * Note: Returns chunk index, not instruction count (requires engine access).
 */
 int silksurf_script_instruction_count(const struct SilkSurfScript *script);

/**
 * Get the library version string.
 */
 const char *silksurf_version(void);

#endif  /* SILKSURF_JS_H */
