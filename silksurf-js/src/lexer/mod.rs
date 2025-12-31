//! Zero-copy lexer for JavaScript
//!
//! Key design decisions:
//! - Tokens reference source directly (&'src str) - no allocation
//! - BPE pattern matching for common token sequences
//! - String interning for identifiers (O(1) comparison)
//! - Span tracking for error messages
//!
//! Performance target: 50-100 MB/s throughput

mod token;
mod span;
mod lexer;
mod bpe;
mod interner;

pub use token::{Token, TokenKind, keyword_lookup};
pub use span::Span;
pub use lexer::Lexer;
pub use interner::{Interner, Symbol};
