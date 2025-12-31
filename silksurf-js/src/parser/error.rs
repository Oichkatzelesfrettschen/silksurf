//! Parser error types and error recovery
//!
//! Error recovery: panic mode + synchronization at statement boundaries.

use crate::lexer::{Span, TokenKind};
use std::fmt;

/// A parse error
#[derive(Debug, Clone)]
pub struct ParseError {
    pub kind: ParseErrorKind,
    pub span: Span,
    pub context: Option<String>,
}

#[derive(Debug, Clone)]
pub enum ParseErrorKind {
    UnexpectedToken {
        expected: Vec<&'static str>,
        found: String,
    },
    UnexpectedEof {
        expected: &'static str,
    },
    InvalidSyntax {
        message: String,
    },
    InvalidAssignmentTarget,
    InvalidDestructuringPattern,
}

impl ParseError {
    pub fn unexpected_token(span: Span, expected: Vec<&'static str>, found: &str) -> Self {
        Self {
            kind: ParseErrorKind::UnexpectedToken {
                expected,
                found: found.to_string(),
            },
            span,
            context: None,
        }
    }

    pub fn unexpected_eof(span: Span, expected: &'static str) -> Self {
        Self {
            kind: ParseErrorKind::UnexpectedEof { expected },
            span,
            context: None,
        }
    }

    pub fn invalid_syntax(span: Span, message: impl Into<String>) -> Self {
        Self {
            kind: ParseErrorKind::InvalidSyntax {
                message: message.into(),
            },
            span,
            context: None,
        }
    }

    pub fn invalid_assignment_target(span: Span) -> Self {
        Self {
            kind: ParseErrorKind::InvalidAssignmentTarget,
            span,
            context: None,
        }
    }

    pub fn with_context(mut self, context: impl Into<String>) -> Self {
        self.context = Some(context.into());
        self
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            ParseErrorKind::UnexpectedToken { expected, found } => {
                if expected.is_empty() {
                    write!(f, "Unexpected token: {}", found)
                } else if expected.len() == 1 {
                    write!(f, "Expected {}, found {}", expected[0], found)
                } else {
                    write!(f, "Expected one of {:?}, found {}", expected, found)
                }
            }
            ParseErrorKind::UnexpectedEof { expected } => {
                write!(f, "Unexpected end of file, expected {}", expected)
            }
            ParseErrorKind::InvalidSyntax { message } => {
                write!(f, "Invalid syntax: {}", message)
            }
            ParseErrorKind::InvalidAssignmentTarget => {
                write!(f, "Invalid left-hand side in assignment")
            }
            ParseErrorKind::InvalidDestructuringPattern => {
                write!(f, "Invalid destructuring pattern")
            }
        }?;

        write!(f, " at {}:{}", self.span.start, self.span.end)?;

        if let Some(ctx) = &self.context {
            write!(f, " ({})", ctx)?;
        }

        Ok(())
    }
}

impl std::error::Error for ParseError {}

pub type ParseResult<T> = Result<T, ParseError>;

/// Synchronization points for error recovery
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncPoint {
    Statement,
    Declaration,
    CloseBrace,
    CloseParen,
    CloseBracket,
}

/// Check if a token kind is a synchronization point
pub fn is_sync_point(kind: &TokenKind, point: SyncPoint) -> bool {
    match point {
        SyncPoint::Statement => matches!(
            kind,
            TokenKind::Semicolon
                | TokenKind::RightBrace
                | TokenKind::Var
                | TokenKind::Let
                | TokenKind::Const
                | TokenKind::Function
                | TokenKind::Class
                | TokenKind::If
                | TokenKind::For
                | TokenKind::While
                | TokenKind::Do
                | TokenKind::Return
                | TokenKind::Break
                | TokenKind::Continue
                | TokenKind::Throw
                | TokenKind::Try
                | TokenKind::Switch
                | TokenKind::Eof
        ),
        SyncPoint::Declaration => matches!(
            kind,
            TokenKind::Var
                | TokenKind::Let
                | TokenKind::Const
                | TokenKind::Function
                | TokenKind::Class
                | TokenKind::Eof
        ),
        SyncPoint::CloseBrace => matches!(kind, TokenKind::RightBrace | TokenKind::Eof),
        SyncPoint::CloseParen => matches!(kind, TokenKind::RightParen | TokenKind::Eof),
        SyncPoint::CloseBracket => matches!(kind, TokenKind::RightBracket | TokenKind::Eof),
    }
}
