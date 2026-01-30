#include <string.h>
#include "silksurf/neural_bpe.h"

SilkBPETokenizer *silk_bpe_create(silk_arena_t *arena) {
    SilkBPETokenizer *bpe = silk_arena_alloc(arena, sizeof(SilkBPETokenizer));
    if (!bpe) return NULL;
    
    bpe->arena = arena;
    bpe->root = silk_arena_alloc(arena, sizeof(SilkBPENode));
    if (bpe->root) memset(bpe->root, 0, sizeof(SilkBPENode));
    
    return bpe;
}

void silk_bpe_add_merge(SilkBPETokenizer *bpe, const char *sequence, uint16_t id) {
    SilkBPENode *curr = bpe->root;
    const unsigned char *p = (const unsigned char *)sequence;
    
    while (*p) {
        if (!curr->children[*p]) {
            curr->children[*p] = silk_arena_alloc(bpe->arena, sizeof(SilkBPENode));
            memset(curr->children[*p], 0, sizeof(SilkBPENode));
        }
        curr = curr->children[*p];
        p++;
    }
    curr->token_id = id;
}

SilkBPEOutput silk_bpe_encode(SilkBPETokenizer *bpe, const char *input, size_t len) {
    SilkBPEOutput out;
    out.tokens = silk_arena_alloc(bpe->arena, sizeof(uint16_t) * len);
    out.count = 0;
    
    const unsigned char *p = (const unsigned char *)input;
    const unsigned char *end = p + len;
    
    while (p < end) {
        SilkBPENode *curr = bpe->root;
        uint16_t best_token = *p; /* Fallback to raw byte */
        size_t match_len = 1;
        
        /* Greedy search for longest prefix in Trie */
        const unsigned char *walker = p;
        while (walker < end && curr->children[*walker]) {
            curr = curr->children[*walker];
            walker++;
            if (curr->token_id != 0) {
                best_token = curr->token_id;
                match_len = walker - p;
            }
        }
        
        out.tokens[out.count++] = best_token;
        p += match_len;
    }
    
    return out;
}
