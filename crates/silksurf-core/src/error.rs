//! Workspace-wide canonical error type.
//!
//! WHY: Per-crate error types (CssError, DomError, TokenizeError,
//! TreeBuildError, NetError, TlsConfigError, EngineError, JsError) are
//! useful at their crate boundaries but cause `From` proliferation and
//! `Box<dyn Error>` ergonomics drift at cross-crate boundaries. SilkError
//! is the canonical type that every public API should funnel through at
//! the workspace boundary (silksurf-app, silksurf-engine surface, FFI).
//!
//! WHAT: silksurf-core has no internal dependencies on its dependents
//! (which would create cycles); the per-crate `From` impls live in the
//! leaf crates that own the source error type and that depend on
//! silksurf-core. SilkError variants are string-erased rather than
//! holding the concrete per-crate type for the same reason: silksurf-core
//! cannot name them.
//!
//! HOW: each leaf crate writes
//!   impl From<MyError> for silksurf_core::SilkError {
//!       fn from(e: MyError) -> Self { silksurf_core::SilkError::MyDomain(e.to_string()) }
//!   }
//! and `?` works at the workspace boundary.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum SilkError {
    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("unsupported feature: {0}")]
    Unsupported(String),

    #[error("CSS error at offset {offset}: {message}")]
    Css { offset: usize, message: String },

    #[error("DOM error: {0}")]
    Dom(String),

    #[error("HTML tokenize error at offset {offset}: {message}")]
    HtmlTokenize { offset: usize, message: String },

    #[error("HTML tree-build error: {0}")]
    HtmlTreeBuild(String),

    #[error("network error: {0}")]
    Net(String),

    #[error("TLS error: {0}")]
    Tls(String),

    #[error("engine pipeline error: {0}")]
    Engine(String),

    #[error("JS runtime error: {0}")]
    Js(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub type SilkResult<T> = Result<T, SilkError>;
