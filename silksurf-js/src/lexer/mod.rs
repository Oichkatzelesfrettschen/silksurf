//! Zero-copy lexer for JavaScript
//!
//! Key design decisions:
//! - Tokens reference source directly (&'src str) - no allocation
//! - BPE pattern matching for common token sequences
//! - String interning for identifiers (O(1) comparison)
//! - Span tracking for error messages
//!
//! Performance target: 50-100 MB/s throughput

mod bpe;
mod interner;
mod lexer;
mod span;
mod token;

pub use interner::{Interner, Symbol};
pub use lexer::Lexer;
pub use span::Span;
pub use token::{Token, TokenKind, keyword_lookup};
