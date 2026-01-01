//! AST (Abstract Syntax Tree) node definitions for JavaScript
//!
//! Zero-copy design: AST nodes reference the source text via Span.
//! Arena-allocated: All nodes stored in bump arena for bulk deallocation.
//!
//! Based on ESTree spec (https://github.com/estree/estree) with simplifications.

use crate::lexer::{Span, Symbol};
use super::ast_arena::{AstBox, AstVec};

/// Root of a JavaScript program
#[derive(Debug, Clone)]
pub struct Program<'src, 'arena> {
    /// Program body (statements)
    pub body: AstVec<'arena, Statement<'src, 'arena>>,
    /// Source type (script or module)
    pub source_type: SourceType,
    /// Span covering entire program
    pub span: Span,
}

/// Source type for the program
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceType {
    Script,
    Module,
}

/// A JavaScript statement
#[derive(Debug, Clone)]
pub enum Statement<'src, 'arena> {
    /// Variable declaration: var/let/const
    VariableDeclaration(VariableDeclaration<'src, 'arena>),
    /// Expression statement: expr;
    Expression(ExpressionStatement<'src, 'arena>),
    /// Block statement: { ... }
    Block(BlockStatement<'src, 'arena>),
    /// If statement: if (cond) {...} else {...}
    If(IfStatement<'src, 'arena>),
    /// While statement: while (cond) {...}
    While(WhileStatement<'src, 'arena>),
    /// Do-while statement: do {...} while (cond)
    DoWhile(DoWhileStatement<'src, 'arena>),
    /// For statement: for (init; test; update) {...}
    For(ForStatement<'src, 'arena>),
    /// For-in statement: for (x in obj) {...}
    ForIn(ForInStatement<'src, 'arena>),
    /// For-of statement: for (x of iterable) {...}
    ForOf(ForOfStatement<'src, 'arena>),
    /// Function declaration: function name() {...}
    FunctionDeclaration(FunctionDeclaration<'src, 'arena>),
    /// Return statement: return expr;
    Return(ReturnStatement<'src, 'arena>),
    /// Break statement: break label;
    Break(BreakStatement<'src>),
    /// Continue statement: continue label;
    Continue(ContinueStatement<'src>),
    /// Throw statement: throw expr;
    Throw(ThrowStatement<'src, 'arena>),
    /// Try statement: try {...} catch {...} finally {...}
    Try(TryStatement<'src, 'arena>),
    /// Switch statement: switch (expr) {...}
    Switch(SwitchStatement<'src, 'arena>),
    /// Labeled statement: label: stmt
    Labeled(LabeledStatement<'src, 'arena>),
    /// With statement: with (obj) {...}
    With(WithStatement<'src, 'arena>),
    /// Debugger statement: debugger;
    Debugger(Span),
    /// Empty statement: ;
    Empty(Span),
    /// Class declaration: class Name {...}
    ClassDeclaration(ClassDeclaration<'src, 'arena>),
    /// Import declaration: import ... from '...'
    Import(ImportDeclaration<'src, 'arena>),
    /// Export declaration: export ...
    Export(ExportDeclaration<'src, 'arena>),
}

/// Variable declaration: var/let/const x = expr;
#[derive(Debug, Clone)]
pub struct VariableDeclaration<'src, 'arena> {
    pub kind: VariableKind,
    pub declarations: AstVec<'arena, VariableDeclarator<'src, 'arena>>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VariableKind {
    Var,
    Let,
    Const,
}

#[derive(Debug, Clone)]
pub struct VariableDeclarator<'src, 'arena> {
    pub id: Pattern<'src, 'arena>,
    pub init: Option<AstBox<'arena, Expression<'src, 'arena>>>,
    pub span: Span,
}

/// Expression statement: expr;
#[derive(Debug, Clone)]
pub struct ExpressionStatement<'src, 'arena> {
    pub expression: AstBox<'arena, Expression<'src, 'arena>>,
    pub span: Span,
}

/// Block statement: { statements }
#[derive(Debug, Clone)]
pub struct BlockStatement<'src, 'arena> {
    pub body: AstVec<'arena, Statement<'src, 'arena>>,
    pub span: Span,
}

/// If statement
#[derive(Debug, Clone)]
pub struct IfStatement<'src, 'arena> {
    pub test: AstBox<'arena, Expression<'src, 'arena>>,
    pub consequent: AstBox<'arena, Statement<'src, 'arena>>,
    pub alternate: Option<AstBox<'arena, Statement<'src, 'arena>>>,
    pub span: Span,
}

/// While statement
#[derive(Debug, Clone)]
pub struct WhileStatement<'src, 'arena> {
    pub test: AstBox<'arena, Expression<'src, 'arena>>,
    pub body: AstBox<'arena, Statement<'src, 'arena>>,
    pub span: Span,
}

/// Do-while statement
#[derive(Debug, Clone)]
pub struct DoWhileStatement<'src, 'arena> {
    pub body: AstBox<'arena, Statement<'src, 'arena>>,
    pub test: AstBox<'arena, Expression<'src, 'arena>>,
    pub span: Span,
}

/// For statement
#[derive(Debug, Clone)]
pub struct ForStatement<'src, 'arena> {
    pub init: Option<ForInit<'src, 'arena>>,
    pub test: Option<AstBox<'arena, Expression<'src, 'arena>>>,
    pub update: Option<AstBox<'arena, Expression<'src, 'arena>>>,
    pub body: AstBox<'arena, Statement<'src, 'arena>>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ForInit<'src, 'arena> {
    VariableDeclaration(VariableDeclaration<'src, 'arena>),
    Expression(AstBox<'arena, Expression<'src, 'arena>>),
}

/// For-in statement
#[derive(Debug, Clone)]
pub struct ForInStatement<'src, 'arena> {
    pub left: ForInLeft<'src, 'arena>,
    pub right: AstBox<'arena, Expression<'src, 'arena>>,
    pub body: AstBox<'arena, Statement<'src, 'arena>>,
    pub span: Span,
}

/// For-of statement
#[derive(Debug, Clone)]
pub struct ForOfStatement<'src, 'arena> {
    pub left: ForInLeft<'src, 'arena>,
    pub right: AstBox<'arena, Expression<'src, 'arena>>,
    pub body: AstBox<'arena, Statement<'src, 'arena>>,
    pub is_await: bool,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ForInLeft<'src, 'arena> {
    VariableDeclaration(VariableDeclaration<'src, 'arena>),
    Pattern(Pattern<'src, 'arena>),
}

/// Function declaration
#[derive(Debug, Clone)]
pub struct FunctionDeclaration<'src, 'arena> {
    pub id: Option<Identifier<'src>>,
    pub params: AstVec<'arena, Pattern<'src, 'arena>>,
    pub body: BlockStatement<'src, 'arena>,
    pub is_async: bool,
    pub is_generator: bool,
    pub span: Span,
}

/// Return statement
#[derive(Debug, Clone)]
pub struct ReturnStatement<'src, 'arena> {
    pub argument: Option<AstBox<'arena, Expression<'src, 'arena>>>,
    pub span: Span,
}

/// Break statement
#[derive(Debug, Clone)]
pub struct BreakStatement<'src> {
    pub label: Option<Identifier<'src>>,
    pub span: Span,
}

/// Continue statement
#[derive(Debug, Clone)]
pub struct ContinueStatement<'src> {
    pub label: Option<Identifier<'src>>,
    pub span: Span,
}

/// Throw statement
#[derive(Debug, Clone)]
pub struct ThrowStatement<'src, 'arena> {
    pub argument: AstBox<'arena, Expression<'src, 'arena>>,
    pub span: Span,
}

/// Try statement
#[derive(Debug, Clone)]
pub struct TryStatement<'src, 'arena> {
    pub block: BlockStatement<'src, 'arena>,
    pub handler: Option<CatchClause<'src, 'arena>>,
    pub finalizer: Option<BlockStatement<'src, 'arena>>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct CatchClause<'src, 'arena> {
    pub param: Option<Pattern<'src, 'arena>>,
    pub body: BlockStatement<'src, 'arena>,
    pub span: Span,
}

/// Switch statement
#[derive(Debug, Clone)]
pub struct SwitchStatement<'src, 'arena> {
    pub discriminant: AstBox<'arena, Expression<'src, 'arena>>,
    pub cases: AstVec<'arena, SwitchCase<'src, 'arena>>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct SwitchCase<'src, 'arena> {
    pub test: Option<AstBox<'arena, Expression<'src, 'arena>>>, // None = default case
    pub consequent: AstVec<'arena, Statement<'src, 'arena>>,
    pub span: Span,
}

/// Labeled statement
#[derive(Debug, Clone)]
pub struct LabeledStatement<'src, 'arena> {
    pub label: Identifier<'src>,
    pub body: AstBox<'arena, Statement<'src, 'arena>>,
    pub span: Span,
}

/// With statement
#[derive(Debug, Clone)]
pub struct WithStatement<'src, 'arena> {
    pub object: AstBox<'arena, Expression<'src, 'arena>>,
    pub body: AstBox<'arena, Statement<'src, 'arena>>,
    pub span: Span,
}

/// Class declaration
#[derive(Debug, Clone)]
pub struct ClassDeclaration<'src, 'arena> {
    pub id: Option<Identifier<'src>>,
    pub super_class: Option<AstBox<'arena, Expression<'src, 'arena>>>,
    pub body: ClassBody<'src, 'arena>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ClassBody<'src, 'arena> {
    pub body: AstVec<'arena, ClassElement<'src, 'arena>>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ClassElement<'src, 'arena> {
    MethodDefinition(MethodDefinition<'src, 'arena>),
    PropertyDefinition(PropertyDefinition<'src, 'arena>),
    StaticBlock(BlockStatement<'src, 'arena>),
}

#[derive(Debug, Clone)]
pub struct MethodDefinition<'src, 'arena> {
    pub key: PropertyKey<'src, 'arena>,
    pub value: FunctionExpression<'src, 'arena>,
    pub kind: MethodKind,
    pub computed: bool,
    pub is_static: bool,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MethodKind {
    Method,
    Get,
    Set,
    Constructor,
}

#[derive(Debug, Clone)]
pub struct PropertyDefinition<'src, 'arena> {
    pub key: PropertyKey<'src, 'arena>,
    pub value: Option<AstBox<'arena, Expression<'src, 'arena>>>,
    pub computed: bool,
    pub is_static: bool,
    pub span: Span,
}

/// Import declaration
#[derive(Debug, Clone)]
pub struct ImportDeclaration<'src, 'arena> {
    pub specifiers: AstVec<'arena, ImportSpecifier<'src>>,
    pub source: StringLiteral<'src>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ImportSpecifier<'src> {
    /// import x from 'module'
    Default(Identifier<'src>),
    /// import * as x from 'module'
    Namespace(Identifier<'src>),
    /// import { x } from 'module'
    Named {
        imported: Identifier<'src>,
        local: Identifier<'src>,
    },
}

/// Export declaration
#[derive(Debug, Clone)]
pub enum ExportDeclaration<'src, 'arena> {
    /// export { x }
    Named {
        specifiers: AstVec<'arena, ExportSpecifier<'src>>,
        source: Option<StringLiteral<'src>>,
        span: Span,
    },
    /// export default expr
    Default {
        declaration: AstBox<'arena, Expression<'src, 'arena>>,
        span: Span,
    },
    /// export * from 'module'
    All {
        source: StringLiteral<'src>,
        exported: Option<Identifier<'src>>,
        span: Span,
    },
    /// export var/function/class
    Declaration {
        declaration: AstBox<'arena, Statement<'src, 'arena>>,
        span: Span,
    },
}

#[derive(Debug, Clone)]
pub struct ExportSpecifier<'src> {
    pub local: Identifier<'src>,
    pub exported: Identifier<'src>,
    pub span: Span,
}

// ============================================================================
// EXPRESSIONS
// ============================================================================

/// A JavaScript expression
#[derive(Debug, Clone)]
pub enum Expression<'src, 'arena> {
    /// Identifier: foo
    Identifier(Identifier<'src>),
    /// Literal: 42, "hello", true, null
    Literal(Literal<'src>),
    /// This expression: this
    This(Span),
    /// Super expression: super
    Super(Span),
    /// Array literal: [1, 2, 3]
    Array(ArrayExpression<'src, 'arena>),
    /// Object literal: { a: 1, b: 2 }
    Object(ObjectExpression<'src, 'arena>),
    /// Function expression: function() {}
    Function(FunctionExpression<'src, 'arena>),
    /// Arrow function: () => {}
    Arrow(ArrowFunctionExpression<'src, 'arena>),
    /// Class expression: class {}
    Class(ClassExpression<'src, 'arena>),
    /// Template literal: `hello ${name}`
    TemplateLiteral(TemplateLiteral<'src, 'arena>),
    /// Tagged template: tag`hello`
    TaggedTemplate(TaggedTemplateExpression<'src, 'arena>),
    /// Member expression: obj.prop or obj[prop]
    Member(MemberExpression<'src, 'arena>),
    /// Call expression: fn(args)
    Call(CallExpression<'src, 'arena>),
    /// New expression: new Foo()
    New(NewExpression<'src, 'arena>),
    /// Unary expression: !x, -x, typeof x
    Unary(UnaryExpression<'src, 'arena>),
    /// Update expression: x++, --x
    Update(UpdateExpression<'src, 'arena>),
    /// Binary expression: x + y
    Binary(BinaryExpression<'src, 'arena>),
    /// Logical expression: x && y
    Logical(LogicalExpression<'src, 'arena>),
    /// Conditional expression: x ? y : z
    Conditional(ConditionalExpression<'src, 'arena>),
    /// Assignment expression: x = y
    Assignment(AssignmentExpression<'src, 'arena>),
    /// Sequence expression: x, y, z
    Sequence(SequenceExpression<'src, 'arena>),
    /// Yield expression: yield x
    Yield(YieldExpression<'src, 'arena>),
    /// Await expression: await x
    Await(AwaitExpression<'src, 'arena>),
    /// Import expression: import('module')
    Import(ImportExpression<'src, 'arena>),
    /// Meta property: new.target, import.meta
    MetaProperty(MetaProperty<'src>),
    /// Spread element: ...x
    Spread(SpreadElement<'src, 'arena>),
    /// Parenthesized: (x)
    Parenthesized(ParenthesizedExpression<'src, 'arena>),
}

/// Identifier
#[derive(Debug, Clone)]
pub struct Identifier<'src> {
    pub name: Symbol,
    pub raw: &'src str,
    pub span: Span,
}

/// Literal value
#[derive(Debug, Clone)]
pub enum Literal<'src> {
    Null(Span),
    Boolean(BooleanLiteral),
    Number(NumberLiteral<'src>),
    String(StringLiteral<'src>),
    RegExp(RegExpLiteral<'src>),
    BigInt(BigIntLiteral<'src>),
}

#[derive(Debug, Clone)]
pub struct BooleanLiteral {
    pub value: bool,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct NumberLiteral<'src> {
    pub value: f64,
    pub raw: &'src str,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct StringLiteral<'src> {
    pub value: &'src str,
    pub raw: &'src str,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct RegExpLiteral<'src> {
    pub pattern: &'src str,
    pub flags: &'src str,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct BigIntLiteral<'src> {
    pub raw: &'src str,
    pub span: Span,
}

/// Array expression
#[derive(Debug, Clone)]
pub struct ArrayExpression<'src, 'arena> {
    pub elements: AstVec<'arena, ArrayElement<'src, 'arena>>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ArrayElement<'src, 'arena> {
    Expression(Expression<'src, 'arena>),
    Spread(SpreadElement<'src, 'arena>),
    Hole, // elision: [1,,3]
}

/// Object expression
#[derive(Debug, Clone)]
pub struct ObjectExpression<'src, 'arena> {
    pub properties: AstVec<'arena, ObjectProperty<'src, 'arena>>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ObjectProperty<'src, 'arena> {
    Property(Property<'src, 'arena>),
    SpreadProperty(SpreadElement<'src, 'arena>),
}

#[derive(Debug, Clone)]
pub struct Property<'src, 'arena> {
    pub key: PropertyKey<'src, 'arena>,
    pub value: AstBox<'arena, Expression<'src, 'arena>>,
    pub kind: PropertyKind,
    pub method: bool,
    pub shorthand: bool,
    pub computed: bool,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum PropertyKey<'src, 'arena> {
    Identifier(Identifier<'src>),
    Literal(Literal<'src>),
    Computed(AstBox<'arena, Expression<'src, 'arena>>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropertyKind {
    Init,
    Get,
    Set,
}

/// Function expression
#[derive(Debug, Clone)]
pub struct FunctionExpression<'src, 'arena> {
    pub id: Option<Identifier<'src>>,
    pub params: AstVec<'arena, Pattern<'src, 'arena>>,
    pub body: BlockStatement<'src, 'arena>,
    pub is_async: bool,
    pub is_generator: bool,
    pub span: Span,
}

/// Arrow function expression
#[derive(Debug, Clone)]
pub struct ArrowFunctionExpression<'src, 'arena> {
    pub params: AstVec<'arena, Pattern<'src, 'arena>>,
    pub body: ArrowBody<'src, 'arena>,
    pub is_async: bool,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ArrowBody<'src, 'arena> {
    Expression(AstBox<'arena, Expression<'src, 'arena>>),
    Block(BlockStatement<'src, 'arena>),
}

/// Class expression
#[derive(Debug, Clone)]
pub struct ClassExpression<'src, 'arena> {
    pub id: Option<Identifier<'src>>,
    pub super_class: Option<AstBox<'arena, Expression<'src, 'arena>>>,
    pub body: ClassBody<'src, 'arena>,
    pub span: Span,
}

/// Template literal
#[derive(Debug, Clone)]
pub struct TemplateLiteral<'src, 'arena> {
    pub quasis: AstVec<'arena, TemplateElement<'src>>,
    pub expressions: AstVec<'arena, Expression<'src, 'arena>>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct TemplateElement<'src> {
    pub raw: &'src str,
    pub cooked: Option<&'src str>,
    pub tail: bool,
    pub span: Span,
}

/// Tagged template expression
#[derive(Debug, Clone)]
pub struct TaggedTemplateExpression<'src, 'arena> {
    pub tag: AstBox<'arena, Expression<'src, 'arena>>,
    pub quasi: TemplateLiteral<'src, 'arena>,
    pub span: Span,
}

/// Member expression
#[derive(Debug, Clone)]
pub struct MemberExpression<'src, 'arena> {
    pub object: AstBox<'arena, Expression<'src, 'arena>>,
    pub property: AstBox<'arena, Expression<'src, 'arena>>,
    pub computed: bool,
    pub optional: bool, // ?.
    pub span: Span,
}

/// Call expression
#[derive(Debug, Clone)]
pub struct CallExpression<'src, 'arena> {
    pub callee: AstBox<'arena, Expression<'src, 'arena>>,
    pub arguments: AstVec<'arena, Argument<'src, 'arena>>,
    pub optional: bool, // ?.()
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum Argument<'src, 'arena> {
    Expression(Expression<'src, 'arena>),
    Spread(SpreadElement<'src, 'arena>),
}

/// New expression
#[derive(Debug, Clone)]
pub struct NewExpression<'src, 'arena> {
    pub callee: AstBox<'arena, Expression<'src, 'arena>>,
    pub arguments: AstVec<'arena, Argument<'src, 'arena>>,
    pub span: Span,
}

/// Unary expression
#[derive(Debug, Clone)]
pub struct UnaryExpression<'src, 'arena> {
    pub operator: UnaryOperator,
    pub argument: AstBox<'arena, Expression<'src, 'arena>>,
    pub prefix: bool,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOperator {
    Minus,      // -
    Plus,       // +
    Not,        // !
    BitwiseNot, // ~
    Typeof,     // typeof
    Void,       // void
    Delete,     // delete
}

/// Update expression
#[derive(Debug, Clone)]
pub struct UpdateExpression<'src, 'arena> {
    pub operator: UpdateOperator,
    pub argument: AstBox<'arena, Expression<'src, 'arena>>,
    pub prefix: bool,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateOperator {
    Increment, // ++
    Decrement, // --
}

/// Binary expression
#[derive(Debug, Clone)]
pub struct BinaryExpression<'src, 'arena> {
    pub operator: BinaryOperator,
    pub left: AstBox<'arena, Expression<'src, 'arena>>,
    pub right: AstBox<'arena, Expression<'src, 'arena>>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOperator {
    // Arithmetic
    Add,    // +
    Sub,    // -
    Mul,    // *
    Div,    // /
    Mod,    // %
    Pow,    // **
    // Comparison
    Eq,     // ==
    Ne,     // !=
    StrictEq,  // ===
    StrictNe,  // !==
    Lt,     // <
    Le,     // <=
    Gt,     // >
    Ge,     // >=
    // Bitwise
    BitwiseOr,  // |
    BitwiseXor, // ^
    BitwiseAnd, // &
    ShiftLeft,  // <<
    ShiftRight, // >>
    UnsignedShiftRight, // >>>
    // Other
    In,         // in
    InstanceOf, // instanceof
}

/// Logical expression
#[derive(Debug, Clone)]
pub struct LogicalExpression<'src, 'arena> {
    pub operator: LogicalOperator,
    pub left: AstBox<'arena, Expression<'src, 'arena>>,
    pub right: AstBox<'arena, Expression<'src, 'arena>>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogicalOperator {
    And,       // &&
    Or,        // ||
    NullishCoalescing, // ??
}

/// Conditional expression
#[derive(Debug, Clone)]
pub struct ConditionalExpression<'src, 'arena> {
    pub test: AstBox<'arena, Expression<'src, 'arena>>,
    pub consequent: AstBox<'arena, Expression<'src, 'arena>>,
    pub alternate: AstBox<'arena, Expression<'src, 'arena>>,
    pub span: Span,
}

/// Assignment expression
#[derive(Debug, Clone)]
pub struct AssignmentExpression<'src, 'arena> {
    pub operator: AssignmentOperator,
    pub left: AssignmentTarget<'src, 'arena>,
    pub right: AstBox<'arena, Expression<'src, 'arena>>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum AssignmentTarget<'src, 'arena> {
    Identifier(Identifier<'src>),
    Member(MemberExpression<'src, 'arena>),
    Pattern(Pattern<'src, 'arena>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignmentOperator {
    Assign,         // =
    AddAssign,      // +=
    SubAssign,      // -=
    MulAssign,      // *=
    DivAssign,      // /=
    ModAssign,      // %=
    PowAssign,      // **=
    ShiftLeftAssign,   // <<=
    ShiftRightAssign,  // >>=
    UnsignedShiftRightAssign, // >>>=
    BitwiseOrAssign,   // |=
    BitwiseXorAssign,  // ^=
    BitwiseAndAssign,  // &=
    LogicalOrAssign,   // ||=
    LogicalAndAssign,  // &&=
    NullishAssign,     // ??=
}

/// Sequence expression
#[derive(Debug, Clone)]
pub struct SequenceExpression<'src, 'arena> {
    pub expressions: AstVec<'arena, Expression<'src, 'arena>>,
    pub span: Span,
}

/// Yield expression
#[derive(Debug, Clone)]
pub struct YieldExpression<'src, 'arena> {
    pub argument: Option<AstBox<'arena, Expression<'src, 'arena>>>,
    pub delegate: bool, // yield*
    pub span: Span,
}

/// Await expression
#[derive(Debug, Clone)]
pub struct AwaitExpression<'src, 'arena> {
    pub argument: AstBox<'arena, Expression<'src, 'arena>>,
    pub span: Span,
}

/// Import expression (dynamic import)
#[derive(Debug, Clone)]
pub struct ImportExpression<'src, 'arena> {
    pub source: AstBox<'arena, Expression<'src, 'arena>>,
    pub span: Span,
}

/// Meta property (new.target, import.meta)
#[derive(Debug, Clone)]
pub struct MetaProperty<'src> {
    pub meta: Identifier<'src>,
    pub property: Identifier<'src>,
    pub span: Span,
}

/// Spread element
#[derive(Debug, Clone)]
pub struct SpreadElement<'src, 'arena> {
    pub argument: AstBox<'arena, Expression<'src, 'arena>>,
    pub span: Span,
}

/// Parenthesized expression
#[derive(Debug, Clone)]
pub struct ParenthesizedExpression<'src, 'arena> {
    pub expression: AstBox<'arena, Expression<'src, 'arena>>,
    pub span: Span,
}

// ============================================================================
// PATTERNS (for destructuring)
// ============================================================================

/// A pattern (used in variable declarations, function params, assignment)
#[derive(Debug, Clone)]
pub enum Pattern<'src, 'arena> {
    /// Simple identifier: x
    Identifier(Identifier<'src>),
    /// Array destructuring: [a, b]
    Array(ArrayPattern<'src, 'arena>),
    /// Object destructuring: { a, b }
    Object(ObjectPattern<'src, 'arena>),
    /// Rest element: ...x
    Rest(RestElement<'src, 'arena>),
    /// Assignment pattern: x = default
    Assignment(AssignmentPattern<'src, 'arena>),
}

#[derive(Debug, Clone)]
pub struct ArrayPattern<'src, 'arena> {
    pub elements: AstVec<'arena, Option<Pattern<'src, 'arena>>>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ObjectPattern<'src, 'arena> {
    pub properties: AstVec<'arena, ObjectPatternProperty<'src, 'arena>>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ObjectPatternProperty<'src, 'arena> {
    Property {
        key: PropertyKey<'src, 'arena>,
        value: Pattern<'src, 'arena>,
        shorthand: bool,
        computed: bool,
        span: Span,
    },
    Rest(RestElement<'src, 'arena>),
}

#[derive(Debug, Clone)]
pub struct RestElement<'src, 'arena> {
    pub argument: AstBox<'arena, Pattern<'src, 'arena>>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct AssignmentPattern<'src, 'arena> {
    pub left: AstBox<'arena, Pattern<'src, 'arena>>,
    pub right: AstBox<'arena, Expression<'src, 'arena>>,
    pub span: Span,
}

// ============================================================================
// IMPL BLOCKS
// ============================================================================

impl<'src, 'arena> Statement<'src, 'arena> {
    /// Get the span of this statement
    pub fn span(&self) -> Span {
        match self {
            Statement::VariableDeclaration(s) => s.span,
            Statement::Expression(s) => s.span,
            Statement::Block(s) => s.span,
            Statement::If(s) => s.span,
            Statement::While(s) => s.span,
            Statement::DoWhile(s) => s.span,
            Statement::For(s) => s.span,
            Statement::ForIn(s) => s.span,
            Statement::ForOf(s) => s.span,
            Statement::FunctionDeclaration(s) => s.span,
            Statement::Return(s) => s.span,
            Statement::Break(s) => s.span,
            Statement::Continue(s) => s.span,
            Statement::Throw(s) => s.span,
            Statement::Try(s) => s.span,
            Statement::Switch(s) => s.span,
            Statement::Labeled(s) => s.span,
            Statement::With(s) => s.span,
            Statement::Debugger(span) => *span,
            Statement::Empty(span) => *span,
            Statement::ClassDeclaration(s) => s.span,
            Statement::Import(s) => s.span,
            Statement::Export(s) => s.span(),
        }
    }
}

impl<'src, 'arena> ExportDeclaration<'src, 'arena> {
    pub fn span(&self) -> Span {
        match self {
            ExportDeclaration::Named { span, .. } => *span,
            ExportDeclaration::Default { span, .. } => *span,
            ExportDeclaration::All { span, .. } => *span,
            ExportDeclaration::Declaration { span, .. } => *span,
        }
    }
}

impl<'src, 'arena> Expression<'src, 'arena> {
    /// Get the span of this expression
    pub fn span(&self) -> Span {
        match self {
            Expression::Identifier(e) => e.span,
            Expression::Literal(e) => e.span(),
            Expression::This(span) => *span,
            Expression::Super(span) => *span,
            Expression::Array(e) => e.span,
            Expression::Object(e) => e.span,
            Expression::Function(e) => e.span,
            Expression::Arrow(e) => e.span,
            Expression::Class(e) => e.span,
            Expression::TemplateLiteral(e) => e.span,
            Expression::TaggedTemplate(e) => e.span,
            Expression::Member(e) => e.span,
            Expression::Call(e) => e.span,
            Expression::New(e) => e.span,
            Expression::Unary(e) => e.span,
            Expression::Update(e) => e.span,
            Expression::Binary(e) => e.span,
            Expression::Logical(e) => e.span,
            Expression::Conditional(e) => e.span,
            Expression::Assignment(e) => e.span,
            Expression::Sequence(e) => e.span,
            Expression::Yield(e) => e.span,
            Expression::Await(e) => e.span,
            Expression::Import(e) => e.span,
            Expression::MetaProperty(e) => e.span,
            Expression::Spread(e) => e.span,
            Expression::Parenthesized(e) => e.span,
        }
    }
}

impl<'src, 'arena> Literal<'src> {
    pub fn span(&self) -> Span {
        match self {
            Literal::Null(span) => *span,
            Literal::Boolean(b) => b.span,
            Literal::Number(n) => n.span,
            Literal::String(s) => s.span,
            Literal::RegExp(r) => r.span,
            Literal::BigInt(b) => b.span,
        }
    }
}

impl<'src, 'arena> Pattern<'src, 'arena> {
    pub fn span(&self) -> Span {
        match self {
            Pattern::Identifier(i) => i.span,
            Pattern::Array(a) => a.span,
            Pattern::Object(o) => o.span,
            Pattern::Rest(r) => r.span(),
            Pattern::Assignment(a) => a.span,
        }
    }
}

impl<'src, 'arena> RestElement<'src, 'arena> {
    pub fn span(&self) -> Span {
        // Span from ... to end of argument
        Span::new(
            self.argument.span().start.saturating_sub(3),
            self.argument.span().end,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_program_creation() {
        let program: Program<'_, '_> = Program {
            body: &[],
            source_type: SourceType::Script,
            span: Span::new(0, 0),
        };
        assert_eq!(program.body.len(), 0);
        assert_eq!(program.source_type, SourceType::Script);
    }

    #[test]
    fn test_expression_span() {
        let expr = Expression::This(Span::new(0, 4));
        assert_eq!(expr.span(), Span::new(0, 4));
    }
}
