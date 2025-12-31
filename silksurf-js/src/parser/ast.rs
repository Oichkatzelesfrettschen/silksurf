//! AST (Abstract Syntax Tree) node definitions for JavaScript
//!
//! Zero-copy design: AST nodes reference the source text via Span.
//! Arena-allocated: All nodes stored in bump arena for bulk deallocation.
//!
//! Based on ESTree spec (https://github.com/estree/estree) with simplifications.

use crate::lexer::{Span, Symbol};

/// Root of a JavaScript program
#[derive(Debug, Clone)]
pub struct Program<'src> {
    /// Program body (statements)
    pub body: Vec<Statement<'src>>,
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
pub enum Statement<'src> {
    /// Variable declaration: var/let/const
    VariableDeclaration(VariableDeclaration<'src>),
    /// Expression statement: expr;
    Expression(ExpressionStatement<'src>),
    /// Block statement: { ... }
    Block(BlockStatement<'src>),
    /// If statement: if (cond) {...} else {...}
    If(IfStatement<'src>),
    /// While statement: while (cond) {...}
    While(WhileStatement<'src>),
    /// Do-while statement: do {...} while (cond)
    DoWhile(DoWhileStatement<'src>),
    /// For statement: for (init; test; update) {...}
    For(ForStatement<'src>),
    /// For-in statement: for (x in obj) {...}
    ForIn(ForInStatement<'src>),
    /// For-of statement: for (x of iterable) {...}
    ForOf(ForOfStatement<'src>),
    /// Function declaration: function name() {...}
    FunctionDeclaration(FunctionDeclaration<'src>),
    /// Return statement: return expr;
    Return(ReturnStatement<'src>),
    /// Break statement: break label;
    Break(BreakStatement<'src>),
    /// Continue statement: continue label;
    Continue(ContinueStatement<'src>),
    /// Throw statement: throw expr;
    Throw(ThrowStatement<'src>),
    /// Try statement: try {...} catch {...} finally {...}
    Try(TryStatement<'src>),
    /// Switch statement: switch (expr) {...}
    Switch(SwitchStatement<'src>),
    /// Labeled statement: label: stmt
    Labeled(LabeledStatement<'src>),
    /// With statement: with (obj) {...}
    With(WithStatement<'src>),
    /// Debugger statement: debugger;
    Debugger(Span),
    /// Empty statement: ;
    Empty(Span),
    /// Class declaration: class Name {...}
    ClassDeclaration(ClassDeclaration<'src>),
    /// Import declaration: import ... from '...'
    Import(ImportDeclaration<'src>),
    /// Export declaration: export ...
    Export(ExportDeclaration<'src>),
}

/// Variable declaration: var/let/const x = expr;
#[derive(Debug, Clone)]
pub struct VariableDeclaration<'src> {
    pub kind: VariableKind,
    pub declarations: Vec<VariableDeclarator<'src>>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VariableKind {
    Var,
    Let,
    Const,
}

#[derive(Debug, Clone)]
pub struct VariableDeclarator<'src> {
    pub id: Pattern<'src>,
    pub init: Option<Box<Expression<'src>>>,
    pub span: Span,
}

/// Expression statement: expr;
#[derive(Debug, Clone)]
pub struct ExpressionStatement<'src> {
    pub expression: Box<Expression<'src>>,
    pub span: Span,
}

/// Block statement: { statements }
#[derive(Debug, Clone)]
pub struct BlockStatement<'src> {
    pub body: Vec<Statement<'src>>,
    pub span: Span,
}

/// If statement
#[derive(Debug, Clone)]
pub struct IfStatement<'src> {
    pub test: Box<Expression<'src>>,
    pub consequent: Box<Statement<'src>>,
    pub alternate: Option<Box<Statement<'src>>>,
    pub span: Span,
}

/// While statement
#[derive(Debug, Clone)]
pub struct WhileStatement<'src> {
    pub test: Box<Expression<'src>>,
    pub body: Box<Statement<'src>>,
    pub span: Span,
}

/// Do-while statement
#[derive(Debug, Clone)]
pub struct DoWhileStatement<'src> {
    pub body: Box<Statement<'src>>,
    pub test: Box<Expression<'src>>,
    pub span: Span,
}

/// For statement
#[derive(Debug, Clone)]
pub struct ForStatement<'src> {
    pub init: Option<ForInit<'src>>,
    pub test: Option<Box<Expression<'src>>>,
    pub update: Option<Box<Expression<'src>>>,
    pub body: Box<Statement<'src>>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ForInit<'src> {
    VariableDeclaration(VariableDeclaration<'src>),
    Expression(Box<Expression<'src>>),
}

/// For-in statement
#[derive(Debug, Clone)]
pub struct ForInStatement<'src> {
    pub left: ForInLeft<'src>,
    pub right: Box<Expression<'src>>,
    pub body: Box<Statement<'src>>,
    pub span: Span,
}

/// For-of statement
#[derive(Debug, Clone)]
pub struct ForOfStatement<'src> {
    pub left: ForInLeft<'src>,
    pub right: Box<Expression<'src>>,
    pub body: Box<Statement<'src>>,
    pub is_await: bool,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ForInLeft<'src> {
    VariableDeclaration(VariableDeclaration<'src>),
    Pattern(Pattern<'src>),
}

/// Function declaration
#[derive(Debug, Clone)]
pub struct FunctionDeclaration<'src> {
    pub id: Option<Identifier<'src>>,
    pub params: Vec<Pattern<'src>>,
    pub body: BlockStatement<'src>,
    pub is_async: bool,
    pub is_generator: bool,
    pub span: Span,
}

/// Return statement
#[derive(Debug, Clone)]
pub struct ReturnStatement<'src> {
    pub argument: Option<Box<Expression<'src>>>,
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
pub struct ThrowStatement<'src> {
    pub argument: Box<Expression<'src>>,
    pub span: Span,
}

/// Try statement
#[derive(Debug, Clone)]
pub struct TryStatement<'src> {
    pub block: BlockStatement<'src>,
    pub handler: Option<CatchClause<'src>>,
    pub finalizer: Option<BlockStatement<'src>>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct CatchClause<'src> {
    pub param: Option<Pattern<'src>>,
    pub body: BlockStatement<'src>,
    pub span: Span,
}

/// Switch statement
#[derive(Debug, Clone)]
pub struct SwitchStatement<'src> {
    pub discriminant: Box<Expression<'src>>,
    pub cases: Vec<SwitchCase<'src>>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct SwitchCase<'src> {
    pub test: Option<Box<Expression<'src>>>, // None = default case
    pub consequent: Vec<Statement<'src>>,
    pub span: Span,
}

/// Labeled statement
#[derive(Debug, Clone)]
pub struct LabeledStatement<'src> {
    pub label: Identifier<'src>,
    pub body: Box<Statement<'src>>,
    pub span: Span,
}

/// With statement
#[derive(Debug, Clone)]
pub struct WithStatement<'src> {
    pub object: Box<Expression<'src>>,
    pub body: Box<Statement<'src>>,
    pub span: Span,
}

/// Class declaration
#[derive(Debug, Clone)]
pub struct ClassDeclaration<'src> {
    pub id: Option<Identifier<'src>>,
    pub super_class: Option<Box<Expression<'src>>>,
    pub body: ClassBody<'src>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ClassBody<'src> {
    pub body: Vec<ClassElement<'src>>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ClassElement<'src> {
    MethodDefinition(MethodDefinition<'src>),
    PropertyDefinition(PropertyDefinition<'src>),
    StaticBlock(BlockStatement<'src>),
}

#[derive(Debug, Clone)]
pub struct MethodDefinition<'src> {
    pub key: PropertyKey<'src>,
    pub value: FunctionExpression<'src>,
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
pub struct PropertyDefinition<'src> {
    pub key: PropertyKey<'src>,
    pub value: Option<Box<Expression<'src>>>,
    pub computed: bool,
    pub is_static: bool,
    pub span: Span,
}

/// Import declaration
#[derive(Debug, Clone)]
pub struct ImportDeclaration<'src> {
    pub specifiers: Vec<ImportSpecifier<'src>>,
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
pub enum ExportDeclaration<'src> {
    /// export { x }
    Named {
        specifiers: Vec<ExportSpecifier<'src>>,
        source: Option<StringLiteral<'src>>,
        span: Span,
    },
    /// export default expr
    Default {
        declaration: Box<Expression<'src>>,
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
        declaration: Box<Statement<'src>>,
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
pub enum Expression<'src> {
    /// Identifier: foo
    Identifier(Identifier<'src>),
    /// Literal: 42, "hello", true, null
    Literal(Literal<'src>),
    /// This expression: this
    This(Span),
    /// Super expression: super
    Super(Span),
    /// Array literal: [1, 2, 3]
    Array(ArrayExpression<'src>),
    /// Object literal: { a: 1, b: 2 }
    Object(ObjectExpression<'src>),
    /// Function expression: function() {}
    Function(FunctionExpression<'src>),
    /// Arrow function: () => {}
    Arrow(ArrowFunctionExpression<'src>),
    /// Class expression: class {}
    Class(ClassExpression<'src>),
    /// Template literal: `hello ${name}`
    TemplateLiteral(TemplateLiteral<'src>),
    /// Tagged template: tag`hello`
    TaggedTemplate(TaggedTemplateExpression<'src>),
    /// Member expression: obj.prop or obj[prop]
    Member(MemberExpression<'src>),
    /// Call expression: fn(args)
    Call(CallExpression<'src>),
    /// New expression: new Foo()
    New(NewExpression<'src>),
    /// Unary expression: !x, -x, typeof x
    Unary(UnaryExpression<'src>),
    /// Update expression: x++, --x
    Update(UpdateExpression<'src>),
    /// Binary expression: x + y
    Binary(BinaryExpression<'src>),
    /// Logical expression: x && y
    Logical(LogicalExpression<'src>),
    /// Conditional expression: x ? y : z
    Conditional(ConditionalExpression<'src>),
    /// Assignment expression: x = y
    Assignment(AssignmentExpression<'src>),
    /// Sequence expression: x, y, z
    Sequence(SequenceExpression<'src>),
    /// Yield expression: yield x
    Yield(YieldExpression<'src>),
    /// Await expression: await x
    Await(AwaitExpression<'src>),
    /// Import expression: import('module')
    Import(ImportExpression<'src>),
    /// Meta property: new.target, import.meta
    MetaProperty(MetaProperty<'src>),
    /// Spread element: ...x
    Spread(SpreadElement<'src>),
    /// Parenthesized: (x)
    Parenthesized(ParenthesizedExpression<'src>),
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
pub struct ArrayExpression<'src> {
    pub elements: Vec<ArrayElement<'src>>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ArrayElement<'src> {
    Expression(Expression<'src>),
    Spread(SpreadElement<'src>),
    Hole, // elision: [1,,3]
}

/// Object expression
#[derive(Debug, Clone)]
pub struct ObjectExpression<'src> {
    pub properties: Vec<ObjectProperty<'src>>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ObjectProperty<'src> {
    Property(Property<'src>),
    SpreadProperty(SpreadElement<'src>),
}

#[derive(Debug, Clone)]
pub struct Property<'src> {
    pub key: PropertyKey<'src>,
    pub value: Box<Expression<'src>>,
    pub kind: PropertyKind,
    pub method: bool,
    pub shorthand: bool,
    pub computed: bool,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum PropertyKey<'src> {
    Identifier(Identifier<'src>),
    Literal(Literal<'src>),
    Computed(Box<Expression<'src>>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropertyKind {
    Init,
    Get,
    Set,
}

/// Function expression
#[derive(Debug, Clone)]
pub struct FunctionExpression<'src> {
    pub id: Option<Identifier<'src>>,
    pub params: Vec<Pattern<'src>>,
    pub body: BlockStatement<'src>,
    pub is_async: bool,
    pub is_generator: bool,
    pub span: Span,
}

/// Arrow function expression
#[derive(Debug, Clone)]
pub struct ArrowFunctionExpression<'src> {
    pub params: Vec<Pattern<'src>>,
    pub body: ArrowBody<'src>,
    pub is_async: bool,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ArrowBody<'src> {
    Expression(Box<Expression<'src>>),
    Block(BlockStatement<'src>),
}

/// Class expression
#[derive(Debug, Clone)]
pub struct ClassExpression<'src> {
    pub id: Option<Identifier<'src>>,
    pub super_class: Option<Box<Expression<'src>>>,
    pub body: ClassBody<'src>,
    pub span: Span,
}

/// Template literal
#[derive(Debug, Clone)]
pub struct TemplateLiteral<'src> {
    pub quasis: Vec<TemplateElement<'src>>,
    pub expressions: Vec<Expression<'src>>,
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
pub struct TaggedTemplateExpression<'src> {
    pub tag: Box<Expression<'src>>,
    pub quasi: TemplateLiteral<'src>,
    pub span: Span,
}

/// Member expression
#[derive(Debug, Clone)]
pub struct MemberExpression<'src> {
    pub object: Box<Expression<'src>>,
    pub property: Box<Expression<'src>>,
    pub computed: bool,
    pub optional: bool, // ?.
    pub span: Span,
}

/// Call expression
#[derive(Debug, Clone)]
pub struct CallExpression<'src> {
    pub callee: Box<Expression<'src>>,
    pub arguments: Vec<Argument<'src>>,
    pub optional: bool, // ?.()
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum Argument<'src> {
    Expression(Expression<'src>),
    Spread(SpreadElement<'src>),
}

/// New expression
#[derive(Debug, Clone)]
pub struct NewExpression<'src> {
    pub callee: Box<Expression<'src>>,
    pub arguments: Vec<Argument<'src>>,
    pub span: Span,
}

/// Unary expression
#[derive(Debug, Clone)]
pub struct UnaryExpression<'src> {
    pub operator: UnaryOperator,
    pub argument: Box<Expression<'src>>,
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
pub struct UpdateExpression<'src> {
    pub operator: UpdateOperator,
    pub argument: Box<Expression<'src>>,
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
pub struct BinaryExpression<'src> {
    pub operator: BinaryOperator,
    pub left: Box<Expression<'src>>,
    pub right: Box<Expression<'src>>,
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
pub struct LogicalExpression<'src> {
    pub operator: LogicalOperator,
    pub left: Box<Expression<'src>>,
    pub right: Box<Expression<'src>>,
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
pub struct ConditionalExpression<'src> {
    pub test: Box<Expression<'src>>,
    pub consequent: Box<Expression<'src>>,
    pub alternate: Box<Expression<'src>>,
    pub span: Span,
}

/// Assignment expression
#[derive(Debug, Clone)]
pub struct AssignmentExpression<'src> {
    pub operator: AssignmentOperator,
    pub left: AssignmentTarget<'src>,
    pub right: Box<Expression<'src>>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum AssignmentTarget<'src> {
    Identifier(Identifier<'src>),
    Member(MemberExpression<'src>),
    Pattern(Pattern<'src>),
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
pub struct SequenceExpression<'src> {
    pub expressions: Vec<Expression<'src>>,
    pub span: Span,
}

/// Yield expression
#[derive(Debug, Clone)]
pub struct YieldExpression<'src> {
    pub argument: Option<Box<Expression<'src>>>,
    pub delegate: bool, // yield*
    pub span: Span,
}

/// Await expression
#[derive(Debug, Clone)]
pub struct AwaitExpression<'src> {
    pub argument: Box<Expression<'src>>,
    pub span: Span,
}

/// Import expression (dynamic import)
#[derive(Debug, Clone)]
pub struct ImportExpression<'src> {
    pub source: Box<Expression<'src>>,
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
pub struct SpreadElement<'src> {
    pub argument: Box<Expression<'src>>,
    pub span: Span,
}

/// Parenthesized expression
#[derive(Debug, Clone)]
pub struct ParenthesizedExpression<'src> {
    pub expression: Box<Expression<'src>>,
    pub span: Span,
}

// ============================================================================
// PATTERNS (for destructuring)
// ============================================================================

/// A pattern (used in variable declarations, function params, assignment)
#[derive(Debug, Clone)]
pub enum Pattern<'src> {
    /// Simple identifier: x
    Identifier(Identifier<'src>),
    /// Array destructuring: [a, b]
    Array(ArrayPattern<'src>),
    /// Object destructuring: { a, b }
    Object(ObjectPattern<'src>),
    /// Rest element: ...x
    Rest(RestElement<'src>),
    /// Assignment pattern: x = default
    Assignment(AssignmentPattern<'src>),
}

#[derive(Debug, Clone)]
pub struct ArrayPattern<'src> {
    pub elements: Vec<Option<Pattern<'src>>>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ObjectPattern<'src> {
    pub properties: Vec<ObjectPatternProperty<'src>>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ObjectPatternProperty<'src> {
    Property {
        key: PropertyKey<'src>,
        value: Pattern<'src>,
        shorthand: bool,
        computed: bool,
        span: Span,
    },
    Rest(RestElement<'src>),
}

#[derive(Debug, Clone)]
pub struct RestElement<'src> {
    pub argument: Box<Pattern<'src>>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct AssignmentPattern<'src> {
    pub left: Box<Pattern<'src>>,
    pub right: Box<Expression<'src>>,
    pub span: Span,
}

// ============================================================================
// IMPL BLOCKS
// ============================================================================

impl<'src> Statement<'src> {
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

impl<'src> ExportDeclaration<'src> {
    pub fn span(&self) -> Span {
        match self {
            ExportDeclaration::Named { span, .. } => *span,
            ExportDeclaration::Default { span, .. } => *span,
            ExportDeclaration::All { span, .. } => *span,
            ExportDeclaration::Declaration { span, .. } => *span,
        }
    }
}

impl<'src> Expression<'src> {
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

impl<'src> Literal<'src> {
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

impl<'src> Pattern<'src> {
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

impl<'src> RestElement<'src> {
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
        let program = Program {
            body: vec![],
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
