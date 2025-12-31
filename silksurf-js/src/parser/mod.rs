//! Recursive descent parser for JavaScript
//!
//! Phase 3 Week 3-4 deliverable.
//! - AST construction with arena allocation
//! - Error recovery (panic mode + synchronization)
//! - Pratt parsing for expressions (precedence climbing)
//!
//! Architecture decisions based on cleanroom study:
//! - Register-based bytecode target (not stack-based) for femto engines
//! - Direct AST→bytecode compilation (drop AST after compile to save memory)
//! - Spec-driven parsing rules from ECMA-262

pub mod ast;
pub mod ast_arena;
pub mod error;
pub mod parser;
pub mod precedence;

pub use ast::*;
pub use ast_arena::{AstArena, AstBox, AstVec, AstVecBuilder};
pub use error::{ParseError, ParseErrorKind, ParseResult};
pub use parser::Parser;
pub use precedence::BindingPower;
