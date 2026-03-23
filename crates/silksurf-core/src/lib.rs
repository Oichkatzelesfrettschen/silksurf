mod arena;
mod error;
mod interner;
mod span;

pub use arena::SilkArena;
pub use error::{SilkError, SilkResult};
pub use interner::{Atom, SilkInterner, should_intern_identifier};
pub use span::Span;

pub type SmallString = smol_str::SmolStr;
pub type ArenaVec<'a, T> = bumpalo::collections::Vec<'a, T>;
