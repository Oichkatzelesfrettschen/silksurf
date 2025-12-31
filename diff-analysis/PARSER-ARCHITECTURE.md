# SilkSurf JS Parser Architecture
**Version**: 1.0
**Date**: 2025-12-30
**Status**: Design Specification (Pre-Implementation)
**Target**: Arena-allocated AST with zero manual deallocation

---

## EXECUTIVE SUMMARY

**What**: Recursive descent parser with Pratt expression parsing, producing arena-allocated AST.

**Why**: Eliminate Boa's 8.5% memory leak rate through automatic arena cleanup.

**How**: AST nodes stored in `Bump` arena, freed in single operation when compilation complete.

**Performance Targets**:
- **Throughput**: >20,000 LOC/s (parse jquery-3.7.1.js in <500ms)
- **Memory**: <10 MB peak for 10K LOC (arena-based, no leaks)
- **Allocations**: Linear in AST size (bump allocation, no malloc overhead)

**Test262 Compliance**: 100% of language tests (51 failures from Phase 0 gap analysis)

---

## 1. ARENA-ALLOCATED AST DESIGN

### 1.1 Why Arena Allocation?

**Problem** (from Phase 0 fuzzing):
- Boa leaked 7,506 allocations (8.5% leak rate)
- Manual deallocation error-prone (async generator strings not freed)
- Complex object graphs hard to track

**Solution**:
```rust
// Single arena for entire compilation unit
let arena = Bump::new();

// Parse source → AST
let ast = parse(&arena, source);

// Use AST (compile to bytecode, analyze, etc.)
compile(ast);

// Drop arena → ALL AST nodes freed in one operation!
// No manual deallocation, zero leaks guaranteed
```

**Benefits**:
1. **Zero leaks**: Arena dropped = all nodes freed
2. **Fast allocation**: Bump pointer (no malloc overhead)
3. **Cache friendly**: Linear memory layout (better CPU cache utilization)
4. **Compile-time safety**: Rust borrow checker ensures no dangling references

---

### 1.2 AST Node Design

**Core Pattern**:
```rust
// All AST nodes use arena lifetime 'ast
pub enum Expr<'ast> {
    Number { value: f64, span: Span },
    String { value: &'ast str, span: Span },  // Zero-copy string
    Identifier { name: &'ast str, span: Span },
    Binary {
        op: BinaryOp,
        left: &'ast Expr<'ast>,   // Arena-allocated child
        right: &'ast Expr<'ast>,  // Arena-allocated child
        span: Span,
    },
    Call {
        callee: &'ast Expr<'ast>,
        args: &'ast [Expr<'ast>],  // Arena-allocated slice
        span: Span,
    },
    // ... (60+ expression variants)
}
```

**Key Properties**:
- **Lifetime `'ast`**: Tied to arena lifetime
- **References**: `&'ast T`, not `Box<T>` (zero ownership overhead)
- **Slices**: `&'ast [T]`, not `Vec<T>` (allocated in arena)
- **Strings**: `&'ast str`, not `String` (zero-copy from source)

---

### 1.3 Complete AST Type Hierarchy

**Statements**:
```rust
pub enum Stmt<'ast> {
    // Variable declarations
    Let {
        declarations: &'ast [VarDeclarator<'ast>],
        span: Span,
    },
    Const {
        declarations: &'ast [VarDeclarator<'ast>],
        span: Span,
    },
    Var {
        declarations: &'ast [VarDeclarator<'ast>],
        span: Span,
    },

    // Control flow
    If {
        test: &'ast Expr<'ast>,
        consequent: &'ast Stmt<'ast>,
        alternate: Option<&'ast Stmt<'ast>>,
        span: Span,
    },
    While {
        test: &'ast Expr<'ast>,
        body: &'ast Stmt<'ast>,
        span: Span,
    },
    For {
        init: Option<ForInit<'ast>>,
        test: Option<&'ast Expr<'ast>>,
        update: Option<&'ast Expr<'ast>>,
        body: &'ast Stmt<'ast>,
        span: Span,
    },
    ForIn {
        left: ForInLeft<'ast>,
        right: &'ast Expr<'ast>,
        body: &'ast Stmt<'ast>,
        span: Span,
    },
    ForOf {
        left: ForOfLeft<'ast>,
        right: &'ast Expr<'ast>,
        body: &'ast Stmt<'ast>,
        is_await: bool,
        span: Span,
    },

    // Exception handling
    Try {
        block: &'ast BlockStmt<'ast>,
        handler: Option<&'ast CatchClause<'ast>>,
        finalizer: Option<&'ast BlockStmt<'ast>>,
        span: Span,
    },
    Throw {
        argument: &'ast Expr<'ast>,
        span: Span,
    },

    // Jumps
    Return {
        argument: Option<&'ast Expr<'ast>>,
        span: Span,
    },
    Break {
        label: Option<&'ast str>,
        span: Span,
    },
    Continue {
        label: Option<&'ast str>,
        span: Span,
    },

    // Labeled statement
    Labeled {
        label: &'ast str,
        body: &'ast Stmt<'ast>,
        span: Span,
    },

    // Switch
    Switch {
        discriminant: &'ast Expr<'ast>,
        cases: &'ast [SwitchCase<'ast>],
        span: Span,
    },

    // Declarations
    FunctionDecl {
        id: &'ast str,
        params: &'ast [Pattern<'ast>],
        body: &'ast BlockStmt<'ast>,
        is_async: bool,
        is_generator: bool,
        span: Span,
    },
    ClassDecl {
        id: &'ast str,
        super_class: Option<&'ast Expr<'ast>>,
        body: &'ast [ClassMember<'ast>],
        span: Span,
    },

    // Other
    Expression {
        expr: &'ast Expr<'ast>,
        span: Span,
    },
    Block {
        body: &'ast [Stmt<'ast>],
        span: Span,
    },
    Empty {
        span: Span,
    },
    Debugger {
        span: Span,
    },
    With {
        object: &'ast Expr<'ast>,
        body: &'ast Stmt<'ast>,
        span: Span,
    },
}
```

**Expressions** (60+ variants):
```rust
pub enum Expr<'ast> {
    // Literals
    Number { value: f64, span: Span },
    BigInt { value: &'ast str, span: Span },
    String { value: &'ast str, span: Span },
    Boolean { value: bool, span: Span },
    Null { span: Span },
    Undefined { span: Span },
    RegExp { pattern: &'ast str, flags: &'ast str, span: Span },
    Template {
        quasis: &'ast [TemplateElement<'ast>],
        expressions: &'ast [Expr<'ast>],
        span: Span,
    },

    // Identifiers
    Identifier { name: &'ast str, span: Span },

    // Binary operations
    Binary {
        op: BinaryOp,
        left: &'ast Expr<'ast>,
        right: &'ast Expr<'ast>,
        span: Span,
    },
    Logical {
        op: LogicalOp,
        left: &'ast Expr<'ast>,
        right: &'ast Expr<'ast>,
        span: Span,
    },
    Unary {
        op: UnaryOp,
        argument: &'ast Expr<'ast>,
        prefix: bool,
        span: Span,
    },
    Update {
        op: UpdateOp,
        argument: &'ast Expr<'ast>,
        prefix: bool,
        span: Span,
    },

    // Assignment
    Assignment {
        op: AssignmentOp,
        left: &'ast Pattern<'ast>,
        right: &'ast Expr<'ast>,
        span: Span,
    },

    // Function calls
    Call {
        callee: &'ast Expr<'ast>,
        args: &'ast [Expr<'ast>],
        span: Span,
    },
    New {
        callee: &'ast Expr<'ast>,
        args: &'ast [Expr<'ast>],
        span: Span,
    },

    // Member access
    Member {
        object: &'ast Expr<'ast>,
        property: MemberProperty<'ast>,
        computed: bool,  // a[b] vs a.b
        optional: bool,  // a?.b
        span: Span,
    },

    // Conditional
    Conditional {
        test: &'ast Expr<'ast>,
        consequent: &'ast Expr<'ast>,
        alternate: &'ast Expr<'ast>,
        span: Span,
    },

    // Sequence
    Sequence {
        expressions: &'ast [Expr<'ast>],
        span: Span,
    },

    // Array/Object
    Array {
        elements: &'ast [Option<Expr<'ast>>],  // None = hole
        span: Span,
    },
    Object {
        properties: &'ast [Property<'ast>],
        span: Span,
    },

    // Function expressions
    Function {
        id: Option<&'ast str>,
        params: &'ast [Pattern<'ast>],
        body: &'ast BlockStmt<'ast>,
        is_async: bool,
        is_generator: bool,
        span: Span,
    },
    Arrow {
        params: &'ast [Pattern<'ast>],
        body: ArrowBody<'ast>,  // Expr or BlockStmt
        is_async: bool,
        span: Span,
    },

    // Class expression
    Class {
        id: Option<&'ast str>,
        super_class: Option<&'ast Expr<'ast>>,
        body: &'ast [ClassMember<'ast>],
        span: Span,
    },

    // Other
    This { span: Span },
    Super { span: Span },
    Await { argument: &'ast Expr<'ast>, span: Span },
    Yield {
        argument: Option<&'ast Expr<'ast>>,
        delegate: bool,
        span: Span,
    },
    MetaProperty {
        meta: &'ast str,    // "new" or "import"
        property: &'ast str, // "target" or "meta"
        span: Span,
    },
}
```

---

## 2. PRATT PARSER FOR EXPRESSIONS

### 2.1 Why Pratt Parsing?

**Advantages**:
- Handles operator precedence elegantly
- Extensible (easy to add new operators)
- Efficient (single-pass, no backtracking)

**Precedence Table** (ES2025):
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Precedence {
    Lowest = 0,
    Comma = 1,           // ,
    Assignment = 2,      // = += -= etc.
    Conditional = 3,     // ? :
    NullishCoalescing = 4, // ??
    LogicalOr = 5,       // ||
    LogicalAnd = 6,      // &&
    BitwiseOr = 7,       // |
    BitwiseXor = 8,      // ^
    BitwiseAnd = 9,      // &
    Equality = 10,       // == != === !==
    Relational = 11,     // < > <= >= in instanceof
    Shift = 12,          // << >> >>>
    Additive = 13,       // + -
    Multiplicative = 14, // * / %
    Exponentiation = 15, // **
    Unary = 16,          // ! ~ + - ++ -- typeof void delete await
    Update = 17,         // ++ -- (postfix)
    Call = 18,           // f() a.b a[b] new f()
    Member = 19,         // . [] ?.
    Primary = 20,        // literals, identifiers, ()
}
```

**Pratt Algorithm**:
```rust
impl<'ast> Parser<'ast> {
    fn parse_expression(&mut self, precedence: Precedence) -> Result<&'ast Expr<'ast>> {
        // Prefix: unary, literals, identifiers, etc.
        let mut left = self.parse_prefix()?;

        // Infix: binary ops, postfix ops, member access, etc.
        while precedence < self.current_precedence() {
            left = self.parse_infix(left)?;
        }

        Ok(left)
    }

    fn parse_prefix(&mut self) -> Result<&'ast Expr<'ast>> {
        match self.current().kind {
            // Literals
            TokenKind::Number => self.parse_number_literal(),
            TokenKind::String => self.parse_string_literal(),
            TokenKind::True | TokenKind::False => self.parse_boolean_literal(),
            TokenKind::Null => self.parse_null_literal(),
            TokenKind::Undefined => self.parse_undefined_literal(),

            // Identifiers
            TokenKind::Identifier => self.parse_identifier(),

            // Unary operators
            TokenKind::Not | TokenKind::Tilde | TokenKind::Plus | TokenKind::Minus |
            TokenKind::PlusPlus | TokenKind::MinusMinus |
            TokenKind::Typeof | TokenKind::Void | TokenKind::Delete => {
                self.parse_unary_expression()
            }

            // Grouping
            TokenKind::LParen => self.parse_parenthesized_expression(),

            // Array literal
            TokenKind::LBracket => self.parse_array_literal(),

            // Object literal
            TokenKind::LBrace => self.parse_object_literal(),

            // Function expression
            TokenKind::Function => self.parse_function_expression(),

            // Arrow function (async)
            TokenKind::Async if self.peek().kind == TokenKind::LParen => {
                self.parse_async_arrow_function()
            }

            // Class expression
            TokenKind::Class => self.parse_class_expression(),

            // Template literal
            TokenKind::TemplateNoSub | TokenKind::TemplateHead => {
                self.parse_template_literal()
            }

            // RegExp literal
            TokenKind::RegExp => self.parse_regexp_literal(),

            // this, super, new.target, import.meta
            TokenKind::This => self.parse_this_expression(),
            TokenKind::Super => self.parse_super_expression(),
            TokenKind::New => self.parse_new_expression(),
            TokenKind::Import => self.parse_import_expression(),

            // await (async functions)
            TokenKind::Await => self.parse_await_expression(),

            // yield (generators)
            TokenKind::Yield => self.parse_yield_expression(),

            _ => Err(ParseError::UnexpectedToken),
        }
    }

    fn parse_infix(&mut self, left: &'ast Expr<'ast>) -> Result<&'ast Expr<'ast>> {
        match self.current().kind {
            // Binary operators
            TokenKind::Plus | TokenKind::Minus | TokenKind::Star | TokenKind::Slash |
            TokenKind::Percent | TokenKind::StarStar |
            TokenKind::LtLt | TokenKind::GtGt | TokenKind::GtGtGt |
            TokenKind::Amp | TokenKind::Pipe | TokenKind::Caret |
            TokenKind::EqEq | TokenKind::NotEq | TokenKind::EqEqEq | TokenKind::NotEqEq |
            TokenKind::Lt | TokenKind::LtEq | TokenKind::Gt | TokenKind::GtEq |
            TokenKind::In | TokenKind::Instanceof => {
                self.parse_binary_expression(left)
            }

            // Logical operators
            TokenKind::AmpAmp | TokenKind::PipePipe | TokenKind::QuestionQuestion => {
                self.parse_logical_expression(left)
            }

            // Assignment operators
            TokenKind::Eq | TokenKind::PlusEq | TokenKind::MinusEq | /* ... */ => {
                self.parse_assignment_expression(left)
            }

            // Conditional (ternary)
            TokenKind::Question => self.parse_conditional_expression(left),

            // Member access
            TokenKind::Dot | TokenKind::QuestionDot => self.parse_member_expression(left),
            TokenKind::LBracket => self.parse_computed_member_expression(left),

            // Function call
            TokenKind::LParen => self.parse_call_expression(left),

            // Postfix increment/decrement
            TokenKind::PlusPlus | TokenKind::MinusMinus => {
                self.parse_update_expression(left)
            }

            // Comma operator
            TokenKind::Comma => self.parse_sequence_expression(left),

            _ => Ok(left),  // No infix operator
        }
    }
}
```

---

## 3. STATEMENT PARSING

### 3.1 Control Flow Edge Cases (Test262 Compliance)

**Critical**: 23 language statement failures from Test262 gap analysis.

**Edge Case 1: try-catch-finally-return**:
```javascript
// Test262: finalizer return overrides try return
function test() {
    try { return 1; }
    finally { return 2; }
}
// Expected: 2
```

**Parser Strategy**:
```rust
fn parse_try_statement(&mut self) -> Result<&'ast Stmt<'ast>> {
    self.expect(TokenKind::Try)?;

    let block = self.parse_block_statement()?;

    let handler = if self.current().kind == TokenKind::Catch {
        Some(self.parse_catch_clause()?)
    } else {
        None
    };

    let finalizer = if self.current().kind == TokenKind::Finally {
        self.advance();
        Some(self.parse_block_statement()?)
    } else {
        None
    };

    // Test262 requirement: either catch or finally must exist
    if handler.is_none() && finalizer.is_none() {
        return Err(ParseError::TryWithoutCatchOrFinally);
    }

    Ok(self.arena.alloc(Stmt::Try {
        block,
        handler,
        finalizer,
        span: Span::merge(block.span, finalizer.map_or(handler.unwrap().span, |f| f.span)),
    }))
}
```

**Edge Case 2: Labeled statement break**:
```javascript
// Test262: labeled break to outer loop
outer: for (let i = 0; i < 10; i++) {
    inner: for (let j = 0; j < 10; j++) {
        break outer;  // Break to outer, not inner
    }
}
```

**Parser Strategy**:
```rust
fn parse_labeled_statement(&mut self) -> Result<&'ast Stmt<'ast>> {
    let label = self.expect_identifier()?;
    self.expect(TokenKind::Colon)?;

    let body = self.parse_statement()?;

    Ok(self.arena.alloc(Stmt::Labeled {
        label: self.arena.alloc_str(label),
        body,
        span: Span::merge(label.span, body.span),
    }))
}

fn parse_break_statement(&mut self) -> Result<&'ast Stmt<'ast>> {
    self.expect(TokenKind::Break)?;

    let label = if self.can_insert_semicolon() {
        None
    } else if self.current().kind == TokenKind::Identifier {
        Some(self.arena.alloc_str(self.expect_identifier()?.lexeme))
    } else {
        None
    };

    self.consume_semicolon()?;

    Ok(self.arena.alloc(Stmt::Break { label, span: /* ... */ }))
}
```

**Edge Case 3: for-await-of**:
```javascript
// Test262: async iterator with throw
for await (let x of asyncIteratorThatThrows) {
    // Should propagate error correctly
}
```

**Parser Strategy**:
```rust
fn parse_for_statement(&mut self) -> Result<&'ast Stmt<'ast>> {
    let is_await = if self.current().kind == TokenKind::Await {
        self.advance();
        true
    } else {
        false
    };

    self.expect(TokenKind::For)?;
    self.expect(TokenKind::LParen)?;

    // Parse for-in/of or traditional for
    // ... (logic to disambiguate)

    if is_for_of {
        let left = self.parse_for_of_left()?;
        let right = self.parse_expression(Precedence::Assignment)?;
        self.expect(TokenKind::RParen)?;
        let body = self.parse_statement()?;

        Ok(self.arena.alloc(Stmt::ForOf {
            left,
            right,
            body,
            is_await,  // Critical for async iteration
            span: /* ... */,
        }))
    } else {
        // Traditional for loop
        // ...
    }
}
```

---

## 4. MODULE PARSING (import/export)

**Module Syntax**:
```javascript
// Named imports
import { foo, bar as baz } from './module.js';

// Default import
import React from 'react';

// Namespace import
import * as utils from './utils.js';

// Side-effect import
import './polyfill.js';

// Named exports
export { foo, bar as baz };
export const x = 42;
export function fn() {}
export default class {}

// Re-export
export { foo } from './other.js';
export * from './all.js';
```

**AST Representation**:
```rust
pub enum ModuleItem<'ast> {
    ImportDeclaration {
        specifiers: &'ast [ImportSpecifier<'ast>],
        source: &'ast str,
        span: Span,
    },
    ExportNamedDeclaration {
        declaration: Option<&'ast Decl<'ast>>,
        specifiers: &'ast [ExportSpecifier<'ast>],
        source: Option<&'ast str>,
        span: Span,
    },
    ExportDefaultDeclaration {
        declaration: ExportDefaultDecl<'ast>,
        span: Span,
    },
    ExportAllDeclaration {
        source: &'ast str,
        exported: Option<&'ast str>,  // export * as name
        span: Span,
    },
    Statement(&'ast Stmt<'ast>),
}

pub enum ImportSpecifier<'ast> {
    Named {
        imported: &'ast str,
        local: &'ast str,
        span: Span,
    },
    Default {
        local: &'ast str,
        span: Span,
    },
    Namespace {
        local: &'ast str,
        span: Span,
    },
}
```

**Top-Level Await**:
```rust
fn parse_module(&mut self) -> Result<&'ast Program<'ast>> {
    let mut body = Vec::new();

    while self.current().kind != TokenKind::Eof {
        let item = self.parse_module_item()?;
        body.push(item);
    }

    // Arena-allocate module items
    let body = self.arena.alloc_slice(&body);

    Ok(self.arena.alloc(Program {
        body,
        source_type: SourceType::Module,
        span: Span::new(0, self.source.len() as u32, 1, 1),
    }))
}
```

---

## 5. ERROR RECOVERY

**Strategy**: Synchronize at statement boundaries

```rust
impl<'ast> Parser<'ast> {
    fn parse_statement(&mut self) -> Result<&'ast Stmt<'ast>> {
        let result = self.try_parse_statement();

        match result {
            Ok(stmt) => Ok(stmt),
            Err(e) => {
                // Log error
                self.errors.push(e);

                // Synchronize at statement boundary
                self.synchronize();

                // Return error statement (parser continues)
                Ok(self.arena.alloc(Stmt::Error { span: e.span }))
            }
        }
    }

    fn synchronize(&mut self) {
        // Skip tokens until we reach a statement boundary
        loop {
            match self.current().kind {
                // Statement starters
                TokenKind::Let | TokenKind::Const | TokenKind::Var |
                TokenKind::Function | TokenKind::Class |
                TokenKind::If | TokenKind::While | TokenKind::For |
                TokenKind::Return | TokenKind::Break | TokenKind::Continue |
                TokenKind::Try | TokenKind::Throw | TokenKind::Switch => break,

                // End of block
                TokenKind::RBrace | TokenKind::Eof => break,

                // Semicolon (statement terminator)
                TokenKind::Semicolon => {
                    self.advance();
                    break;
                }

                _ => { self.advance(); }
            }
        }
    }
}
```

---

## 6. TEST262 COMPLIANCE

**Phase 2-5 Goal**: Fix all 51 language test failures

**Test Integration**:
```rust
#[cfg(test)]
mod tests {
    #[test]
    fn test262_parser_suite() {
        let test_dir = Path::new("test262/test/language");
        let mut passed = 0;
        let mut failed = 0;

        for entry in walk_test_files(test_dir) {
            let source = fs::read_to_string(&entry).unwrap();

            match parse_test262(&source) {
                Ok(_) => passed += 1,
                Err(e) => {
                    failed += 1;
                    eprintln!("FAIL: {:?}: {}", entry, e);
                }
            }
        }

        println!("Test262 parser: {} passed, {} failed", passed, failed);
        assert_eq!(failed, 0);
    }
}
```

---

## 7. DELIVERABLES

**Phase 4-5** (14 days):
1. ✅ Complete Parser<'ast> implementation
2. ✅ All 60+ expression types
3. ✅ All 20+ statement types
4. ✅ Module parsing (import/export)
5. ✅ Error recovery (continue on errors)
6. ✅ Test262 language tests: 100% (51 failures fixed)
7. ✅ Performance: >20,000 LOC/s

---

**Next**: BYTECODE-ARCHITECTURE.md (VM instruction set design)
