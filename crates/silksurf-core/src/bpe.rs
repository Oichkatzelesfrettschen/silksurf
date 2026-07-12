/*
 * bpe.rs -- greedy byte-pair-encoding tokenizer over a byte trie.
 *
 * BpeTokenizer maps registered byte sequences (typically common 4-12 byte
 * HTML fragments such as "<div>" or " class=\"") to single u16 token ids.
 * encode() walks the trie greedily for the longest registered prefix at
 * each position and falls back to the raw byte value (0-255) when no
 * merge matches, so every input encodes losslessly into u16 tokens and
 * registered ids conventionally start at 256.
 *
 * The trie stores nodes in one Vec and links children by u32 index; index
 * 0 is the root, which is never a child, so 0 doubles as the absent-child
 * sentinel. Each node carries a dense [u32; 256] child table -- the same
 * space-for-speed trade the retired C implementation made
 * (src/neural/bpe.c, re-homed under AD-024 step 2; scope AD-006).
 *
 * Token id 0 is reserved: the trie uses it internally as "no token ends
 * at this node", and byte 0 already encodes itself via the raw-byte
 * fallback.
 */

const NO_CHILD: u32 = 0;
const NO_TOKEN: u16 = 0;

struct BpeNode {
    token_id: u16,
    children: [u32; 256],
}

impl BpeNode {
    fn new() -> Self {
        Self {
            token_id: NO_TOKEN,
            children: [NO_CHILD; 256],
        }
    }
}

pub struct BpeTokenizer {
    nodes: Vec<BpeNode>,
}

impl BpeTokenizer {
    #[must_use]
    pub fn new() -> Self {
        Self {
            nodes: vec![BpeNode::new()],
        }
    }

    /// Register `sequence` as a single token with the given nonzero id.
    ///
    /// Empty sequences and id 0 are ignored: byte 0 and every other raw
    /// byte already encode themselves, and id 0 is the internal
    /// no-token sentinel.
    pub fn add_merge(&mut self, sequence: &[u8], id: u16) {
        if sequence.is_empty() || id == NO_TOKEN {
            debug_assert!(
                !sequence.is_empty() && id != NO_TOKEN,
                "BPE merge needs a non-empty sequence and a nonzero id"
            );
            return;
        }
        let mut current = 0usize;
        for &byte in sequence {
            let slot = self.nodes[current].children[usize::from(byte)];
            current = if slot == NO_CHILD {
                let next = self.nodes.len();
                self.nodes.push(BpeNode::new());
                self.nodes[current].children[usize::from(byte)] = next as u32;
                next
            } else {
                slot as usize
            };
        }
        self.nodes[current].token_id = id;
    }

    /// Encode `input` into tokens: greedy longest registered prefix at
    /// each position, raw byte value as the fallback token.
    #[must_use]
    pub fn encode(&self, input: &[u8]) -> Vec<u16> {
        let mut tokens = Vec::with_capacity(input.len());
        let mut position = 0usize;
        while position < input.len() {
            let mut current = 0usize;
            let mut best_token = u16::from(input[position]);
            let mut match_len = 1usize;
            for (offset, &byte) in input[position..].iter().enumerate() {
                let slot = self.nodes[current].children[usize::from(byte)];
                if slot == NO_CHILD {
                    break;
                }
                current = slot as usize;
                if self.nodes[current].token_id != NO_TOKEN {
                    best_token = self.nodes[current].token_id;
                    match_len = offset + 1;
                }
            }
            tokens.push(best_token);
            position += match_len;
        }
        tokens
    }
}

impl Default for BpeTokenizer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn html_vocab() -> BpeTokenizer {
        let mut bpe = BpeTokenizer::new();
        bpe.add_merge(b"<!DOCTYPE html>", 256);
        bpe.add_merge(b"<html>", 257);
        bpe.add_merge(b"<body>", 258);
        bpe.add_merge(b"</div>", 259);
        bpe.add_merge(b"</span>", 260);
        bpe.add_merge(b" class=\"", 261);
        bpe.add_merge(b" id=\"", 262);
        bpe.add_merge(b"<div>", 263);
        bpe
    }

    #[test]
    fn raw_bytes_encode_as_themselves() {
        let bpe = BpeTokenizer::new();
        assert_eq!(bpe.encode(b"abc"), vec![97, 98, 99]);
        assert_eq!(bpe.encode(&[0u8, 255u8]), vec![0, 255]);
    }

    #[test]
    fn empty_input_encodes_to_nothing() {
        assert!(html_vocab().encode(b"").is_empty());
    }

    #[test]
    fn registered_sequence_encodes_as_one_token() {
        let bpe = html_vocab();
        assert_eq!(bpe.encode(b"<div>"), vec![263]);
    }

    #[test]
    fn longest_prefix_wins_over_shorter_registered_prefix() {
        let mut bpe = BpeTokenizer::new();
        bpe.add_merge(b"<d", 300);
        bpe.add_merge(b"<div>", 301);
        assert_eq!(bpe.encode(b"<div>"), vec![301]);
        // The shorter merge still applies where the longer one cannot.
        assert_eq!(bpe.encode(b"<dx"), vec![300, u16::from(b'x')]);
    }

    #[test]
    fn partial_match_falls_back_to_raw_byte_and_reconsumes() {
        let bpe = html_vocab();
        // "<divx" walks four trie levels without completing "<div>", so
        // '<' emits as a raw byte and scanning resumes at 'd'.
        assert_eq!(
            bpe.encode(b"<divx"),
            vec![
                u16::from(b'<'),
                u16::from(b'd'),
                u16::from(b'i'),
                u16::from(b'v'),
                u16::from(b'x')
            ]
        );
    }

    #[test]
    fn bench_fixture_compresses_and_round_trips_structure() {
        let bpe = html_vocab();
        let html: &[u8] =
            b"<!DOCTYPE html><html><body><div class=\"test\">Hello</div></body></html>";
        let tokens = bpe.encode(html);
        assert!(tokens.starts_with(&[256, 257, 258]));
        // The fixture's div carries attributes ("<div class=..."), so the
        // standalone "<div>" merge (263) cannot match; the attribute merge
        // and the closing-tag merge do.
        assert!(tokens.contains(&261) && tokens.contains(&259));
        assert!(!tokens.contains(&263));
        // Merges shrink the fixture well below its raw byte length.
        assert!(tokens.len() < html.len() / 2, "expected >2x compression");
    }

    #[test]
    fn zero_id_and_empty_sequence_merges_are_ignored() {
        let mut bpe = BpeTokenizer::new();
        // Release-profile contract: both degenerate registrations are
        // no-ops. (In debug builds they debug_assert; this test guards
        // the release path via the public behavior only.)
        if cfg!(not(debug_assertions)) {
            bpe.add_merge(b"", 300);
            bpe.add_merge(b"ab", 0);
            assert_eq!(bpe.encode(b"ab"), vec![97, 98]);
        }
    }
}
