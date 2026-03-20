//! Recursive descent parser with Pratt precedence climbing
//!
//! Architecture:
//! - Zero-copy: AST references source via Span
//! - Single pass: No backtracking required
//! - Error recovery: Panic mode with synchronization
//!
//! Expression parsing uses Pratt parsing (precedence climbing).
//! Statement parsing uses standard recursive descent.

use crate::lexer::{Interner, Lexer, Span, Token, TokenKind};
use crate::parser::ast::{
    Argument, ArrayElement, ArrayExpression, ArrayPattern, ArrowBody, ArrowFunctionExpression,
    AssignmentExpression, AssignmentPattern, AssignmentTarget, AwaitExpression, BinaryExpression,
    BlockStatement, BooleanLiteral, BreakStatement, CallExpression, CatchClause, ClassBody,
    ClassDeclaration, ClassElement, ClassExpression, ConditionalExpression, ContinueStatement,
    DoWhileStatement, Expression, ExpressionStatement, ForInLeft, ForInStatement, ForInit,
    ForOfStatement, ForStatement, FunctionDeclaration, FunctionExpression, Identifier, IfStatement,
    Literal, LogicalExpression, MemberExpression, MethodDefinition, MethodKind, NewExpression,
    NumberLiteral, ObjectExpression, ObjectPattern, ObjectPatternProperty, ObjectProperty,
    ParenthesizedExpression, Pattern, Program, Property, PropertyDefinition, PropertyKey,
    PropertyKind, RegExpLiteral, RestElement, ReturnStatement, SequenceExpression, SourceType,
    SpreadElement, Statement, StringLiteral, SwitchCase, SwitchStatement, TaggedTemplateExpression,
    TemplateElement, TemplateLiteral, ThrowStatement, TryStatement, UnaryExpression,
    UpdateExpression, VariableDeclaration, VariableDeclarator, VariableKind, WhileStatement,
    YieldExpression,
};
use crate::parser::ast_arena::{AstArena, AstVec, AstVecBuilder};
use crate::parser::error::{is_sync_point, ParseError, ParseResult, SyncPoint};
use crate::parser::precedence::{
    infix_binding_power, postfix_binding_power, prefix_binding_power, token_to_assignment_op,
    token_to_binary_op, token_to_logical_op, token_to_unary_op, token_to_update_op, BindingPower,
};

/// The JavaScript parser
pub struct Parser<'src, 'arena> {
    /// Source code
    source: &'src str,
    /// Lexer producing tokens (owns the interner)
    lexer: Lexer<'src>,
    /// Current token
    current: Token<'src>,
    /// Previous token (for spans)
    previous: Token<'src>,
    /// Collected errors (for error recovery)
    errors: Vec<ParseError>,
    /// Are we in panic mode?
    panic_mode: bool,
    /// Arena for AST allocation
    arena: &'arena AstArena,
}

impl<'src, 'arena> Parser<'src, 'arena> {
    /// Create a new parser
    pub fn new(source: &'src str, arena: &'arena AstArena) -> Self {
        let mut lexer = Lexer::new(source);
        let first_token = lexer.next_token();

        // Dummy previous token at start
        let dummy = Token {
            kind: TokenKind::Eof,
            span: Span::new(0, 0),
        };

        Self {
            source,
            lexer,
            current: first_token,
            previous: dummy,
            errors: Vec::new(),
            panic_mode: false,
            arena,
        }
    }

    /// Parse the source into a Program AST
    #[must_use]
    pub fn parse(mut self) -> (Program<'src, 'arena>, Vec<ParseError>) {
        // Pre-allocate: estimate ~1 statement per 80 bytes of source
        let estimated_stmts = (self.source.len() / 80).max(8);
        let mut body = AstVecBuilder::with_capacity(estimated_stmts);
        let start = self.current.span.start;

        while !self.is_at_end() {
            match self.parse_statement() {
                Ok(stmt) => body.push(stmt),
                Err(e) => {
                    self.errors.push(e);
                    self.synchronize(SyncPoint::Statement);
                }
            }
        }

        let end = self.previous.span.end;
        let program = Program {
            body: body.freeze(self.arena),
            source_type: SourceType::Script, // TODO: detect module
            span: Span::new(start, end),
        };

        (program, self.errors)
    }

    /// Get the string interner
    #[must_use]
    pub fn interner(&self) -> &Interner {
        self.lexer.interner()
    }

    /// Take the interner (transfers ownership, consumes lexer)
    #[must_use]
    pub fn into_interner(self) -> Interner {
        self.lexer.into_interner()
    }

    // ========================================================================
    // Token handling
    // ========================================================================

    /// Check if at end of file
    #[inline(always)]
    fn is_at_end(&self) -> bool {
        matches!(self.current.kind, TokenKind::Eof)
    }

    /// Advance to next token
    #[inline(always)]
    fn advance(&mut self) -> Token<'src> {
        self.previous = self.current;
        self.current = self.lexer.next_token();
        self.previous
    }

    /// Check current token kind
    #[inline(always)]
    fn check(&self, kind: &TokenKind) -> bool {
        std::mem::discriminant(&self.current.kind) == std::mem::discriminant(kind)
    }

    /// Check if current is one of the given kinds
    #[inline]
    fn check_any(&self, kinds: &[TokenKind]) -> bool {
        kinds.iter().any(|k| self.check(k))
    }

    /// Consume token if it matches, return true
    #[inline(always)]
    fn match_token(&mut self, kind: &TokenKind) -> bool {
        if self.check(kind) {
            self.advance();
            true
        } else {
            false
        }
    }

    /// Consume token if it matches any, return true
    fn match_any(&mut self, kinds: &[TokenKind]) -> bool {
        for kind in kinds {
            if self.check(kind) {
                self.advance();
                return true;
            }
        }
        false
    }

    /// Expect a specific token, error if not found
    #[inline]
    fn expect(&mut self, kind: &TokenKind, msg: &'static str) -> ParseResult<Token<'src>> {
        if self.check(kind) {
            Ok(self.advance())
        } else {
            Err(ParseError::unexpected_token(
                self.current.span,
                vec![msg],
                &format!("{:?}", self.current.kind),
            ))
        }
    }

    /// Get text for a span
    #[inline(always)]
    fn text(&self, span: Span) -> &'src str {
        &self.source[span.start as usize..span.end as usize]
    }

    /// Check if current token is an identifier
    #[inline(always)]
    fn is_identifier(&self) -> bool {
        matches!(self.current.kind, TokenKind::Identifier(_))
    }

    // ========================================================================
    // Error recovery
    // ========================================================================

    /// Enter panic mode and synchronize at the given point
    fn synchronize(&mut self, point: SyncPoint) {
        self.panic_mode = true;

        while !self.is_at_end() {
            // Found a synchronization point
            if is_sync_point(&self.current.kind, point) {
                if matches!(self.current.kind, TokenKind::Semicolon) {
                    self.advance(); // Consume the semicolon
                }
                self.panic_mode = false;
                return;
            }

            self.advance();
        }

        self.panic_mode = false;
    }

    // ========================================================================
    // Statement parsing
    // ========================================================================

    /// Parse a statement
    fn parse_statement(&mut self) -> ParseResult<Statement<'src, 'arena>> {
        match &self.current.kind {
            TokenKind::Var | TokenKind::Let | TokenKind::Const => self.parse_variable_declaration(),
            TokenKind::Function => self.parse_function_declaration(),
            TokenKind::Class => self.parse_class_declaration(),
            TokenKind::If => self.parse_if_statement(),
            TokenKind::While => self.parse_while_statement(),
            TokenKind::Do => self.parse_do_while_statement(),
            TokenKind::For => self.parse_for_statement(),
            TokenKind::Return => self.parse_return_statement(),
            TokenKind::Break => self.parse_break_statement(),
            TokenKind::Continue => self.parse_continue_statement(),
            TokenKind::Throw => self.parse_throw_statement(),
            TokenKind::Try => self.parse_try_statement(),
            TokenKind::Switch => self.parse_switch_statement(),
            TokenKind::LeftBrace => self.parse_block_statement(),
            TokenKind::Semicolon => {
                let span = self.advance().span;
                Ok(Statement::Empty(span))
            }
            TokenKind::Debugger => {
                let span = self.advance().span;
                self.expect(&TokenKind::Semicolon, ";")?;
                Ok(Statement::Debugger(span))
            }
            _ => self.parse_expression_statement(),
        }
    }

    /// Parse variable declaration: var/let/const x = y;
    fn parse_variable_declaration(&mut self) -> ParseResult<Statement<'src, 'arena>> {
        let start = self.current.span.start;
        let kind = match &self.current.kind {
            TokenKind::Var => VariableKind::Var,
            TokenKind::Let => VariableKind::Let,
            TokenKind::Const => VariableKind::Const,
            _ => unreachable!(),
        };
        self.advance();

        // Most declarations have 1-3 declarators
        let mut declarations = AstVecBuilder::with_capacity(2);
        loop {
            let decl = self.parse_variable_declarator()?;
            declarations.push(decl);

            if !self.match_token(&TokenKind::Comma) {
                break;
            }
        }

        let end = self.previous.span.end;
        // Semicolon is optional in some contexts (for-loop init)
        self.match_token(&TokenKind::Semicolon);

        Ok(Statement::VariableDeclaration(VariableDeclaration {
            kind,
            declarations: declarations.freeze(self.arena),
            span: Span::new(start, end),
        }))
    }

    /// Parse a single variable declarator
    fn parse_variable_declarator(&mut self) -> ParseResult<VariableDeclarator<'src, 'arena>> {
        let start = self.current.span.start;
        let id = self.parse_binding_pattern()?;

        let init = if self.match_token(&TokenKind::Assign) {
            Some(self.arena.alloc(self.parse_assignment_expression()?))
        } else {
            None
        };

        let end = self.previous.span.end;
        Ok(VariableDeclarator {
            id,
            init,
            span: Span::new(start, end),
        })
    }

    /// Parse binding pattern (identifier or destructuring)
    fn parse_binding_pattern(&mut self) -> ParseResult<Pattern<'src, 'arena>> {
        match &self.current.kind {
            TokenKind::Identifier(_) => {
                let ident = self.parse_identifier()?;
                Ok(Pattern::Identifier(ident))
            }
            TokenKind::LeftBracket => self.parse_array_pattern(),
            TokenKind::LeftBrace => self.parse_object_pattern(),
            _ => Err(ParseError::unexpected_token(
                self.current.span,
                vec!["identifier", "[", "{"],
                &format!("{:?}", self.current.kind),
            )),
        }
    }

    /// Parse array pattern: [a, b, ...rest]
    fn parse_array_pattern(&mut self) -> ParseResult<Pattern<'src, 'arena>> {
        let start = self.current.span.start;
        self.expect(&TokenKind::LeftBracket, "[")?;

        let mut elements = AstVecBuilder::new();
        while !self.check(&TokenKind::RightBracket) && !self.is_at_end() {
            if self.check(&TokenKind::Comma) {
                // Elision (hole)
                elements.push(None);
            } else if self.check(&TokenKind::Ellipsis) {
                self.advance();
                let arg = self.parse_binding_pattern()?;
                elements.push(Some(Pattern::Rest(RestElement {
                    argument: self.arena.alloc(arg),
                    span: Span::new(start, self.previous.span.end),
                })));
            } else {
                let element = self.parse_binding_pattern()?;
                // Check for default value
                let element = if self.match_token(&TokenKind::Assign) {
                    let right = self.parse_assignment_expression()?;
                    Pattern::Assignment(AssignmentPattern {
                        left: self.arena.alloc(element.clone()),
                        right: self.arena.alloc(right),
                        span: Span::new(element.span().start, self.previous.span.end),
                    })
                } else {
                    element
                };
                elements.push(Some(element));
            }

            if !self.check(&TokenKind::RightBracket) {
                self.expect(&TokenKind::Comma, ",")?;
            }
        }

        self.expect(&TokenKind::RightBracket, "]")?;
        let end = self.previous.span.end;

        Ok(Pattern::Array(ArrayPattern {
            elements: elements.freeze(self.arena),
            span: Span::new(start, end),
        }))
    }

    /// Parse object pattern: { a, b: c, ...rest }
    fn parse_object_pattern(&mut self) -> ParseResult<Pattern<'src, 'arena>> {
        let start = self.current.span.start;
        self.expect(&TokenKind::LeftBrace, "{")?;

        let mut properties = AstVecBuilder::new();
        while !self.check(&TokenKind::RightBrace) && !self.is_at_end() {
            if self.check(&TokenKind::Ellipsis) {
                self.advance();
                let arg = self.parse_binding_pattern()?;
                properties.push(ObjectPatternProperty::Rest(RestElement {
                    argument: self.arena.alloc(arg),
                    span: Span::new(start, self.previous.span.end),
                }));
            } else {
                let prop = self.parse_object_pattern_property()?;
                properties.push(prop);
            }

            if !self.check(&TokenKind::RightBrace) {
                self.expect(&TokenKind::Comma, ",")?;
            }
        }

        self.expect(&TokenKind::RightBrace, "}")?;
        let end = self.previous.span.end;

        Ok(Pattern::Object(ObjectPattern {
            properties: properties.freeze(self.arena),
            span: Span::new(start, end),
        }))
    }

    /// Parse object pattern property
    fn parse_object_pattern_property(
        &mut self,
    ) -> ParseResult<ObjectPatternProperty<'src, 'arena>> {
        let start = self.current.span.start;
        let computed = self.check(&TokenKind::LeftBracket);

        let key = self.parse_property_key()?;

        // Shorthand: { a } is equivalent to { a: a }
        let (value, shorthand) = if self.match_token(&TokenKind::Colon) {
            let pattern = self.parse_binding_pattern()?;
            (pattern, false)
        } else {
            // Shorthand - key must be identifier
            match &key {
                PropertyKey::Identifier(ident) => (Pattern::Identifier(ident.clone()), true),
                _ => {
                    return Err(ParseError::invalid_syntax(
                        self.current.span,
                        "Shorthand property must be an identifier",
                    ))
                }
            }
        };

        // Default value
        let value = if self.match_token(&TokenKind::Assign) {
            let right = self.parse_assignment_expression()?;
            Pattern::Assignment(AssignmentPattern {
                left: self.arena.alloc(value.clone()),
                right: self.arena.alloc(right),
                span: Span::new(value.span().start, self.previous.span.end),
            })
        } else {
            value
        };

        let end = self.previous.span.end;
        Ok(ObjectPatternProperty::Property {
            key,
            value,
            shorthand,
            computed,
            span: Span::new(start, end),
        })
    }

    /// Parse function declaration
    fn parse_function_declaration(&mut self) -> ParseResult<Statement<'src, 'arena>> {
        let start = self.current.span.start;
        let is_async = self.previous.kind == TokenKind::Async;
        self.expect(&TokenKind::Function, "function")?;

        let is_generator = self.match_token(&TokenKind::Star);

        // Function name (required for declarations)
        let id = if self.is_identifier() {
            Some(self.parse_identifier()?)
        } else {
            None
        };

        self.expect(&TokenKind::LeftParen, "(")?;
        let params = self.parse_function_params()?;
        self.expect(&TokenKind::RightParen, ")")?;

        let body = self.parse_block_statement_inner()?;
        let end = self.previous.span.end;

        Ok(Statement::FunctionDeclaration(FunctionDeclaration {
            id,
            params,
            body,
            is_async,
            is_generator,
            span: Span::new(start, end),
        }))
    }

    /// Parse function parameters
    #[inline]
    fn parse_function_params(&mut self) -> ParseResult<AstVec<'arena, Pattern<'src, 'arena>>> {
        // Typical function has 0-4 parameters
        let mut params = AstVecBuilder::with_capacity(3);

        while !self.check(&TokenKind::RightParen) && !self.is_at_end() {
            if self.check(&TokenKind::Ellipsis) {
                self.advance();
                let arg = self.parse_binding_pattern()?;
                params.push(Pattern::Rest(RestElement {
                    argument: self.arena.alloc(arg),
                    span: self.previous.span,
                }));
                break; // Rest must be last
            }

            let param = self.parse_binding_pattern()?;
            // Default value
            let param = if self.match_token(&TokenKind::Assign) {
                let right = self.parse_assignment_expression()?;
                Pattern::Assignment(AssignmentPattern {
                    left: self.arena.alloc(param.clone()),
                    right: self.arena.alloc(right),
                    span: Span::new(param.span().start, self.previous.span.end),
                })
            } else {
                param
            };

            params.push(param);

            if !self.check(&TokenKind::RightParen) {
                self.expect(&TokenKind::Comma, ",")?;
            }
        }

        Ok(params.freeze(self.arena))
    }

    /// Parse class declaration
    fn parse_class_declaration(&mut self) -> ParseResult<Statement<'src, 'arena>> {
        let start = self.current.span.start;
        self.expect(&TokenKind::Class, "class")?;

        let id = if self.is_identifier() {
            Some(self.parse_identifier()?)
        } else {
            None
        };

        let super_class = if self.match_token(&TokenKind::Extends) {
            Some(self.arena.alloc(self.parse_left_hand_side_expression()?))
        } else {
            None
        };

        let body = self.parse_class_body()?;
        let end = self.previous.span.end;

        Ok(Statement::ClassDeclaration(ClassDeclaration {
            id,
            super_class,
            body,
            span: Span::new(start, end),
        }))
    }

    /// Parse class body
    #[inline]
    fn parse_class_body(&mut self) -> ParseResult<ClassBody<'src, 'arena>> {
        let start = self.current.span.start;
        self.expect(&TokenKind::LeftBrace, "{")?;

        // Typical class has 2-5 methods
        let mut body = AstVecBuilder::with_capacity(4);
        while !self.check(&TokenKind::RightBrace) && !self.is_at_end() {
            // Skip semicolons in class body
            if self.match_token(&TokenKind::Semicolon) {
                continue;
            }

            let element = self.parse_class_element()?;
            body.push(element);
        }

        self.expect(&TokenKind::RightBrace, "}")?;
        let end = self.previous.span.end;

        Ok(ClassBody {
            body: body.freeze(self.arena),
            span: Span::new(start, end),
        })
    }

    /// Parse class element (method or property)
    fn parse_class_element(&mut self) -> ParseResult<ClassElement<'src, 'arena>> {
        let start = self.current.span.start;
        let is_static = self.match_token(&TokenKind::Static);

        // Static block
        if is_static && self.check(&TokenKind::LeftBrace) {
            let block = self.parse_block_statement_inner()?;
            return Ok(ClassElement::StaticBlock(block));
        }

        // Method kind (get/set/constructor)
        let mut kind = MethodKind::Method;
        if self.is_identifier() {
            let name = self.text(self.current.span);
            if name == "constructor" && !is_static {
                kind = MethodKind::Constructor;
            }
        }

        let is_generator = self.match_token(&TokenKind::Star);
        let computed = self.check(&TokenKind::LeftBracket);
        let key = self.parse_property_key()?;

        // Is it a method or property?
        if self.check(&TokenKind::LeftParen) {
            // Method
            self.expect(&TokenKind::LeftParen, "(")?;
            let params = self.parse_function_params()?;
            self.expect(&TokenKind::RightParen, ")")?;
            let body = self.parse_block_statement_inner()?;
            let end = self.previous.span.end;

            Ok(ClassElement::MethodDefinition(MethodDefinition {
                key,
                value: FunctionExpression {
                    id: None,
                    params,
                    body,
                    is_async: false, // TODO: handle async
                    is_generator,
                    span: Span::new(start, end),
                },
                kind,
                computed,
                is_static,
                span: Span::new(start, end),
            }))
        } else {
            // Property
            let value = if self.match_token(&TokenKind::Assign) {
                Some(self.arena.alloc(self.parse_assignment_expression()?))
            } else {
                None
            };

            self.match_token(&TokenKind::Semicolon);
            let end = self.previous.span.end;

            Ok(ClassElement::PropertyDefinition(PropertyDefinition {
                key,
                value,
                computed,
                is_static,
                span: Span::new(start, end),
            }))
        }
    }

    /// Parse if statement
    fn parse_if_statement(&mut self) -> ParseResult<Statement<'src, 'arena>> {
        let start = self.current.span.start;
        self.expect(&TokenKind::If, "if")?;
        self.expect(&TokenKind::LeftParen, "(")?;
        let test = self.arena.alloc(self.parse_expression()?);
        self.expect(&TokenKind::RightParen, ")")?;

        let consequent = self.arena.alloc(self.parse_statement()?);

        let alternate = if self.match_token(&TokenKind::Else) {
            Some(self.arena.alloc(self.parse_statement()?))
        } else {
            None
        };

        let end = self.previous.span.end;
        Ok(Statement::If(IfStatement {
            test,
            consequent,
            alternate,
            span: Span::new(start, end),
        }))
    }

    /// Parse while statement
    fn parse_while_statement(&mut self) -> ParseResult<Statement<'src, 'arena>> {
        let start = self.current.span.start;
        self.expect(&TokenKind::While, "while")?;
        self.expect(&TokenKind::LeftParen, "(")?;
        let test = self.arena.alloc(self.parse_expression()?);
        self.expect(&TokenKind::RightParen, ")")?;

        let body = self.arena.alloc(self.parse_statement()?);
        let end = self.previous.span.end;

        Ok(Statement::While(WhileStatement {
            test,
            body,
            span: Span::new(start, end),
        }))
    }

    /// Parse do-while statement
    fn parse_do_while_statement(&mut self) -> ParseResult<Statement<'src, 'arena>> {
        let start = self.current.span.start;
        self.expect(&TokenKind::Do, "do")?;

        let body = self.arena.alloc(self.parse_statement()?);

        self.expect(&TokenKind::While, "while")?;
        self.expect(&TokenKind::LeftParen, "(")?;
        let test = self.arena.alloc(self.parse_expression()?);
        self.expect(&TokenKind::RightParen, ")")?;
        self.match_token(&TokenKind::Semicolon);

        let end = self.previous.span.end;
        Ok(Statement::DoWhile(DoWhileStatement {
            body,
            test,
            span: Span::new(start, end),
        }))
    }

    /// Parse for statement (for, for-in, for-of)
    fn parse_for_statement(&mut self) -> ParseResult<Statement<'src, 'arena>> {
        let start = self.current.span.start;
        self.expect(&TokenKind::For, "for")?;
        let is_await = self.match_token(&TokenKind::Await);
        self.expect(&TokenKind::LeftParen, "(")?;

        // Parse init part
        let init = if self.check(&TokenKind::Semicolon) {
            None
        } else if self.check(&TokenKind::Var)
            || self.check(&TokenKind::Let)
            || self.check(&TokenKind::Const)
        {
            Some(self.parse_for_init_var()?)
        } else {
            Some(ForInit::Expression(self.arena.alloc(self.parse_expression()?)))
        };

        // Check for for-in or for-of
        if self.check(&TokenKind::In) || self.check(&TokenKind::Of) {
            return self.parse_for_in_of(start, init, is_await);
        }

        // Regular for loop
        self.expect(&TokenKind::Semicolon, ";")?;

        let test = if self.check(&TokenKind::Semicolon) {
            None
        } else {
            Some(self.arena.alloc(self.parse_expression()?))
        };
        self.expect(&TokenKind::Semicolon, ";")?;

        let update = if self.check(&TokenKind::RightParen) {
            None
        } else {
            Some(self.arena.alloc(self.parse_expression()?))
        };
        self.expect(&TokenKind::RightParen, ")")?;

        let body = self.arena.alloc(self.parse_statement()?);
        let end = self.previous.span.end;

        Ok(Statement::For(ForStatement {
            init,
            test,
            update,
            body,
            span: Span::new(start, end),
        }))
    }

    /// Parse for-loop init (var/let/const)
    fn parse_for_init_var(&mut self) -> ParseResult<ForInit<'src, 'arena>> {
        let kind = match &self.current.kind {
            TokenKind::Var => VariableKind::Var,
            TokenKind::Let => VariableKind::Let,
            TokenKind::Const => VariableKind::Const,
            _ => unreachable!(),
        };
        let start = self.advance().span.start;

        let mut declarations = AstVecBuilder::new();
        loop {
            let decl = self.parse_variable_declarator()?;
            declarations.push(decl);

            if !self.match_token(&TokenKind::Comma) {
                break;
            }
        }

        let end = self.previous.span.end;
        Ok(ForInit::VariableDeclaration(VariableDeclaration {
            kind,
            declarations: declarations.freeze(self.arena),
            span: Span::new(start, end),
        }))
    }

    /// Parse for-in or for-of statement
    fn parse_for_in_of(
        &mut self,
        start: u32,
        init: Option<ForInit<'src, 'arena>>,
        is_await: bool,
    ) -> ParseResult<Statement<'src, 'arena>> {
        let is_of = self.match_token(&TokenKind::Of);
        if !is_of {
            self.expect(&TokenKind::In, "in")?;
        }

        let left = match init {
            Some(ForInit::VariableDeclaration(decl)) => ForInLeft::VariableDeclaration(decl),
            Some(ForInit::Expression(expr)) => {
                // Convert expression to pattern
                ForInLeft::Pattern(Self::expression_to_pattern(expr)?)
            }
            None => {
                return Err(ParseError::invalid_syntax(
                    self.current.span,
                    "Expected left-hand side in for-in/of",
                ))
            }
        };

        let right = self.arena.alloc(self.parse_assignment_expression()?);
        self.expect(&TokenKind::RightParen, ")")?;

        let body = self.arena.alloc(self.parse_statement()?);
        let end = self.previous.span.end;

        if is_of {
            Ok(Statement::ForOf(ForOfStatement {
                left,
                right,
                body,
                is_await,
                span: Span::new(start, end),
            }))
        } else {
            Ok(Statement::ForIn(ForInStatement {
                left,
                right,
                body,
                span: Span::new(start, end),
            }))
        }
    }

    /// Convert expression to pattern (for destructuring)
    fn expression_to_pattern(
        expr: &Expression<'src, 'arena>,
    ) -> ParseResult<Pattern<'src, 'arena>> {
        match expr {
            Expression::Identifier(ident) => Ok(Pattern::Identifier(ident.clone())),
            _ => Err(ParseError::invalid_syntax(expr.span(), "Invalid destructuring pattern")),
        }
    }

    /// Parse return statement
    fn parse_return_statement(&mut self) -> ParseResult<Statement<'src, 'arena>> {
        let start = self.current.span.start;
        self.expect(&TokenKind::Return, "return")?;

        let argument = if self.check(&TokenKind::Semicolon)
            || self.check(&TokenKind::RightBrace)
            || self.is_at_end()
        {
            None
        } else {
            Some(self.arena.alloc(self.parse_expression()?))
        };

        self.match_token(&TokenKind::Semicolon);
        let end = self.previous.span.end;

        Ok(Statement::Return(ReturnStatement {
            argument,
            span: Span::new(start, end),
        }))
    }

    /// Parse break statement
    fn parse_break_statement(&mut self) -> ParseResult<Statement<'src, 'arena>> {
        let start = self.current.span.start;
        self.expect(&TokenKind::Break, "break")?;

        let label = if self.is_identifier() && !self.check(&TokenKind::Semicolon) {
            Some(self.parse_identifier()?)
        } else {
            None
        };

        self.match_token(&TokenKind::Semicolon);
        let end = self.previous.span.end;

        Ok(Statement::Break(BreakStatement {
            label,
            span: Span::new(start, end),
        }))
    }

    /// Parse continue statement
    fn parse_continue_statement(&mut self) -> ParseResult<Statement<'src, 'arena>> {
        let start = self.current.span.start;
        self.expect(&TokenKind::Continue, "continue")?;

        let label = if self.is_identifier() && !self.check(&TokenKind::Semicolon) {
            Some(self.parse_identifier()?)
        } else {
            None
        };

        self.match_token(&TokenKind::Semicolon);
        let end = self.previous.span.end;

        Ok(Statement::Continue(ContinueStatement {
            label,
            span: Span::new(start, end),
        }))
    }

    /// Parse throw statement
    fn parse_throw_statement(&mut self) -> ParseResult<Statement<'src, 'arena>> {
        let start = self.current.span.start;
        self.expect(&TokenKind::Throw, "throw")?;

        // Throw must have an argument (no line terminator between throw and expression)
        let argument = self.arena.alloc(self.parse_expression()?);
        self.match_token(&TokenKind::Semicolon);
        let end = self.previous.span.end;

        Ok(Statement::Throw(ThrowStatement {
            argument,
            span: Span::new(start, end),
        }))
    }

    /// Parse try statement
    fn parse_try_statement(&mut self) -> ParseResult<Statement<'src, 'arena>> {
        let start = self.current.span.start;
        self.expect(&TokenKind::Try, "try")?;

        let block = self.parse_block_statement_inner()?;

        let handler = if self.match_token(&TokenKind::Catch) {
            let param = if self.match_token(&TokenKind::LeftParen) {
                let p = self.parse_binding_pattern()?;
                self.expect(&TokenKind::RightParen, ")")?;
                Some(p)
            } else {
                None
            };

            let body = self.parse_block_statement_inner()?;
            Some(CatchClause {
                param,
                body: body.clone(),
                span: body.span,
            })
        } else {
            None
        };

        let finalizer = if self.match_token(&TokenKind::Finally) {
            Some(self.parse_block_statement_inner()?)
        } else {
            None
        };

        let end = self.previous.span.end;
        Ok(Statement::Try(TryStatement {
            block,
            handler,
            finalizer,
            span: Span::new(start, end),
        }))
    }

    /// Parse switch statement
    fn parse_switch_statement(&mut self) -> ParseResult<Statement<'src, 'arena>> {
        let start = self.current.span.start;
        self.expect(&TokenKind::Switch, "switch")?;
        self.expect(&TokenKind::LeftParen, "(")?;
        let discriminant = self.arena.alloc(self.parse_expression()?);
        self.expect(&TokenKind::RightParen, ")")?;

        self.expect(&TokenKind::LeftBrace, "{")?;

        let mut cases = AstVecBuilder::new();
        while !self.check(&TokenKind::RightBrace) && !self.is_at_end() {
            let case = self.parse_switch_case()?;
            cases.push(case);
        }

        self.expect(&TokenKind::RightBrace, "}")?;
        let end = self.previous.span.end;

        Ok(Statement::Switch(SwitchStatement {
            discriminant,
            cases: cases.freeze(self.arena),
            span: Span::new(start, end),
        }))
    }

    /// Parse switch case
    fn parse_switch_case(&mut self) -> ParseResult<SwitchCase<'src, 'arena>> {
        let start = self.current.span.start;

        let test = if self.match_token(&TokenKind::Case) {
            Some(self.arena.alloc(self.parse_expression()?))
        } else {
            self.expect(&TokenKind::Default, "case or default")?;
            None
        };

        self.expect(&TokenKind::Colon, ":")?;

        let mut consequent = AstVecBuilder::new();
        while !self.check(&TokenKind::Case)
            && !self.check(&TokenKind::Default)
            && !self.check(&TokenKind::RightBrace)
            && !self.is_at_end()
        {
            let stmt = self.parse_statement()?;
            consequent.push(stmt);
        }

        let end = self.previous.span.end;
        Ok(SwitchCase {
            test,
            consequent: consequent.freeze(self.arena),
            span: Span::new(start, end),
        })
    }

    /// Parse block statement
    fn parse_block_statement(&mut self) -> ParseResult<Statement<'src, 'arena>> {
        let block = self.parse_block_statement_inner()?;
        Ok(Statement::Block(block))
    }

    /// Parse block statement (inner)
    #[inline]
    fn parse_block_statement_inner(&mut self) -> ParseResult<BlockStatement<'src, 'arena>> {
        let start = self.current.span.start;
        self.expect(&TokenKind::LeftBrace, "{")?;

        // Typical block has 3-8 statements
        let mut body = AstVecBuilder::with_capacity(4);
        while !self.check(&TokenKind::RightBrace) && !self.is_at_end() {
            match self.parse_statement() {
                Ok(stmt) => body.push(stmt),
                Err(e) => {
                    self.errors.push(e);
                    self.synchronize(SyncPoint::Statement);
                }
            }
        }

        self.expect(&TokenKind::RightBrace, "}")?;
        let end = self.previous.span.end;

        Ok(BlockStatement {
            body: body.freeze(self.arena),
            span: Span::new(start, end),
        })
    }

    /// Parse expression statement
    fn parse_expression_statement(&mut self) -> ParseResult<Statement<'src, 'arena>> {
        let start = self.current.span.start;
        let expression = self.arena.alloc(self.parse_expression()?);
        self.match_token(&TokenKind::Semicolon);
        let end = self.previous.span.end;

        Ok(Statement::Expression(ExpressionStatement {
            expression,
            span: Span::new(start, end),
        }))
    }

    // ========================================================================
    // Expression parsing (Pratt)
    // ========================================================================

    /// Parse expression
    fn parse_expression(&mut self) -> ParseResult<Expression<'src, 'arena>> {
        self.parse_expression_bp(BindingPower::MIN)
    }

    /// Parse assignment expression (excludes comma)
    fn parse_assignment_expression(&mut self) -> ParseResult<Expression<'src, 'arena>> {
        self.parse_expression_bp(BindingPower::COMMA)
    }

    /// Parse expression with binding power (Pratt core)
    fn parse_expression_bp(
        &mut self,
        min_bp: BindingPower,
    ) -> ParseResult<Expression<'src, 'arena>> {
        // Parse prefix (unary/primary)
        let mut lhs = self.parse_prefix_expression()?;

        // Parse infix and postfix
        loop {
            // Check for postfix operators first
            if let Some(bp) = postfix_binding_power(&self.current.kind) {
                if bp < min_bp {
                    break;
                }

                lhs = self.parse_postfix_expression(lhs)?;
                continue;
            }

            // Check for infix operators
            if let Some((left_bp, right_bp)) = infix_binding_power(&self.current.kind) {
                if left_bp < min_bp {
                    break;
                }

                lhs = self.parse_infix_expression(lhs, right_bp)?;
                continue;
            }

            break;
        }

        Ok(lhs)
    }

    /// Parse prefix expression
    fn parse_prefix_expression(&mut self) -> ParseResult<Expression<'src, 'arena>> {
        let token = &self.current;

        // Check for prefix operators
        if let Some(bp) = prefix_binding_power(&token.kind) {
            let start = self.current.span.start;
            let op_token = self.advance();

            // Unary operator
            if let Some(op) = token_to_unary_op(&op_token.kind) {
                let argument = self.arena.alloc(self.parse_expression_bp(bp)?);
                let end = self.previous.span.end;
                return Ok(Expression::Unary(UnaryExpression {
                    operator: op,
                    argument,
                    prefix: true,
                    span: Span::new(start, end),
                }));
            }

            // Update operator (prefix)
            if let Some(op) = token_to_update_op(&op_token.kind) {
                let argument = self.arena.alloc(self.parse_expression_bp(bp)?);
                let end = self.previous.span.end;
                return Ok(Expression::Update(UpdateExpression {
                    operator: op,
                    argument,
                    prefix: true,
                    span: Span::new(start, end),
                }));
            }

            // Await
            if matches!(op_token.kind, TokenKind::Await) {
                let argument = self.arena.alloc(self.parse_expression_bp(bp)?);
                let end = self.previous.span.end;
                return Ok(Expression::Await(AwaitExpression {
                    argument,
                    span: Span::new(start, end),
                }));
            }

            // Yield
            if matches!(op_token.kind, TokenKind::Yield) {
                let delegate = self.match_token(&TokenKind::Star);
                let argument = if self.check(&TokenKind::Semicolon)
                    || self.check(&TokenKind::RightBrace)
                    || self.is_at_end()
                {
                    None
                } else {
                    Some(self.arena.alloc(self.parse_expression_bp(bp)?))
                };
                let end = self.previous.span.end;
                return Ok(Expression::Yield(YieldExpression {
                    argument,
                    delegate,
                    span: Span::new(start, end),
                }));
            }

            // New expression
            if matches!(op_token.kind, TokenKind::New) {
                return self.parse_new_expression(start);
            }
        }

        // Primary expression
        self.parse_primary_expression()
    }

    /// Parse new expression
    fn parse_new_expression(&mut self, start: u32) -> ParseResult<Expression<'src, 'arena>> {
        let callee = self
            .arena
            .alloc(self.parse_expression_bp(BindingPower::NEW)?);

        let arguments = if self.match_token(&TokenKind::LeftParen) {
            self.parse_arguments()?
        } else {
            &[]
        };

        let end = self.previous.span.end;
        Ok(Expression::New(NewExpression {
            callee,
            arguments,
            span: Span::new(start, end),
        }))
    }

    /// Parse infix expression
    fn parse_infix_expression(
        &mut self,
        lhs: Expression<'src, 'arena>,
        right_bp: BindingPower,
    ) -> ParseResult<Expression<'src, 'arena>> {
        let start = lhs.span().start;
        let op_token = self.advance();

        // Assignment operators
        if let Some(op) = token_to_assignment_op(&op_token.kind) {
            let target = Self::expression_to_assignment_target(lhs)?;
            let right = self.arena.alloc(self.parse_expression_bp(right_bp)?);
            let end = self.previous.span.end;
            return Ok(Expression::Assignment(AssignmentExpression {
                operator: op,
                left: target,
                right,
                span: Span::new(start, end),
            }));
        }

        // Conditional (ternary)
        if matches!(op_token.kind, TokenKind::Question) {
            let consequent = self.arena.alloc(self.parse_assignment_expression()?);
            self.expect(&TokenKind::Colon, ":")?;
            let alternate = self.arena.alloc(self.parse_expression_bp(right_bp)?);
            let end = self.previous.span.end;
            return Ok(Expression::Conditional(ConditionalExpression {
                test: self.arena.alloc(lhs),
                consequent,
                alternate,
                span: Span::new(start, end),
            }));
        }

        // Logical operators
        if let Some(op) = token_to_logical_op(&op_token.kind) {
            let right = self.arena.alloc(self.parse_expression_bp(right_bp)?);
            let end = self.previous.span.end;
            return Ok(Expression::Logical(LogicalExpression {
                operator: op,
                left: self.arena.alloc(lhs),
                right,
                span: Span::new(start, end),
            }));
        }

        // Binary operators
        if let Some(op) = token_to_binary_op(&op_token.kind) {
            let right = self.arena.alloc(self.parse_expression_bp(right_bp)?);
            let end = self.previous.span.end;
            return Ok(Expression::Binary(BinaryExpression {
                operator: op,
                left: self.arena.alloc(lhs),
                right,
                span: Span::new(start, end),
            }));
        }

        // Comma operator
        if matches!(op_token.kind, TokenKind::Comma) {
            let mut expressions = AstVecBuilder::new();
            expressions.push(lhs);
            expressions.push(self.parse_expression_bp(right_bp)?);
            while self.match_token(&TokenKind::Comma) {
                expressions.push(self.parse_expression_bp(right_bp)?);
            }
            let end = self.previous.span.end;
            return Ok(Expression::Sequence(SequenceExpression {
                expressions: expressions.freeze(self.arena),
                span: Span::new(start, end),
            }));
        }

        Err(ParseError::unexpected_token(
            op_token.span,
            vec!["operator"],
            &format!("{:?}", op_token.kind),
        ))
    }

    /// Parse postfix expression
    fn parse_postfix_expression(
        &mut self,
        lhs: Expression<'src, 'arena>,
    ) -> ParseResult<Expression<'src, 'arena>> {
        let start = lhs.span().start;

        // Postfix update operators
        if let Some(op) = token_to_update_op(&self.current.kind) {
            self.advance();
            let end = self.previous.span.end;
            return Ok(Expression::Update(UpdateExpression {
                operator: op,
                argument: self.arena.alloc(lhs),
                prefix: false,
                span: Span::new(start, end),
            }));
        }

        // Member access (dot)
        if self.match_token(&TokenKind::Dot) {
            let property = self
                .arena
                .alloc(Expression::Identifier(self.parse_identifier()?));
            let end = self.previous.span.end;
            return Ok(Expression::Member(MemberExpression {
                object: self.arena.alloc(lhs),
                property,
                computed: false,
                optional: false,
                span: Span::new(start, end),
            }));
        }

        // Optional chaining
        if self.match_token(&TokenKind::QuestionDot) {
            if self.check(&TokenKind::LeftParen) {
                // Optional call: x?.()
                self.advance();
                let arguments = self.parse_arguments()?;
                let end = self.previous.span.end;
                return Ok(Expression::Call(CallExpression {
                    callee: self.arena.alloc(lhs),
                    arguments,
                    optional: true,
                    span: Span::new(start, end),
                }));
            } else if self.check(&TokenKind::LeftBracket) {
                // Optional computed member: x?.[y]
                self.advance();
                let property = self.arena.alloc(self.parse_expression()?);
                self.expect(&TokenKind::RightBracket, "]")?;
                let end = self.previous.span.end;
                return Ok(Expression::Member(MemberExpression {
                    object: self.arena.alloc(lhs),
                    property,
                    computed: true,
                    optional: true,
                    span: Span::new(start, end),
                }));
            }
            // Optional member: x?.y
            let property = self
                .arena
                .alloc(Expression::Identifier(self.parse_identifier()?));
            let end = self.previous.span.end;
            return Ok(Expression::Member(MemberExpression {
                object: self.arena.alloc(lhs),
                property,
                computed: false,
                optional: true,
                span: Span::new(start, end),
            }));
        }

        // Computed member access
        if self.match_token(&TokenKind::LeftBracket) {
            let property = self.arena.alloc(self.parse_expression()?);
            self.expect(&TokenKind::RightBracket, "]")?;
            let end = self.previous.span.end;
            return Ok(Expression::Member(MemberExpression {
                object: self.arena.alloc(lhs),
                property,
                computed: true,
                optional: false,
                span: Span::new(start, end),
            }));
        }

        // Call expression
        if self.match_token(&TokenKind::LeftParen) {
            let arguments = self.parse_arguments()?;
            let end = self.previous.span.end;
            return Ok(Expression::Call(CallExpression {
                callee: self.arena.alloc(lhs),
                arguments,
                optional: false,
                span: Span::new(start, end),
            }));
        }

        // Template literal tag
        if matches!(self.current.kind, TokenKind::Template(_)) {
            let quasi = self.parse_template_literal()?;
            let end = self.previous.span.end;
            return Ok(Expression::TaggedTemplate(TaggedTemplateExpression {
                tag: self.arena.alloc(lhs),
                quasi,
                span: Span::new(start, end),
            }));
        }

        Err(ParseError::unexpected_token(
            self.current.span,
            vec!["postfix operator"],
            &format!("{:?}", self.current.kind),
        ))
    }

    /// Parse arguments list
    fn parse_arguments(&mut self) -> ParseResult<AstVec<'arena, Argument<'src, 'arena>>> {
        let mut args = AstVecBuilder::new();

        while !self.check(&TokenKind::RightParen) && !self.is_at_end() {
            if self.check(&TokenKind::Ellipsis) {
                self.advance();
                let arg = self.parse_assignment_expression()?;
                args.push(Argument::Spread(SpreadElement {
                    argument: self.arena.alloc(arg),
                    span: self.previous.span,
                }));
            } else {
                let arg = self.parse_assignment_expression()?;
                args.push(Argument::Expression(arg));
            }

            if !self.check(&TokenKind::RightParen) {
                self.expect(&TokenKind::Comma, ",")?;
            }
        }

        self.expect(&TokenKind::RightParen, ")")?;
        Ok(args.freeze(self.arena))
    }

    /// Parse primary expression
    fn parse_primary_expression(&mut self) -> ParseResult<Expression<'src, 'arena>> {
        match &self.current.kind {
            // Identifiers
            TokenKind::Identifier(_) => {
                let ident = self.parse_identifier()?;
                Ok(Expression::Identifier(ident))
            }

            // Literals
            TokenKind::Integer(_) | TokenKind::Float(_) => self.parse_number_literal(),
            TokenKind::String(_) => self.parse_string_literal(),
            TokenKind::True => {
                let span = self.advance().span;
                Ok(Expression::Literal(Literal::Boolean(BooleanLiteral { value: true, span })))
            }
            TokenKind::False => {
                let span = self.advance().span;
                Ok(Expression::Literal(Literal::Boolean(BooleanLiteral { value: false, span })))
            }
            TokenKind::Null => {
                let span = self.advance().span;
                Ok(Expression::Literal(Literal::Null(span)))
            }
            TokenKind::RegExp(_) => self.parse_regexp_literal(),

            // This/Super
            TokenKind::This => {
                let span = self.advance().span;
                Ok(Expression::This(span))
            }
            TokenKind::Super => {
                let span = self.advance().span;
                Ok(Expression::Super(span))
            }

            // Grouping/Arrow
            TokenKind::LeftParen => self.parse_parenthesized_or_arrow(),

            // Array literal
            TokenKind::LeftBracket => self.parse_array_literal(),

            // Object literal
            TokenKind::LeftBrace => self.parse_object_literal(),

            // Function expression
            TokenKind::Function => self.parse_function_expression(),

            // Class expression
            TokenKind::Class => self.parse_class_expression(),

            // Async function/arrow
            TokenKind::Async => self.parse_async_expression(),

            // Template literal
            TokenKind::Template(_) => {
                let lit = self.parse_template_literal()?;
                Ok(Expression::TemplateLiteral(lit))
            }

            _ => Err(ParseError::unexpected_token(
                self.current.span,
                vec!["expression"],
                &format!("{:?}", self.current.kind),
            )),
        }
    }

    /// Parse identifier
    fn parse_identifier(&mut self) -> ParseResult<Identifier<'src>> {
        match &self.current.kind {
            TokenKind::Identifier(symbol) => {
                let sym = *symbol;
                let token = self.advance();
                Ok(Identifier {
                    name: sym,
                    raw: self.text(token.span),
                    span: token.span,
                })
            }
            _ => Err(ParseError::unexpected_token(
                self.current.span,
                vec!["identifier"],
                &format!("{:?}", self.current.kind),
            )),
        }
    }

    /// Parse number literal
    fn parse_number_literal(&mut self) -> ParseResult<Expression<'src, 'arena>> {
        let token = self.advance();
        let raw = self.text(token.span);

        let value = match &token.kind {
            TokenKind::Integer(s) | TokenKind::Float(s) => {
                // Parse the number
                if s.starts_with("0x") || s.starts_with("0X") {
                    i64::from_str_radix(&s[2..].replace('_', ""), 16).unwrap_or(0) as f64
                } else if s.starts_with("0o") || s.starts_with("0O") {
                    i64::from_str_radix(&s[2..].replace('_', ""), 8).unwrap_or(0) as f64
                } else if s.starts_with("0b") || s.starts_with("0B") {
                    i64::from_str_radix(&s[2..].replace('_', ""), 2).unwrap_or(0) as f64
                } else {
                    s.replace('_', "").parse::<f64>().unwrap_or(0.0)
                }
            }
            _ => unreachable!(),
        };

        Ok(Expression::Literal(Literal::Number(NumberLiteral {
            value,
            raw,
            span: token.span,
        })))
    }

    /// Parse string literal
    fn parse_string_literal(&mut self) -> ParseResult<Expression<'src, 'arena>> {
        let token = self.advance();
        let raw = self.text(token.span);
        // Remove quotes for value
        let value = &raw[1..raw.len() - 1];

        Ok(Expression::Literal(Literal::String(StringLiteral {
            value,
            raw,
            span: token.span,
        })))
    }

    /// Parse regexp literal
    fn parse_regexp_literal(&mut self) -> ParseResult<Expression<'src, 'arena>> {
        let token = self.advance();
        match &token.kind {
            TokenKind::RegExp(pattern) => {
                // Parse pattern and flags from the single string
                // Format: /pattern/flags
                let s = *pattern;
                let (pat, flags) = if let Some(last_slash) = s.rfind('/') {
                    if last_slash > 0 {
                        (&s[1..last_slash], &s[last_slash + 1..])
                    } else {
                        (s, "")
                    }
                } else {
                    (s, "")
                };

                Ok(Expression::Literal(Literal::RegExp(RegExpLiteral {
                    pattern: pat,
                    flags,
                    span: token.span,
                })))
            }
            _ => unreachable!(),
        }
    }

    /// Parse parenthesized expression or arrow function
    fn parse_parenthesized_or_arrow(&mut self) -> ParseResult<Expression<'src, 'arena>> {
        let start = self.current.span.start;
        self.expect(&TokenKind::LeftParen, "(")?;

        // Empty parens => arrow function
        if self.check(&TokenKind::RightParen) {
            self.advance();
            if self.check(&TokenKind::Arrow) {
                return self.parse_arrow_function(start, Vec::new(), false);
            }
            return Err(ParseError::invalid_syntax(
                self.current.span,
                "Expected '=>' after empty parameter list",
            ));
        }

        // Parse expression
        let expr = self.parse_expression()?;
        self.expect(&TokenKind::RightParen, ")")?;

        // Check for arrow function
        if self.check(&TokenKind::Arrow) {
            // Convert expression to parameters
            let params = Self::expression_to_params(&expr)?;
            return self.parse_arrow_function(start, params, false);
        }

        let end = self.previous.span.end;
        Ok(Expression::Parenthesized(ParenthesizedExpression {
            expression: self.arena.alloc(expr),
            span: Span::new(start, end),
        }))
    }

    /// Convert expression to arrow function parameters
    fn expression_to_params(
        expr: &Expression<'src, 'arena>,
    ) -> ParseResult<Vec<Pattern<'src, 'arena>>> {
        match expr {
            Expression::Identifier(ident) => Ok(vec![Pattern::Identifier(ident.clone())]),
            Expression::Sequence(seq) => {
                let mut params = Vec::new();
                for e in seq.expressions {
                    params.extend(Self::expression_to_params(e)?);
                }
                Ok(params)
            }
            _ => Err(ParseError::invalid_syntax(expr.span(), "Invalid arrow function parameter")),
        }
    }

    /// Parse arrow function
    fn parse_arrow_function(
        &mut self,
        start: u32,
        params: Vec<Pattern<'src, 'arena>>,
        is_async: bool,
    ) -> ParseResult<Expression<'src, 'arena>> {
        self.expect(&TokenKind::Arrow, "=>")?;

        let body = if self.check(&TokenKind::LeftBrace) {
            ArrowBody::Block(self.parse_block_statement_inner()?)
        } else {
            ArrowBody::Expression(self.arena.alloc(self.parse_assignment_expression()?))
        };

        let params = if params.is_empty() {
            &[]
        } else {
            self.arena.alloc_slice_from_iter(params)
        };

        let end = self.previous.span.end;
        Ok(Expression::Arrow(ArrowFunctionExpression {
            params,
            body,
            is_async,
            span: Span::new(start, end),
        }))
    }

    /// Parse array literal
    fn parse_array_literal(&mut self) -> ParseResult<Expression<'src, 'arena>> {
        let start = self.current.span.start;
        self.expect(&TokenKind::LeftBracket, "[")?;

        let mut elements = AstVecBuilder::new();
        while !self.check(&TokenKind::RightBracket) && !self.is_at_end() {
            if self.check(&TokenKind::Comma) {
                // Elision
                elements.push(ArrayElement::Hole);
            } else if self.check(&TokenKind::Ellipsis) {
                self.advance();
                let arg = self.parse_assignment_expression()?;
                elements.push(ArrayElement::Spread(SpreadElement {
                    argument: self.arena.alloc(arg),
                    span: self.previous.span,
                }));
            } else {
                let expr = self.parse_assignment_expression()?;
                elements.push(ArrayElement::Expression(expr));
            }

            if !self.check(&TokenKind::RightBracket) {
                self.expect(&TokenKind::Comma, ",")?;
            }
        }

        self.expect(&TokenKind::RightBracket, "]")?;
        let end = self.previous.span.end;

        Ok(Expression::Array(ArrayExpression {
            elements: elements.freeze(self.arena),
            span: Span::new(start, end),
        }))
    }

    /// Parse object literal
    fn parse_object_literal(&mut self) -> ParseResult<Expression<'src, 'arena>> {
        let start = self.current.span.start;
        self.expect(&TokenKind::LeftBrace, "{")?;

        let mut properties = AstVecBuilder::new();
        while !self.check(&TokenKind::RightBrace) && !self.is_at_end() {
            if self.check(&TokenKind::Ellipsis) {
                self.advance();
                let arg = self.parse_assignment_expression()?;
                properties.push(ObjectProperty::SpreadProperty(SpreadElement {
                    argument: self.arena.alloc(arg),
                    span: self.previous.span,
                }));
            } else {
                let prop = self.parse_object_property()?;
                properties.push(ObjectProperty::Property(prop));
            }

            if !self.check(&TokenKind::RightBrace) {
                self.expect(&TokenKind::Comma, ",")?;
            }
        }

        self.expect(&TokenKind::RightBrace, "}")?;
        let end = self.previous.span.end;

        Ok(Expression::Object(ObjectExpression {
            properties: properties.freeze(self.arena),
            span: Span::new(start, end),
        }))
    }

    /// Parse object property
    fn parse_object_property(&mut self) -> ParseResult<Property<'src, 'arena>> {
        let start = self.current.span.start;
        let computed = self.check(&TokenKind::LeftBracket);

        // Check for get/set
        let kind = PropertyKind::Init;
        let mut method = false;

        let key = self.parse_property_key()?;

        // Shorthand property: { x } is { x: x }
        if !computed
            && !self.check(&TokenKind::Colon)
            && !self.check(&TokenKind::LeftParen)
            && matches!(&key, PropertyKey::Identifier(_))
        {
            let ident = match &key {
                PropertyKey::Identifier(i) => i.clone(),
                _ => unreachable!(),
            };
            let end = self.previous.span.end;
            return Ok(Property {
                key,
                value: self.arena.alloc(Expression::Identifier(ident)),
                kind: PropertyKind::Init,
                method: false,
                shorthand: true,
                computed: false,
                span: Span::new(start, end),
            });
        }

        // Method: { x() {} }
        if self.check(&TokenKind::LeftParen) {
            method = true;
            self.advance();
            let params = self.parse_function_params()?;
            self.expect(&TokenKind::RightParen, ")")?;
            let body = self.parse_block_statement_inner()?;
            let end = self.previous.span.end;

            let func = Expression::Function(FunctionExpression {
                id: None,
                params,
                body,
                is_async: false,
                is_generator: false,
                span: Span::new(start, end),
            });

            return Ok(Property {
                key,
                value: self.arena.alloc(func),
                kind,
                method,
                shorthand: false,
                computed,
                span: Span::new(start, end),
            });
        }

        // Regular property: { x: y }
        self.expect(&TokenKind::Colon, ":")?;
        let value = self.arena.alloc(self.parse_assignment_expression()?);
        let end = self.previous.span.end;

        Ok(Property {
            key,
            value,
            kind,
            method,
            shorthand: false,
            computed,
            span: Span::new(start, end),
        })
    }

    /// Parse property key
    fn parse_property_key(&mut self) -> ParseResult<PropertyKey<'src, 'arena>> {
        if self.match_token(&TokenKind::LeftBracket) {
            let expr = self.parse_assignment_expression()?;
            self.expect(&TokenKind::RightBracket, "]")?;
            return Ok(PropertyKey::Computed(self.arena.alloc(expr)));
        }

        match &self.current.kind {
            TokenKind::Identifier(_) => {
                let ident = self.parse_identifier()?;
                Ok(PropertyKey::Identifier(ident))
            }
            TokenKind::String(_) => {
                let lit = self.parse_string_literal()?;
                match lit {
                    Expression::Literal(l) => Ok(PropertyKey::Literal(l)),
                    _ => unreachable!(),
                }
            }
            TokenKind::Integer(_) | TokenKind::Float(_) => {
                let lit = self.parse_number_literal()?;
                match lit {
                    Expression::Literal(l) => Ok(PropertyKey::Literal(l)),
                    _ => unreachable!(),
                }
            }
            _ => Err(ParseError::unexpected_token(
                self.current.span,
                vec!["property key"],
                &format!("{:?}", self.current.kind),
            )),
        }
    }

    /// Parse function expression
    fn parse_function_expression(&mut self) -> ParseResult<Expression<'src, 'arena>> {
        let start = self.current.span.start;
        let is_async = self.previous.kind == TokenKind::Async;
        self.expect(&TokenKind::Function, "function")?;

        let is_generator = self.match_token(&TokenKind::Star);

        let id = if self.is_identifier() {
            Some(self.parse_identifier()?)
        } else {
            None
        };

        self.expect(&TokenKind::LeftParen, "(")?;
        let params = self.parse_function_params()?;
        self.expect(&TokenKind::RightParen, ")")?;

        let body = self.parse_block_statement_inner()?;
        let end = self.previous.span.end;

        Ok(Expression::Function(FunctionExpression {
            id,
            params,
            body,
            is_async,
            is_generator,
            span: Span::new(start, end),
        }))
    }

    /// Parse class expression
    fn parse_class_expression(&mut self) -> ParseResult<Expression<'src, 'arena>> {
        let start = self.current.span.start;
        self.expect(&TokenKind::Class, "class")?;

        let id = if self.is_identifier() {
            Some(self.parse_identifier()?)
        } else {
            None
        };

        let super_class = if self.match_token(&TokenKind::Extends) {
            Some(self.arena.alloc(self.parse_left_hand_side_expression()?))
        } else {
            None
        };

        let body = self.parse_class_body()?;
        let end = self.previous.span.end;

        Ok(Expression::Class(ClassExpression {
            id,
            super_class,
            body,
            span: Span::new(start, end),
        }))
    }

    /// Parse async expression (function or arrow)
    fn parse_async_expression(&mut self) -> ParseResult<Expression<'src, 'arena>> {
        let start = self.current.span.start;
        self.expect(&TokenKind::Async, "async")?;

        if self.check(&TokenKind::Function) {
            // async function
            self.advance();
            let is_generator = self.match_token(&TokenKind::Star);

            let id = if self.is_identifier() {
                Some(self.parse_identifier()?)
            } else {
                None
            };

            self.expect(&TokenKind::LeftParen, "(")?;
            let params = self.parse_function_params()?;
            self.expect(&TokenKind::RightParen, ")")?;

            let body = self.parse_block_statement_inner()?;
            let end = self.previous.span.end;

            return Ok(Expression::Function(FunctionExpression {
                id,
                params,
                body,
                is_async: true,
                is_generator,
                span: Span::new(start, end),
            }));
        }

        // async arrow function
        if self.check(&TokenKind::LeftParen) {
            self.advance();
            if self.check(&TokenKind::RightParen) {
                self.advance();
                return self.parse_arrow_function(start, Vec::new(), true);
            }
            let expr = self.parse_expression()?;
            self.expect(&TokenKind::RightParen, ")")?;
            let params = Self::expression_to_params(&expr)?;
            return self.parse_arrow_function(start, params, true);
        }

        // async x => ...
        if self.is_identifier() {
            let ident = self.parse_identifier()?;
            let params = vec![Pattern::Identifier(ident)];
            return self.parse_arrow_function(start, params, true);
        }

        Err(ParseError::unexpected_token(
            self.current.span,
            vec!["function", "(", "identifier"],
            &format!("{:?}", self.current.kind),
        ))
    }

    /// Parse template literal
    /// Note: The lexer produces a single Template token for the entire template.
    /// Full template literal parsing with expressions would require lexer changes.
    fn parse_template_literal(&mut self) -> ParseResult<TemplateLiteral<'src, 'arena>> {
        let start = self.current.span.start;
        let token = self.advance();

        match &token.kind {
            TokenKind::Template(raw_str) => {
                // For now, treat as a single quasi (no interpolation)
                // Full template parsing would need lexer to produce Head/Middle/Tail tokens
                let raw = *raw_str;
                let cooked = if raw.len() >= 2 {
                    Some(&raw[1..raw.len() - 1]) // Remove backticks
                } else {
                    Some(raw)
                };

                let quasis = self
                    .arena
                    .alloc_slice_from_iter(std::iter::once(TemplateElement {
                        raw: cooked.unwrap_or(""),
                        cooked,
                        tail: true,
                        span: token.span,
                    }));

                Ok(TemplateLiteral {
                    quasis,
                    expressions: &[],
                    span: Span::new(start, token.span.end),
                })
            }
            _ => Err(ParseError::unexpected_token(
                token.span,
                vec!["template literal"],
                &format!("{:?}", token.kind),
            )),
        }
    }

    /// Parse left-hand side expression (for new, call, member)
    fn parse_left_hand_side_expression(&mut self) -> ParseResult<Expression<'src, 'arena>> {
        self.parse_expression_bp(BindingPower::NEW)
    }

    /// Convert expression to assignment target
    fn expression_to_assignment_target(
        expr: Expression<'src, 'arena>,
    ) -> ParseResult<AssignmentTarget<'src, 'arena>> {
        match expr {
            Expression::Identifier(ident) => Ok(AssignmentTarget::Identifier(ident)),
            Expression::Member(member) => Ok(AssignmentTarget::Member(member)),
            _ => Err(ParseError::invalid_assignment_target(expr.span())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_expression() {
        let arena = AstArena::new();
        let (program, errors) = Parser::new("42;", &arena).parse();
        assert!(errors.is_empty());
        assert_eq!(program.body.len(), 1);
    }

    #[test]
    fn test_variable_declaration() {
        let arena = AstArena::new();
        let (program, errors) = Parser::new("let x = 10;", &arena).parse();
        assert!(errors.is_empty());
        assert_eq!(program.body.len(), 1);
        match &program.body[0] {
            Statement::VariableDeclaration(decl) => {
                assert_eq!(decl.kind, VariableKind::Let);
                assert_eq!(decl.declarations.len(), 1);
            }
            _ => panic!("Expected variable declaration"),
        }
    }

    #[test]
    fn test_function_declaration() {
        let arena = AstArena::new();
        let (program, errors) = Parser::new("function add(a, b) { return a + b; }", &arena).parse();
        assert!(errors.is_empty());
        assert_eq!(program.body.len(), 1);
        match &program.body[0] {
            Statement::FunctionDeclaration(func) => {
                assert!(func.id.is_some());
                assert_eq!(func.params.len(), 2);
            }
            _ => panic!("Expected function declaration"),
        }
    }

    #[test]
    fn test_if_statement() {
        let arena = AstArena::new();
        let (program, errors) = Parser::new("if (x) { y; } else { z; }", &arena).parse();
        assert!(errors.is_empty());
        match &program.body[0] {
            Statement::If(if_stmt) => {
                assert!(if_stmt.alternate.is_some());
            }
            _ => panic!("Expected if statement"),
        }
    }

    #[test]
    fn test_binary_expression() {
        let arena = AstArena::new();
        let (_program, errors) = Parser::new("1 + 2 * 3;", &arena).parse();
        assert!(errors.is_empty());
        // The AST should reflect precedence: 1 + (2 * 3)
    }

    #[test]
    fn test_arrow_function() {
        let arena = AstArena::new();
        let (_program, errors) = Parser::new("const f = (x) => x * 2;", &arena).parse();
        assert!(errors.is_empty());
    }

    #[test]
    fn test_class_declaration() {
        let arena = AstArena::new();
        let (program, errors) =
            Parser::new("class Foo { constructor() {} bar() {} }", &arena).parse();
        assert!(errors.is_empty());
        match &program.body[0] {
            Statement::ClassDeclaration(cls) => {
                assert_eq!(cls.body.body.len(), 2);
            }
            _ => panic!("Expected class declaration"),
        }
    }

    #[test]
    fn test_error_recovery() {
        let arena = AstArena::new();
        let (_program, errors) = Parser::new("let x = ; let y = 2;", &arena).parse();
        // Should recover and parse second declaration
        assert!(!errors.is_empty());
    }
}
