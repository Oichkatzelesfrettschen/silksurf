//! JavaScript token types (zero-copy design)
//!
//! Tokens hold references to source (&'src str) instead of
//! owned strings, eliminating allocation during lexing.

use phf::phf_map;

use super::interner::Symbol;
use super::span::Span;

/// A token with source location
#[derive(Debug, Clone, Copy)]
pub struct Token<'src> {
    /// Token type
    pub kind: TokenKind<'src>,
    /// Location in source
    pub span: Span,
}

impl<'src> Token<'src> {
    /// Create a new token
    #[inline]
    #[must_use]
    pub const fn new(kind: TokenKind<'src>, span: Span) -> Self {
        Self { kind, span }
    }

    /// Get the lexeme (source text) for this token
    #[inline]
    #[must_use]
    pub fn lexeme(self, source: &'src str) -> &'src str {
        self.span.text(source)
    }
}

/// Token kinds for JavaScript
///
/// Zero-copy: string values are slices into source,
/// identifiers use interned symbols.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TokenKind<'src> {
    // Literals
    /// Integer literal (stored as slice, parsed later)
    Integer(&'src str),
    /// Float literal
    Float(&'src str),
    /// String literal (includes quotes)
    String(&'src str),
    /// Template literal part
    Template(&'src str),
    /// Regular expression literal
    RegExp(&'src str),
    /// Boolean true
    True,
    /// Boolean false
    False,
    /// null
    Null,
    /// undefined (not technically a literal, but reserved)
    Undefined,

    // Identifiers and keywords
    /// Identifier (interned)
    Identifier(Symbol),
    /// Private identifier (#name)
    PrivateIdentifier(Symbol),

    // Keywords
    Await,
    Break,
    Case,
    Catch,
    Class,
    Const,
    Continue,
    Debugger,
    Default,
    Delete,
    Do,
    Else,
    Enum,
    Export,
    Extends,
    Finally,
    For,
    Function,
    If,
    Import,
    In,
    Instanceof,
    Let,
    New,
    Return,
    Super,
    Switch,
    This,
    Throw,
    Try,
    Typeof,
    Var,
    Void,
    While,
    With,
    Yield,
    // Strict mode keywords
    Implements,
    Interface,
    Package,
    Private,
    Protected,
    Public,
    Static,
    // Async/generator
    Async,
    Of,
    Get,
    Set,
    As,
    From,
    Target,
    Meta,

    // Punctuators
    /// {
    LeftBrace,
    /// }
    RightBrace,
    /// (
    LeftParen,
    /// )
    RightParen,
    /// [
    LeftBracket,
    /// ]
    RightBracket,
    /// .
    Dot,
    /// ...
    Ellipsis,
    /// ;
    Semicolon,
    /// ,
    Comma,
    /// <
    LessThan,
    /// >
    GreaterThan,
    /// <=
    LessEqual,
    /// >=
    GreaterEqual,
    /// ==
    Equal,
    /// !=
    NotEqual,
    /// ===
    StrictEqual,
    /// !==
    StrictNotEqual,
    /// +
    Plus,
    /// -
    Minus,
    /// *
    Star,
    /// /
    Slash,
    /// %
    Percent,
    /// **
    StarStar,
    /// ++
    PlusPlus,
    /// --
    MinusMinus,
    /// <<
    LeftShift,
    /// >>
    RightShift,
    /// >>>
    UnsignedRightShift,
    /// &
    Ampersand,
    /// |
    Pipe,
    /// ^
    Caret,
    /// !
    Bang,
    /// ~
    Tilde,
    /// &&
    AmpersandAmpersand,
    /// ||
    PipePipe,
    /// ??
    QuestionQuestion,
    /// ?
    Question,
    /// ?.
    QuestionDot,
    /// :
    Colon,
    /// =
    Assign,
    /// +=
    PlusAssign,
    /// -=
    MinusAssign,
    /// *=
    StarAssign,
    /// /=
    SlashAssign,
    /// %=
    PercentAssign,
    /// **=
    StarStarAssign,
    /// <<=
    LeftShiftAssign,
    /// >>=
    RightShiftAssign,
    /// >>>=
    UnsignedRightShiftAssign,
    /// &=
    AmpersandAssign,
    /// |=
    PipeAssign,
    /// ^=
    CaretAssign,
    /// &&=
    AmpersandAmpersandAssign,
    /// ||=
    PipePipeAssign,
    /// ??=
    QuestionQuestionAssign,
    /// =>
    Arrow,
    /// @
    At,

    // Special
    /// End of file
    Eof,
    /// Lexer error
    Error(&'src str),
    /// Line terminator (for ASI)
    LineTerminator,
    /// Comment (single-line or multi-line)
    Comment(&'src str),
}

impl TokenKind<'_> {
    /// Check if this is a keyword
    #[must_use]
    pub const fn is_keyword(&self) -> bool {
        matches!(
            self,
            Self::Await
                | Self::Break
                | Self::Case
                | Self::Catch
                | Self::Class
                | Self::Const
                | Self::Continue
                | Self::Debugger
                | Self::Default
                | Self::Delete
                | Self::Do
                | Self::Else
                | Self::Enum
                | Self::Export
                | Self::Extends
                | Self::Finally
                | Self::For
                | Self::Function
                | Self::If
                | Self::Import
                | Self::In
                | Self::Instanceof
                | Self::Let
                | Self::New
                | Self::Return
                | Self::Super
                | Self::Switch
                | Self::This
                | Self::Throw
                | Self::Try
                | Self::Typeof
                | Self::Var
                | Self::Void
                | Self::While
                | Self::With
                | Self::Yield
        )
    }

    /// Check if this is a literal
    #[must_use]
    pub const fn is_literal(&self) -> bool {
        matches!(
            self,
            Self::Integer(_)
                | Self::Float(_)
                | Self::String(_)
                | Self::Template(_)
                | Self::RegExp(_)
                | Self::True
                | Self::False
                | Self::Null
        )
    }

    /// Check if this token can start an expression
    #[must_use]
    pub const fn can_start_expression(&self) -> bool {
        matches!(
            self,
            Self::Identifier(_)
                | Self::Integer(_)
                | Self::Float(_)
                | Self::String(_)
                | Self::Template(_)
                | Self::True
                | Self::False
                | Self::Null
                | Self::This
                | Self::Function
                | Self::Class
                | Self::New
                | Self::LeftParen
                | Self::LeftBracket
                | Self::LeftBrace
                | Self::Plus
                | Self::Minus
                | Self::Bang
                | Self::Tilde
                | Self::Typeof
                | Self::Void
                | Self::Delete
                | Self::Await
                | Self::Yield
                | Self::PlusPlus
                | Self::MinusMinus
        )
    }
}

/// Keyword ID for perfect hash lookup
/// Maps to `TokenKind` variants without lifetime
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum KeywordId {
    Await,
    Break,
    Case,
    Catch,
    Class,
    Const,
    Continue,
    Debugger,
    Default,
    Delete,
    Do,
    Else,
    Enum,
    Export,
    Extends,
    False,
    Finally,
    For,
    Function,
    If,
    Import,
    In,
    Instanceof,
    Let,
    New,
    Null,
    Return,
    Super,
    Switch,
    This,
    Throw,
    True,
    Try,
    Typeof,
    Undefined,
    Var,
    Void,
    While,
    With,
    Yield,
    Implements,
    Interface,
    Package,
    Private,
    Protected,
    Public,
    Static,
    Async,
    Of,
    Get,
    Set,
    As,
    From,
    Target,
    Meta,
}

impl KeywordId {
    /// Convert to `TokenKind`
    #[inline]
    const fn to_token_kind(self) -> TokenKind<'static> {
        match self {
            Self::Await => TokenKind::Await,
            Self::Break => TokenKind::Break,
            Self::Case => TokenKind::Case,
            Self::Catch => TokenKind::Catch,
            Self::Class => TokenKind::Class,
            Self::Const => TokenKind::Const,
            Self::Continue => TokenKind::Continue,
            Self::Debugger => TokenKind::Debugger,
            Self::Default => TokenKind::Default,
            Self::Delete => TokenKind::Delete,
            Self::Do => TokenKind::Do,
            Self::Else => TokenKind::Else,
            Self::Enum => TokenKind::Enum,
            Self::Export => TokenKind::Export,
            Self::Extends => TokenKind::Extends,
            Self::False => TokenKind::False,
            Self::Finally => TokenKind::Finally,
            Self::For => TokenKind::For,
            Self::Function => TokenKind::Function,
            Self::If => TokenKind::If,
            Self::Import => TokenKind::Import,
            Self::In => TokenKind::In,
            Self::Instanceof => TokenKind::Instanceof,
            Self::Let => TokenKind::Let,
            Self::New => TokenKind::New,
            Self::Null => TokenKind::Null,
            Self::Return => TokenKind::Return,
            Self::Super => TokenKind::Super,
            Self::Switch => TokenKind::Switch,
            Self::This => TokenKind::This,
            Self::Throw => TokenKind::Throw,
            Self::True => TokenKind::True,
            Self::Try => TokenKind::Try,
            Self::Typeof => TokenKind::Typeof,
            Self::Undefined => TokenKind::Undefined,
            Self::Var => TokenKind::Var,
            Self::Void => TokenKind::Void,
            Self::While => TokenKind::While,
            Self::With => TokenKind::With,
            Self::Yield => TokenKind::Yield,
            Self::Implements => TokenKind::Implements,
            Self::Interface => TokenKind::Interface,
            Self::Package => TokenKind::Package,
            Self::Private => TokenKind::Private,
            Self::Protected => TokenKind::Protected,
            Self::Public => TokenKind::Public,
            Self::Static => TokenKind::Static,
            Self::Async => TokenKind::Async,
            Self::Of => TokenKind::Of,
            Self::Get => TokenKind::Get,
            Self::Set => TokenKind::Set,
            Self::As => TokenKind::As,
            Self::From => TokenKind::From,
            Self::Target => TokenKind::Target,
            Self::Meta => TokenKind::Meta,
        }
    }
}

/// Perfect hash map for O(1) keyword lookup
/// Generated at compile time with zero runtime cost
static KEYWORDS: phf::Map<&'static str, KeywordId> = phf_map! {
    "await" => KeywordId::Await,
    "break" => KeywordId::Break,
    "case" => KeywordId::Case,
    "catch" => KeywordId::Catch,
    "class" => KeywordId::Class,
    "const" => KeywordId::Const,
    "continue" => KeywordId::Continue,
    "debugger" => KeywordId::Debugger,
    "default" => KeywordId::Default,
    "delete" => KeywordId::Delete,
    "do" => KeywordId::Do,
    "else" => KeywordId::Else,
    "enum" => KeywordId::Enum,
    "export" => KeywordId::Export,
    "extends" => KeywordId::Extends,
    "false" => KeywordId::False,
    "finally" => KeywordId::Finally,
    "for" => KeywordId::For,
    "function" => KeywordId::Function,
    "if" => KeywordId::If,
    "import" => KeywordId::Import,
    "in" => KeywordId::In,
    "instanceof" => KeywordId::Instanceof,
    "let" => KeywordId::Let,
    "new" => KeywordId::New,
    "null" => KeywordId::Null,
    "return" => KeywordId::Return,
    "super" => KeywordId::Super,
    "switch" => KeywordId::Switch,
    "this" => KeywordId::This,
    "throw" => KeywordId::Throw,
    "true" => KeywordId::True,
    "try" => KeywordId::Try,
    "typeof" => KeywordId::Typeof,
    "undefined" => KeywordId::Undefined,
    "var" => KeywordId::Var,
    "void" => KeywordId::Void,
    "while" => KeywordId::While,
    "with" => KeywordId::With,
    "yield" => KeywordId::Yield,
    "implements" => KeywordId::Implements,
    "interface" => KeywordId::Interface,
    "package" => KeywordId::Package,
    "private" => KeywordId::Private,
    "protected" => KeywordId::Protected,
    "public" => KeywordId::Public,
    "static" => KeywordId::Static,
    "async" => KeywordId::Async,
    "of" => KeywordId::Of,
    "get" => KeywordId::Get,
    "set" => KeywordId::Set,
    "as" => KeywordId::As,
    "from" => KeywordId::From,
    "target" => KeywordId::Target,
    "meta" => KeywordId::Meta,
};

/// Lookup table for keyword matching (O(1) via perfect hash)
/// Returns Some(TokenKind) if the string is a keyword, None otherwise.
#[inline]
#[must_use]
pub fn keyword_lookup(s: &str) -> Option<TokenKind<'static>> {
    KEYWORDS.get(s).map(|id| id.to_token_kind())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keyword_lookup() {
        assert!(matches!(
            keyword_lookup("function"),
            Some(TokenKind::Function)
        ));
        assert!(matches!(keyword_lookup("const"), Some(TokenKind::Const)));
        assert!(keyword_lookup("notakeyword").is_none());
    }

    #[test]
    fn test_token_kind_checks() {
        assert!(TokenKind::Function.is_keyword());
        assert!(TokenKind::Integer("42").is_literal());
        assert!(TokenKind::LeftParen.can_start_expression());
    }
}
