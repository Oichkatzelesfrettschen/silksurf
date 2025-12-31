#ifndef SILK_NEURAL_BPE_H
#define SILK_NEURAL_BPE_H

#include <stdint.h>
#include <stddef.h>
#include "silksurf/allocator.h"

/* 
 * SilkBPE: High-performance Byte Pair Encoding for HTML5
 * Mapping common 4-12 byte sequences into single 16-bit tokens.
 */

#define SILK_BPE_MAX_TOKEN 1024
#define SILK_BPE_ROOT_NODES 256

typedef struct SilkBPENode {
    uint16_t token_id;
    struct SilkBPENode *children[SILK_BPE_ROOT_NODES];
} SilkBPENode;

typedef struct {
    SilkBPENode *root;
    silk_arena_t *arena;
} SilkBPETokenizer;

/* Lifecycle */
SilkBPETokenizer *silk_bpe_create(silk_arena_t *arena);
void silk_bpe_add_merge(SilkBPETokenizer *bpe, const char *sequence, uint16_t id);

/* Core Encoding */
typedef struct {
    uint16_t *tokens;
    size_t count;
} SilkBPEOutput;

SilkBPEOutput silk_bpe_encode(SilkBPETokenizer *bpe, const char *input, size_t len);

#endif
