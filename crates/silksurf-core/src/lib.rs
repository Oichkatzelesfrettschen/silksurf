mod arena;
mod error;
mod interner;
mod span;

/// Greedy byte-pair-encoding tokenizer over a byte trie (AD-006 scope;
/// re-homed from the retired C tree under AD-024). See [`bpe::BpeTokenizer`].
pub mod bpe;

/// Forensics-grade primitives (deterministic clock + seedable PRNG)
/// for reproducible tests. See [`testing::Clock`] and [`testing::Rng`].
pub mod testing;

/// Public Suffix List matcher deriving the registrable domain (eTLD+1) that
/// scheme-plus-site keys are built from. See [`psl::registrable_domain`].
pub mod psl;

pub use arena::SilkArena;
pub use error::{SilkError, SilkResult};
pub use interner::{Atom, SilkInterner, should_intern_identifier};
pub use span::Span;

pub type SmallString = smol_str::SmolStr;
pub type ArenaVec<'a, T> = bumpalo::collections::Vec<'a, T>;
