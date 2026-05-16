//! Bytecode compiler - transforms AST to bytecode
//!
//! Implements:
//! - Scope analysis (var hoisting, TDZ for let/const)
//! - Register allocation
//! - Control flow lowering
//! - Closure capture detection

use std::collections::HashMap;

use super::chunk::{Chunk, Constant, SourceLocation};
use super::instruction::{Instruction, Register};
use super::opcode::Opcode;
use crate::lexer::{Span, Symbol};
use crate::parser::{
    Argument, ArrayElement, AssignmentOperator, AssignmentTarget, BinaryOperator, Expression,
    ForInLeft, ForInit, Identifier, Literal, LogicalOperator, ObjectProperty, Program, PropertyKey,
    Statement, UnaryOperator, UpdateOperator, VariableDeclaration, VariableKind,
};

/// Compilation error
#[derive(Debug, Clone)]
pub struct CompileError {
    pub message: String,
    pub span: Span,
}

impl CompileError {
    pub fn new(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span,
        }
    }
}

/// Result type for compilation
pub type CompileResult<T> = Result<T, CompileError>;

/// Output of `compile_with_children`: main chunk, child function chunks, and interned string pool.
pub type CompileOutput = (Chunk, Vec<Chunk>, Vec<(u32, String)>);

/// Variable binding in the scope
#[derive(Debug, Clone)]
struct Binding {
    slot: u8,
    kind: VariableKind,
    initialized: bool,
}

/// Scope for lexical environment
#[derive(Debug)]
struct Scope {
    bindings: HashMap<Symbol, Binding>,
    parent: Option<usize>,
    depth: u8,
}

impl Scope {
    fn new(parent: Option<usize>, depth: u8) -> Self {
        Self {
            bindings: HashMap::new(),
            parent,
            depth,
        }
    }
}

/// Label for break/continue targets
#[derive(Debug, Clone, Copy)]
struct JumpLabel {
    offset: usize,
}

/// Loop context for break/continue
#[derive(Debug)]
struct LoopContext {
    break_targets: Vec<JumpLabel>,
    continue_targets: Vec<JumpLabel>,
}

/// Description of one upvalue captured by an inner function from its
/// enclosing function. `parent_slot` is the source register (in the
/// parent's frame) the parent will read at `BindCapture` time. `name`
/// is kept for lookup deduplication so re-referencing the same
/// captured variable does not allocate a new slot.
#[derive(Debug, Clone, Copy)]
struct UpvalueDesc {
    name: Symbol,
    parent_slot: u8,
}

/// Bytecode compiler
pub struct Compiler<'src, 'arena> {
    chunk: Chunk,
    /// Child chunks for nested function expressions / arrow functions
    child_chunks: Vec<Chunk>,
    /// String intern table for property names and identifiers.
    /// Maps string content -> u32 index in the VM's `StringTable`.
    string_pool: HashMap<String, u32>,
    next_string_id: u32,
    scopes: Vec<Scope>,
    current_scope: usize,
    next_register: u8,
    max_register: u8,
    loop_stack: Vec<LoopContext>,
    strict: bool,
    /*
     * parent_locals -- snapshot of the enclosing function's local slots.
     *
     * WHY: A child Compiler (inner function body) needs to detect when an
     * identifier reference resolves to a binding in an enclosing function
     * scope so it can register that binding as an upvalue. The `scopes`
     * field only covers this Compiler's own block scopes (which never
     * cross function boundaries), so we capture the parent's
     * symbol-to-slot map at construction time and consult it on lookup
     * miss before falling through to GetGlobal.
     *
     * Top-level Compilers leave this empty.
     */
    parent_locals: HashMap<Symbol, u8>,
    /*
     * upvalues -- ordered list of captured-from-parent slot indices.
     *
     * WHY: When the child references `parent_locals[sym]`, we record the
     * parent slot here and use this Vec's index as the runtime
     * captures-array index. The compiler emits GetCapture(reg, depth=0,
     * captures_idx) for reads and a matching SetCapture for writes,
     * relying on `op_get_capture`'s depth==0 mode to read from
     * CallFrame.captures (seeded by op_call from JsFunction.captures).
     *
     * The parent emits one BindCapture(func_reg, parent_slot) instruction
     * per entry, in this exact order, immediately after NewFunction so
     * the inner closure carries its upvalues from creation time.
     */
    upvalues: Vec<UpvalueDesc>,
    /*
     * is_async_function -- true when compiling the body of an async function
     * or async arrow.
     *
     * WHY: Inside an async body every `return X` must wrap X in Promise.resolve
     * before unwinding to the caller, and the implicit fall-off-the-end return
     * must do the same with `undefined`.  The compiler emits AsyncReturn
     * instead of Ret/RetUndefined when this flag is set, and the VM's
     * op_async_return handles the wrapping at runtime (see vm/mod.rs).
     *
     * The flag is only ever set on a child compiler created for the async
     * function's body; the top-level Compiler always has it cleared.
     *
     * See: Statement::FunctionDeclaration, Expression::Function, Expression::Arrow
     *      for where the flag is set when compiling async bodies
     * See: Statement::Return for the AsyncReturn vs Ret selection
     */
    is_async_function: bool,
    errors: Vec<CompileError>,
    _phantom: std::marker::PhantomData<(&'src (), &'arena ())>,
}

impl<'src, 'arena> Compiler<'src, 'arena> {
    #[must_use]
    pub fn new() -> Self {
        Self::new_with_pool(HashMap::new(), 0)
    }

    /*
     * new_with_pool -- create a child compiler that shares the parent's string pool.
     *
     * WHY: Each function expression body is compiled by a fresh Compiler.
     * Without sharing the string pool, the child's string constants use IDs
     * from a separate pool (0, 1, 2...) that collide with parent IDs.
     * When main.rs remaps the parent's strings to VM IDs, the child's
     * constants get the WRONG VM string (parent's string 0 != child's string 0).
     *
     * By starting the child with a copy of the parent's pool and the parent's
     * next_string_id, new strings the child interns get fresh IDs that don't
     * overlap with any existing strings. After compilation, the caller merges
     * the child's pool back via into_parts().
     *
     * See: Expression::Function for where this is used
     * See: compile_with_children for how the unified pool is returned
     */
    fn new_with_pool(pool: HashMap<String, u32>, next_id: u32) -> Self {
        Self::new_with_pool_and_parent(pool, next_id, HashMap::new())
    }

    /*
     * new_with_pool_and_parent -- create a child compiler that knows about
     * its enclosing function's local bindings.
     *
     * WHY: Closure detection. The child compiler's `lookup_var` first
     * walks its own scopes (block scopes within the child function); on
     * miss, it consults `parent_locals` to find variables that should be
     * promoted to upvalues. Pass an empty map for the top-level Compiler.
     */
    fn new_with_pool_and_parent(
        pool: HashMap<String, u32>,
        next_id: u32,
        parent_locals: HashMap<Symbol, u8>,
    ) -> Self {
        let scopes = vec![Scope::new(None, 0)];
        Self {
            chunk: Chunk::new(),
            child_chunks: Vec::new(),
            string_pool: pool,
            next_string_id: next_id,
            scopes,
            current_scope: 0,
            next_register: 0,
            max_register: 0,
            loop_stack: Vec::new(),
            strict: false,
            is_async_function: false,
            errors: Vec::new(),
            parent_locals,
            upvalues: Vec::new(),
            _phantom: std::marker::PhantomData,
        }
    }

    /*
     * collect_locals_snapshot -- export the current scope chain as a
     * symbol-to-slot map for a child compiler's parent_locals.
     *
     * WHY: The child only needs symbol -> slot mapping for upvalue
     * resolution; it does not need access to the parent's mutable
     * Compiler. We flatten ALL active scopes (innermost wins on
     * collision, matching ES lexical lookup).
     */
    fn collect_locals_snapshot(&self) -> HashMap<Symbol, u8> {
        let mut snapshot: HashMap<Symbol, u8> = HashMap::new();
        // Walk from the outermost scope inward so inner shadowing wins.
        let mut chain: Vec<usize> = Vec::new();
        let mut idx = Some(self.current_scope);
        while let Some(i) = idx {
            chain.push(i);
            idx = self.scopes[i].parent;
        }
        for scope_idx in chain.iter().rev() {
            for (sym, binding) in &self.scopes[*scope_idx].bindings {
                snapshot.insert(*sym, binding.slot);
            }
        }
        snapshot
    }

    /*
     * compile -- transform a parsed AST into bytecode.
     *
     * Returns (main_chunk, child_chunks) where child_chunks are
     * function expression / arrow function bodies. The caller must
     * add all chunks to the VM: main chunk first, then children.
     * Function constants reference child chunks by index.
     *
     * See: Vm::add_chunk() for registering chunks
     * See: op_new_function for creating Value::Function from chunk index
     */
    pub fn compile(mut self, program: &Program<'src, 'arena>) -> CompileResult<Chunk> {
        self.check_strict_directive(program);
        self.collect_declarations(program.body);

        for stmt in program.body {
            self.compile_statement(stmt)?;
        }

        self.emit(Instruction::new(Opcode::RetUndefined));
        self.chunk.register_count = self.max_register + 1;
        self.chunk.strict = self.strict;

        if !self.errors.is_empty() {
            return Err(self.errors.remove(0));
        }

        Ok(self.chunk)
    }

    /// Get child chunks (function bodies) produced during compilation.
    /// Must be called after `compile()` on a second Compiler instance,
    /// or via `compile_with_children()`.
    /// Compile and return (`main_chunk`, `child_chunks`, `string_pool`).
    pub fn compile_with_children(
        mut self,
        program: &Program<'src, 'arena>,
    ) -> CompileResult<CompileOutput> {
        self.check_strict_directive(program);
        self.collect_declarations(program.body);

        for stmt in program.body {
            self.compile_statement(stmt)?;
        }

        self.emit(Instruction::new(Opcode::RetUndefined));
        self.chunk.register_count = self.max_register + 1;
        self.chunk.strict = self.strict;

        if !self.errors.is_empty() {
            return Err(self.errors.remove(0));
        }

        let pool = self.get_string_pool();
        Ok((self.chunk, self.child_chunks, pool))
    }

    /// Convert this compiler into its chunk (for child function compilation).
    fn into_chunk(mut self) -> Chunk {
        self.chunk.register_count = self.max_register + 1;
        self.chunk
    }

    /*
     * into_parts -- consume the compiler, returning the chunk AND all child
     * chunks plus the merged string pool.
     *
     * WHY: into_chunk() dropped child_chunks (nested function bodies) and the
     * string pool. Callers that use new_with_pool() need both to:
     *   1. Propagate nested function chunks up to the parent's child_chunks list
     *   2. Merge string additions back to the parent's pool so subsequent
     *      interning continues from the right ID.
     *
     * Returns: (main_chunk, nested_chunks, string_pool, next_string_id)
     */
    fn into_parts(
        mut self,
    ) -> (
        Chunk,
        Vec<Chunk>,
        HashMap<String, u32>,
        u32,
        Vec<UpvalueDesc>,
    ) {
        self.chunk.register_count = self.max_register + 1;
        (
            self.chunk,
            self.child_chunks,
            self.string_pool,
            self.next_string_id,
            self.upvalues,
        )
    }

    /*
     * intern_string -- intern a property/variable name, returning its u32 ID.
     *
     * WHY: Every property access (obj.prop), global lookup (GetGlobal),
     * and property set (obj.prop = val) needs the property name as a u32
     * string index. The VM's StringTable resolves these at runtime.
     *
     * Previously ALL property names used Constant::String(0) -- a hardcoded
     * index that resolved to whatever string was first interned. This broke
     * all property access since every property name was the same.
     *
     * Now each unique string gets a distinct ID. The string pool is passed
     * to the VM after compilation via get_string_pool().
     */
    fn intern_string(&mut self, name: &str) -> u32 {
        if let Some(&id) = self.string_pool.get(name) {
            id
        } else {
            let id = self.next_string_id;
            self.next_string_id += 1;
            self.string_pool.insert(name.to_string(), id);
            id
        }
    }

    /// Get the string pool for loading into the VM's `StringTable`.
    #[must_use]
    pub fn get_string_pool(&self) -> Vec<(u32, String)> {
        self.string_pool
            .iter()
            .map(|(s, &id)| (id, s.clone()))
            .collect()
    }

    fn check_strict_directive(&mut self, program: &Program<'src, 'arena>) {
        if let Some(Statement::Expression(expr_stmt)) = program.body.first()
            && let Expression::Literal(Literal::String(s)) = expr_stmt.expression
            && s.value == "use strict"
        {
            self.strict = true;
        }
    }

    /*
     * collect_declarations -- gather all `var` and function declarations in a
     * function-scope-rooted statement list and pre-bind their slots.
     *
     * WHY: ECMA-262 hoists `var` and function declarations to the nearest
     * enclosing function (or script) scope, regardless of how deeply nested
     * they appear inside blocks, if/else arms, for/while bodies, try/catch,
     * switch cases, etc. Without recursive traversal, a `var i = 0` inside
     * `for (var i = 0; ...)` would never get a binding in the enclosing
     * scope, causing every read of `i` from the loop body to fall through
     * to GetGlobal "i" (returning Undefined) instead of GetLocal slot=N.
     *
     * The recursion stops at function/arrow/class boundaries: those introduce
     * their own function scope and are responsible for calling
     * collect_declarations on their own bodies (see compile_function_body and
     * the function-expression compile paths).
     *
     * For-loop init declarations ARE walked even though the for-loop wraps
     * its body in a fresh lexical scope at compile time -- `var` ignores
     * lexical scopes, and the for-init `var` must hoist to the function
     * scope so that the body, the test, and the update all see the same
     * binding.
     */
    fn collect_declarations(&mut self, stmts: &[Statement<'src, 'arena>]) {
        for stmt in stmts {
            self.collect_declarations_in_stmt(stmt);
        }
    }

    fn collect_declarations_in_stmt(&mut self, stmt: &Statement<'src, 'arena>) {
        match stmt {
            Statement::VariableDeclaration(decl) => {
                if decl.kind == VariableKind::Var {
                    self.collect_var_declaration(decl);
                }
            }
            // Hoist function declarations: pre-allocate their slots so the
            // function is available before its textual position.
            // Do NOT descend into the function body -- it is its own scope.
            Statement::FunctionDeclaration(func) => {
                if let Some(ref id) = func.id
                    && self.lookup_var(id.name).is_none()
                {
                    self.declare_var(id.name, VariableKind::Var, false);
                }
            }
            Statement::Block(block) => {
                for s in block.body {
                    self.collect_declarations_in_stmt(s);
                }
            }
            Statement::If(if_stmt) => {
                self.collect_declarations_in_stmt(if_stmt.consequent);
                if let Some(alt) = if_stmt.alternate.as_ref() {
                    self.collect_declarations_in_stmt(alt);
                }
            }
            Statement::While(w) => self.collect_declarations_in_stmt(w.body),
            Statement::DoWhile(dw) => self.collect_declarations_in_stmt(dw.body),
            Statement::For(for_stmt) => {
                if let Some(crate::parser::ForInit::VariableDeclaration(decl)) =
                    for_stmt.init.as_ref()
                    && decl.kind == VariableKind::Var
                {
                    self.collect_var_declaration(decl);
                }
                self.collect_declarations_in_stmt(for_stmt.body);
            }
            Statement::ForIn(for_in) => {
                if let crate::parser::ForInLeft::VariableDeclaration(decl) = &for_in.left
                    && decl.kind == VariableKind::Var
                {
                    self.collect_var_declaration(decl);
                }
                self.collect_declarations_in_stmt(for_in.body);
            }
            Statement::ForOf(for_of) => {
                if let crate::parser::ForInLeft::VariableDeclaration(decl) = &for_of.left
                    && decl.kind == VariableKind::Var
                {
                    self.collect_var_declaration(decl);
                }
                self.collect_declarations_in_stmt(for_of.body);
            }
            Statement::Try(try_stmt) => {
                for s in try_stmt.block.body {
                    self.collect_declarations_in_stmt(s);
                }
                if let Some(catch) = &try_stmt.handler {
                    for s in catch.body.body {
                        self.collect_declarations_in_stmt(s);
                    }
                }
                if let Some(finalizer) = &try_stmt.finalizer {
                    for s in finalizer.body {
                        self.collect_declarations_in_stmt(s);
                    }
                }
            }
            Statement::Switch(sw) => {
                for case in sw.cases {
                    for s in case.consequent {
                        self.collect_declarations_in_stmt(s);
                    }
                }
            }
            Statement::Labeled(lab) => self.collect_declarations_in_stmt(lab.body),
            Statement::With(w) => self.collect_declarations_in_stmt(w.body),
            // Other statements cannot host hoisted var/function declarations.
            _ => {}
        }
    }

    fn collect_var_declaration(&mut self, decl: &VariableDeclaration<'src, 'arena>) {
        for declarator in decl.declarations {
            if let crate::parser::Pattern::Identifier(id) = &declarator.id
                && self.lookup_var(id.name).is_none()
            {
                self.declare_var(id.name, VariableKind::Var, false);
            }
        }
    }

    fn alloc_register(&mut self) -> Register {
        let reg = Register::new(self.next_register);
        self.next_register += 1;
        if self.next_register > self.max_register {
            self.max_register = self.next_register;
        }
        reg
    }

    fn free_registers_to(&mut self, checkpoint: u8) {
        self.next_register = checkpoint;
    }

    fn emit(&mut self, instr: Instruction) -> usize {
        self.chunk.emit(instr)
    }

    fn emit_at(&mut self, instr: Instruction, span: Span) -> usize {
        let loc = SourceLocation {
            offset: span.start,
            line: 0,
            column: 0,
        };
        self.chunk.emit_with_loc(instr, loc)
    }

    fn current_offset(&self) -> usize {
        self.chunk.len()
    }

    fn patch_jump(&mut self, offset: usize) {
        let target = self.current_offset();
        let instr = self.chunk.instructions[offset];
        // UNWRAP-OK: compiler invariant: patch_jump is only called on offsets returned by
        // emit() with a valid Opcode (Jmp/JmpFalse/JmpTrue/etc.); the byte was just written
        // by us from a typed Opcode value, so round-trip Opcode::from_byte cannot fail.
        let opcode = Opcode::from_byte(instr.opcode()).unwrap();
        let relative = (target as i32) - (offset as i32) - 1;

        if opcode == Opcode::Jmp {
            self.chunk.instructions[offset] = Instruction::new_offset(Opcode::Jmp, relative);
        } else {
            let reg = instr.dst();
            self.chunk.instructions[offset] =
                Instruction::new_r_offset(opcode, reg, relative as i16);
        }
    }

    fn declare_var(&mut self, name: Symbol, kind: VariableKind, initialized: bool) {
        let slot = self.alloc_register();
        let binding = Binding {
            slot: slot.0,
            kind,
            initialized,
        };
        self.scopes[self.current_scope]
            .bindings
            .insert(name, binding);
    }

    fn lookup_var(&self, name: Symbol) -> Option<(u8, u8)> {
        let mut scope_idx = self.current_scope;
        let mut depth = 0u8;

        loop {
            if let Some(binding) = self.scopes[scope_idx].bindings.get(&name) {
                return Some((depth, binding.slot));
            }
            if let Some(parent) = self.scopes[scope_idx].parent {
                scope_idx = parent;
                depth += 1;
            } else {
                break;
            }
        }
        None
    }

    /*
     * resolve_upvalue -- check whether `name` is a captured-from-parent
     * binding and, if so, return its captures-array slot, allocating a
     * new entry on first reference.
     *
     * WHY: Called by compile_identifier (and the SetLocal-vs-SetCapture
     * choice in compile_assignment / compile_var_declaration) after
     * `lookup_var` has missed the local scope chain. If the parent
     * function has a binding with this name, we promote it to an upvalue,
     * cache the slot in `upvalues`, and return the slot index. The
     * compiler then emits GetCapture/SetCapture with depth=0 and slot =
     * captures index, and the parent emits one BindCapture per upvalue
     * after NewFunction.
     *
     * Returns None if the name is not in the parent's local set (the
     * caller falls through to a global lookup).
     */
    fn resolve_upvalue(&mut self, name: Symbol) -> Option<u8> {
        if let Some(existing) = self.upvalues.iter().position(|u| u.name == name) {
            return Some(existing as u8);
        }
        let parent_slot = *self.parent_locals.get(&name)?;
        let idx = self.upvalues.len() as u8;
        self.upvalues.push(UpvalueDesc { name, parent_slot });
        Some(idx)
    }

    fn enter_scope(&mut self) {
        let parent = self.current_scope;
        let depth = self.scopes[parent].depth + 1;
        self.scopes.push(Scope::new(Some(parent), depth));
        self.current_scope = self.scopes.len() - 1;
    }

    fn exit_scope(&mut self) {
        if let Some(parent) = self.scopes[self.current_scope].parent {
            self.current_scope = parent;
        }
    }

    // Statement compilation

    fn compile_statement(&mut self, stmt: &Statement<'src, 'arena>) -> CompileResult<()> {
        match stmt {
            Statement::Expression(expr_stmt) => {
                let checkpoint = self.next_register;
                let _reg = self.compile_expression(expr_stmt.expression)?;
                self.free_registers_to(checkpoint);
            }
            Statement::VariableDeclaration(decl) => {
                self.compile_var_declaration(decl)?;
            }
            Statement::Block(block) => {
                self.enter_scope();
                for s in block.body {
                    self.compile_statement(s)?;
                }
                self.exit_scope();
            }
            Statement::If(if_stmt) => {
                let cond_reg = self.compile_expression(if_stmt.test)?;
                let else_jump =
                    self.emit(Instruction::new_r_offset(Opcode::JmpFalse, cond_reg.0, 0));
                self.compile_statement(if_stmt.consequent)?;

                if let Some(alternate) = if_stmt.alternate {
                    let end_jump = self.emit(Instruction::new_offset(Opcode::Jmp, 0));
                    self.patch_jump(else_jump);
                    self.compile_statement(alternate)?;
                    self.patch_jump(end_jump);
                } else {
                    self.patch_jump(else_jump);
                }
            }
            Statement::While(while_stmt) => {
                let loop_start = self.current_offset();
                self.loop_stack.push(LoopContext {
                    break_targets: Vec::new(),
                    continue_targets: Vec::new(),
                });

                let cond_reg = self.compile_expression(while_stmt.test)?;
                let exit_jump =
                    self.emit(Instruction::new_r_offset(Opcode::JmpFalse, cond_reg.0, 0));
                self.compile_statement(while_stmt.body)?;

                let back_offset = (loop_start as i32) - (self.current_offset() as i32) - 1;
                self.emit(Instruction::new_offset(Opcode::Jmp, back_offset));
                self.patch_jump(exit_jump);

                // UNWRAP-OK: compiler invariant: we pushed a LoopContext at the top of this
                // While arm and have not popped it; loop_stack is non-empty here.
                let loop_ctx = self.loop_stack.pop().unwrap();
                for brk in loop_ctx.break_targets {
                    self.patch_jump(brk.offset);
                }
                for cont in loop_ctx.continue_targets {
                    let rel = (loop_start as i32) - (cont.offset as i32) - 1;
                    self.chunk.instructions[cont.offset] =
                        Instruction::new_offset(Opcode::Jmp, rel);
                }
            }
            Statement::For(for_stmt) => {
                self.enter_scope();

                if let Some(init) = for_stmt.init.as_ref() {
                    match init {
                        ForInit::VariableDeclaration(decl) => {
                            self.compile_var_declaration(decl)?;
                        }
                        ForInit::Expression(expr) => {
                            let _ = self.compile_expression(expr)?;
                        }
                    }
                }

                let loop_start = self.current_offset();
                self.loop_stack.push(LoopContext {
                    break_targets: Vec::new(),
                    continue_targets: Vec::new(),
                });

                let exit_jump = if let Some(test) = for_stmt.test.as_ref() {
                    let cond_reg = self.compile_expression(test)?;
                    Some(self.emit(Instruction::new_r_offset(Opcode::JmpFalse, cond_reg.0, 0)))
                } else {
                    None
                };

                self.compile_statement(for_stmt.body)?;
                let continue_target = self.current_offset();

                if let Some(update) = for_stmt.update.as_ref() {
                    let _ = self.compile_expression(update)?;
                }

                let back_offset = (loop_start as i32) - (self.current_offset() as i32) - 1;
                self.emit(Instruction::new_offset(Opcode::Jmp, back_offset));

                if let Some(exit) = exit_jump {
                    self.patch_jump(exit);
                }

                // UNWRAP-OK: compiler invariant: we pushed a LoopContext at the top of this
                // For arm and have not popped it; loop_stack is non-empty here.
                let loop_ctx = self.loop_stack.pop().unwrap();
                for brk in loop_ctx.break_targets {
                    self.patch_jump(brk.offset);
                }
                for cont in loop_ctx.continue_targets {
                    let rel = (continue_target as i32) - (cont.offset as i32) - 1;
                    self.chunk.instructions[cont.offset] =
                        Instruction::new_offset(Opcode::Jmp, rel);
                }

                self.exit_scope();
            }
            /*
             * do { body } while (condition);
             *
             * Bytecode: body -> test -> JmpTrue(body_start)
             * The body always executes at least once.
             */
            Statement::DoWhile(dw) => {
                let loop_start = self.current_offset();
                self.loop_stack.push(LoopContext {
                    break_targets: Vec::new(),
                    continue_targets: Vec::new(),
                });

                self.compile_statement(dw.body)?;

                let continue_target = self.current_offset();
                let cond_reg = self.compile_expression(dw.test)?;
                let back_offset = (loop_start as i32) - (self.current_offset() as i32) - 1;
                self.emit(Instruction::new_r_offset(
                    Opcode::JmpTrue,
                    cond_reg.0,
                    back_offset as i16,
                ));

                // UNWRAP-OK: compiler invariant: we pushed a LoopContext at the top of this
                // DoWhile arm and have not popped it; loop_stack is non-empty here.
                let loop_ctx = self.loop_stack.pop().unwrap();
                for brk in loop_ctx.break_targets {
                    self.patch_jump(brk.offset);
                }
                for cont in loop_ctx.continue_targets {
                    let rel = (continue_target as i32) - (cont.offset as i32) - 1;
                    self.chunk.instructions[cont.offset] =
                        Instruction::new_offset(Opcode::Jmp, rel);
                }
            }
            /*
             * try { body } catch (e) { handler } finally { cleanup }
             *
             * WHY: try/catch/finally requires three separate bytecode layouts
             * depending on which clauses are present. In all cases an
             * ExceptionHandler record is added to chunk.handlers and EnterTry
             * carries its index so the VM can look up catch_target and
             * finally_target at throw time.
             *
             * NORMAL path (no exception):
             *   EnterTry [idx]
             *   <try body>
             *   LeaveTry
             *   [Jmp skip_catch]       -- if catch present
             *   [<catch block>]        -- if catch present
             *   [skip_catch:]
             *   [<finally body>]       -- if finally present (normal path copy)
             *   [Jmp after_finally]    -- if finally present
             *   [<finally body again>] -- finally throw-path duplicate (Rethrow)
             *   after_finally:         -- / or just end if no finally
             *
             * THROW path:
             *   catch_target points to EnterCatch; exception in r0; compiler
             *   copies r0 -> catch variable via Mov immediately after EnterCatch.
             *   finally_target points to the throw-path duplicate of the finally
             *   block, which ends with Rethrow.
             *
             * See: vm/mod.rs dispatch_exception for throw routing
             * See: vm/mod.rs op_rethrow for throw-path finally termination
             * See: chunk.rs ExceptionHandler for the handler record layout
             */
            Statement::Try(try_stmt) => {
                let has_catch = try_stmt.handler.is_some();

                // Reserve a handler slot index before emitting EnterTry.
                // We will backpatch catch_target and finally_target below.
                let handler_index = self.chunk.handlers.len() as u16;
                use crate::bytecode::chunk::ExceptionHandler;
                self.chunk.handlers.push(ExceptionHandler {
                    try_start: self.current_offset() as u32,
                    try_end: 0,           // backpatched after LeaveTry
                    catch_target: None,   // backpatched if catch present
                    finally_target: None, // backpatched if finally present
                    exception_reg: 0,
                });

                // EnterTry: operand is the handler index in chunk.handlers.
                let _enter_try = self.emit(Instruction::new_ri(Opcode::EnterTry, 0, handler_index));

                // Compile try body.
                for stmt in try_stmt.block.body {
                    self.compile_statement(stmt)?;
                }

                // LeaveTry: normal exit from try block.
                self.emit(Instruction::new(Opcode::LeaveTry));

                // Backpatch try_end to the instruction after LeaveTry.
                let try_end_pc = self.current_offset() as u32;
                self.chunk.handlers[handler_index as usize].try_end = try_end_pc;

                // ---- catch block ----
                let skip_catch_jump = if has_catch {
                    // Jump over catch block on normal path.
                    let jmp = self.emit(Instruction::new_offset(Opcode::Jmp, 0));
                    Some(jmp)
                } else {
                    None
                };

                if let Some(ref handler) = try_stmt.handler {
                    // Record where the catch block starts.
                    let catch_start = self.current_offset() as u32;
                    self.chunk.handlers[handler_index as usize].catch_target = Some(catch_start);

                    self.emit(Instruction::new(Opcode::EnterCatch));

                    // Bind the catch variable: exception is already in r0.
                    // Allocate a register for `e` and copy r0 into it so the
                    // catch body can reference it by name.
                    if let Some(crate::parser::Pattern::Identifier(ref id)) = handler.param {
                        self.declare_var(id.name, VariableKind::Let, true);
                        if let Some((_depth, slot)) = self.lookup_var(id.name) {
                            // Copy exception from r0 into the catch variable's slot.
                            self.emit(Instruction::new_rr(Opcode::Mov, slot, 0));
                        }
                    }

                    // Compile catch body.
                    for stmt in handler.body.body {
                        self.compile_statement(stmt)?;
                    }
                }

                // Patch the normal-path jump over the catch block.
                if let Some(jmp) = skip_catch_jump {
                    self.patch_jump(jmp);
                }

                // ---- finally block ----
                if let Some(ref finalizer) = try_stmt.finalizer {
                    // Normal path finally: fall through from catch (or try if no catch).
                    // The finally code runs and then jumps past the throw-path duplicate.
                    for stmt in finalizer.body {
                        self.compile_statement(stmt)?;
                    }
                    let skip_throw_finally = self.emit(Instruction::new_offset(Opcode::Jmp, 0));

                    // Throw-path finally: a duplicate of the finally body that ends
                    // with Rethrow to re-throw the pending exception.
                    let finally_throw_start = self.current_offset() as u32;
                    self.chunk.handlers[handler_index as usize].finally_target =
                        Some(finally_throw_start);

                    self.emit(Instruction::new(Opcode::EnterFinally));
                    for stmt in finalizer.body {
                        self.compile_statement(stmt)?;
                    }
                    // Rethrow re-throws the pending_exception stored in the TryHandler.
                    self.emit(Instruction::new(Opcode::Rethrow));

                    // Patch the normal-path jump over the throw-path duplicate.
                    self.patch_jump(skip_throw_finally);
                }
            }
            Statement::Return(ret) => {
                /*
                 * Inside an async function body we MUST emit AsyncReturn so the
                 * VM wraps the value in Promise.resolve before unwinding to
                 * the caller.  For a bare `return;` we synthesise an undefined
                 * register and feed it through AsyncReturn so the caller sees
                 * a fulfilled Promise<undefined> rather than the raw undefined.
                 *
                 * Non-async functions keep the original Ret/RetUndefined path
                 * so we do not regress any existing test.
                 *
                 * See: vm/mod.rs op_async_return for the runtime wrap
                 * See: is_async_function for where the flag is set
                 */
                if let Some(arg) = ret.argument.as_ref() {
                    let reg = self.compile_expression(arg)?;
                    let op = if self.is_async_function {
                        Opcode::AsyncReturn
                    } else {
                        Opcode::Ret
                    };
                    self.emit_at(Instruction::new_r(op, reg.0), ret.span);
                } else if self.is_async_function {
                    let undef_reg = self.alloc_register();
                    self.emit_at(
                        Instruction::new_r(Opcode::LoadUndefined, undef_reg.0),
                        ret.span,
                    );
                    self.emit_at(
                        Instruction::new_r(Opcode::AsyncReturn, undef_reg.0),
                        ret.span,
                    );
                } else {
                    self.emit_at(Instruction::new(Opcode::RetUndefined), ret.span);
                }
            }
            Statement::Throw(throw) => {
                let reg = self.compile_expression(throw.argument)?;
                self.emit_at(Instruction::new_r(Opcode::Throw, reg.0), throw.span);
            }
            Statement::Break(brk) => {
                let offset = self.emit(Instruction::new_offset(Opcode::Jmp, 0));
                if let Some(loop_ctx) = self.loop_stack.last_mut() {
                    loop_ctx.break_targets.push(JumpLabel { offset });
                } else {
                    self.errors
                        .push(CompileError::new("break outside of loop", brk.span));
                }
            }
            Statement::Continue(cont) => {
                let offset = self.emit(Instruction::new_offset(Opcode::Jmp, 0));
                if let Some(loop_ctx) = self.loop_stack.last_mut() {
                    loop_ctx.continue_targets.push(JumpLabel { offset });
                } else {
                    self.errors
                        .push(CompileError::new("continue outside of loop", cont.span));
                }
            }
            Statement::Debugger(span) => {
                self.emit_at(Instruction::new(Opcode::Debugger), *span);
            }
            /*
             * FunctionDeclaration -- compile the body into a child chunk and
             * store the resulting Value::Function in the declared slot.
             *
             * WHY: Previously used Constant::Function(0) (always chunk 0 --
             * completely wrong) and mismatched the temp register with the
             * declared slot. This caused all function declarations to produce
             * garbage function values or undefined.
             *
             * Now mirrors Expression::Function: compile body with shared string
             * pool, flatten nested chunks, and emit NewFunction into the
             * pre-hoisted slot from collect_declarations.
             *
             * See: Expression::Function for the same shared-pool pattern
             * See: collect_declarations for the slot pre-allocation
             */
            Statement::FunctionDeclaration(func) => {
                if let Some(ref id) = func.id {
                    let parent_locals = self.collect_locals_snapshot();
                    let mut child = Compiler::new_with_pool_and_parent(
                        self.string_pool.clone(),
                        self.next_string_id,
                        parent_locals,
                    );
                    // Propagate the async marker so Statement::Return inside
                    // the body emits AsyncReturn instead of Ret.
                    child.is_async_function = func.is_async;
                    /*
                     * Two-pass parameter binding to support destructuring.
                     *
                     * Pass 1: allocate one register slot per param in positional
                     *         order so the VM's call convention (args[i] -> reg[i])
                     *         lands correctly. Identifier params get a named slot;
                     *         pattern params get an anonymous slot (alloc_register).
                     * Pass 2: emit GetProp/GetElem/SetLocal instructions that
                     *         destructure each anonymous slot into the named bindings.
                     *
                     * See: compile_pattern_binding for the recursive binding helper
                     * See: vm/mod.rs call frame setup for arg->register mapping
                     */
                    let mut destructuring_params = Vec::new();
                    for param in func.params {
                        if let crate::parser::Pattern::Identifier(pid) = param {
                            child.declare_var(pid.name, VariableKind::Let, true);
                        } else {
                            let slot = child.alloc_register();
                            destructuring_params.push((slot.0, param));
                        }
                    }
                    for &(slot, pattern) in &destructuring_params {
                        child.compile_pattern_binding(
                            pattern,
                            Register(slot),
                            VariableKind::Let,
                        )?;
                    }
                    child.collect_declarations(func.body.body);
                    for stmt in func.body.body {
                        child.compile_statement(stmt)?;
                    }
                    /*
                     * Implicit fall-off-the-end return.
                     *
                     * Sync function: just RetUndefined.
                     * Async function: must produce a fulfilled Promise<undefined>
                     * so the caller's `await` (or `.then`) sees a settled value.
                     * We synthesise an undefined register, then emit AsyncReturn
                     * so the VM wraps it in Promise.resolve(undefined).
                     */
                    if func.is_async {
                        let undef_reg = child.alloc_register();
                        child
                            .chunk
                            .emit(Instruction::new_r(Opcode::LoadUndefined, undef_reg.0));
                        child
                            .chunk
                            .emit(Instruction::new_r(Opcode::AsyncReturn, undef_reg.0));
                    } else {
                        child.chunk.emit(Instruction::new(Opcode::RetUndefined));
                    }
                    let (mut child_chunk, nested, merged_pool, merged_next, child_upvalues) =
                        child.into_parts();
                    self.string_pool = merged_pool;
                    self.next_string_id = merged_next;
                    /*
                     * Flag the child chunk as a generator body so the VM's
                     * op_call diverts invocations to invoke_generator (which
                     * runs the body eagerly, collects yields, and returns an
                     * iterator).  Without this flag the body would execute
                     * as a regular function and yield opcodes would silently
                     * no-op (no buffer on generator_yield_stack).
                     *
                     * See: vm/mod.rs op_call generator-branch.
                     * See: vm/generator.rs for the eager-strategy rationale.
                     */
                    if func.is_generator {
                        child_chunk.is_generator = true;
                    }
                    let base_offset = self.child_chunks.len() as u32;
                    let n_nested = nested.len() as u32;
                    for nc in nested {
                        self.child_chunks.push(nc);
                    }
                    for constant in child_chunk.constants_mut() {
                        if let Constant::Function(idx) = constant {
                            *idx += base_offset;
                        }
                    }
                    let func_idx = base_offset + n_nested;
                    self.child_chunks.push(child_chunk);
                    let const_idx = self.chunk.add_constant(Constant::Function(func_idx));
                    let func_reg = self.alloc_register();
                    /*
                     * Emit NewGenerator for `function*` declarations.
                     * Functionally identical to NewFunction at construction
                     * time (both produce a Value::Function); the divergence
                     * happens at call time when op_call inspects the chunk's
                     * is_generator flag.  Keeping the opcode distinct makes
                     * disassembly and tracing clearer.
                     */
                    let new_op = if func.is_generator {
                        Opcode::NewGenerator
                    } else {
                        Opcode::NewFunction
                    };
                    self.emit(Instruction::new_ri(new_op, func_reg.0, const_idx));
                    // Bind upvalues into the closure: one BindCapture per
                    // captured outer variable, in the same order the inner
                    // function compiled them. See Vm::op_bind_capture.
                    for upvalue in &child_upvalues {
                        self.emit(Instruction::new_rr(
                            Opcode::BindCapture,
                            func_reg.0,
                            upvalue.parent_slot,
                        ));
                    }
                    // Store into the pre-hoisted slot
                    if let Some((depth, slot)) = self.lookup_var(id.name) {
                        if depth == 0 {
                            self.emit(Instruction::new_rr(Opcode::SetLocal, slot, func_reg.0));
                        } else {
                            self.emit(Instruction::new_rrr(
                                Opcode::SetCapture,
                                depth,
                                slot,
                                func_reg.0,
                            ));
                        }
                    }
                }
            }
            /*
             * for (x of iterable) { body }
             *
             * WHY: for...of is widely used in modern JS for iterating arrays,
             * strings, Maps, and Sets. Without compilation, the loop body is
             * silently skipped.
             *
             * Bytecode sequence:
             *   iter_src = compile(right)
             *   iter      = GetIterator(iter_src)
             *   loop:
             *     result  = IterNext(iter)
             *     done    = IterDone(result)
             *     JmpTrue done -> exit
             *     value   = IterValue(result)
             *     SetLocal(loop_var, value)  -- or SetGlobal if undeclared
             *     compile(body)
             *     Jmp -> loop
             *   exit:
             *     IterClose(iter)
             *
             * See: vm/mod.rs op_get_iterator / op_iter_next for runtime semantics
             */
            Statement::ForOf(for_of) => {
                self.enter_scope();

                // Compile the iterable expression
                let iter_src = self.compile_expression(for_of.right)?;
                let checkpoint = self.next_register;

                // Allocate scratch registers that persist for the loop duration
                let iter_reg = self.alloc_register();
                let result_reg = self.alloc_register();
                let done_reg = self.alloc_register();
                let val_reg = self.alloc_register();

                self.emit(Instruction::new_rr(
                    Opcode::GetIterator,
                    iter_reg.0,
                    iter_src.0,
                ));

                let loop_start = self.current_offset();
                self.loop_stack.push(LoopContext {
                    break_targets: Vec::new(),
                    continue_targets: Vec::new(),
                });

                // result = iter.next()
                self.emit(Instruction::new_rr(
                    Opcode::IterNext,
                    result_reg.0,
                    iter_reg.0,
                ));
                // done = result.done
                self.emit(Instruction::new_rr(
                    Opcode::IterDone,
                    done_reg.0,
                    result_reg.0,
                ));
                // if done: exit
                let exit_jump =
                    self.emit(Instruction::new_r_offset(Opcode::JmpTrue, done_reg.0, 0));
                // value = result.value
                self.emit(Instruction::new_rr(
                    Opcode::IterValue,
                    val_reg.0,
                    result_reg.0,
                ));

                // Bind loop variable
                match &for_of.left {
                    ForInLeft::VariableDeclaration(decl) => {
                        for declarator in decl.declarations {
                            if let crate::parser::Pattern::Identifier(id) = &declarator.id {
                                self.declare_var(id.name, decl.kind, true);
                                if let Some((depth, slot)) = self.lookup_var(id.name) {
                                    if depth == 0 {
                                        self.emit(Instruction::new_rr(
                                            Opcode::SetLocal,
                                            slot,
                                            val_reg.0,
                                        ));
                                    } else {
                                        self.emit(Instruction::new_rrr(
                                            Opcode::SetCapture,
                                            depth,
                                            slot,
                                            val_reg.0,
                                        ));
                                    }
                                }
                            }
                        }
                    }
                    ForInLeft::Pattern(crate::parser::Pattern::Identifier(id)) => {
                        if let Some((depth, slot)) = self.lookup_var(id.name) {
                            if depth == 0 {
                                self.emit(Instruction::new_rr(Opcode::SetLocal, slot, val_reg.0));
                            } else {
                                self.emit(Instruction::new_rrr(
                                    Opcode::SetCapture,
                                    depth,
                                    slot,
                                    val_reg.0,
                                ));
                            }
                        } else {
                            // Fall back to global assignment
                            let str_id = self.intern_string(id.raw);
                            let const_idx = self.chunk.add_constant(Constant::String(str_id));
                            self.emit(Instruction::new_ri(Opcode::SetGlobal, val_reg.0, const_idx));
                        }
                    }
                    ForInLeft::Pattern(_) => {}
                }

                // Compile body
                let continue_target = self.current_offset();
                self.compile_statement(for_of.body)?;

                // Jump back to loop head
                let back_offset = (loop_start as i32) - (self.current_offset() as i32) - 1;
                self.emit(Instruction::new_offset(Opcode::Jmp, back_offset));

                // Patch exit jump
                self.patch_jump(exit_jump);

                // Close iterator
                self.emit(Instruction::new_r(Opcode::IterClose, iter_reg.0));

                // UNWRAP-OK: compiler invariant: we pushed a LoopContext at the top of this
                // ForOf arm and have not popped it; loop_stack is non-empty here.
                let loop_ctx = self.loop_stack.pop().unwrap();
                for brk in loop_ctx.break_targets {
                    self.patch_jump(brk.offset);
                }
                for cont in loop_ctx.continue_targets {
                    let rel = (continue_target as i32) - (cont.offset as i32) - 1;
                    self.chunk.instructions[cont.offset] =
                        Instruction::new_offset(Opcode::Jmp, rel);
                }

                self.free_registers_to(checkpoint);
                self.exit_scope();
            }
            /*
             * for (k in obj) { body }
             *
             * WHY: for...in iterates enumerable string keys of an object.
             * Compiled as: collect Object.keys(obj) into array, then indexed
             * iteration. Reuses GetIterator over the keys array.
             *
             * Implementation: synthesise `Object.keys(right)` at compile time
             * by emitting GetProp("keys") on the Object global then Call,
             * then use the same GetIterator loop as for...of.
             * Simplified: emit code equivalent to for (k of Object.keys(obj)).
             */
            Statement::ForIn(for_in) => {
                self.enter_scope();

                // Compute the object
                let obj_reg = self.compile_expression(for_in.right)?;
                let checkpoint = self.next_register;

                // keys_arr = Object.keys(obj)
                let obj_global_reg = self.alloc_register();
                let keys_fn_reg = self.alloc_register();
                let keys_arr_reg = self.alloc_register();

                let obj_str_id = self.intern_string("Object");
                let obj_const_idx = self.chunk.add_constant(Constant::String(obj_str_id));
                self.emit(Instruction::new_ri(
                    Opcode::GetGlobal,
                    obj_global_reg.0,
                    obj_const_idx,
                ));

                let keys_str_id = self.intern_string("keys");
                let keys_const_idx = self.chunk.add_constant(Constant::String(keys_str_id));
                self.emit(Instruction::new_rrr(
                    Opcode::GetProp,
                    keys_fn_reg.0,
                    obj_global_reg.0,
                    keys_const_idx as u8,
                ));
                // Call keys_fn(obj) -> emit Call: dst=keys_arr, fn=keys_fn, argc=1, argv=obj_reg
                // Call encoding: dst=r[keys_arr], src1=r[keys_fn], src2=argc(1)
                // Args are in consecutive registers starting at keys_fn+1, so mov obj_reg there
                let arg_reg = self.alloc_register();
                self.emit(Instruction::new_rr(Opcode::Mov, arg_reg.0, obj_reg.0));
                self.emit(Instruction::new_rrr(
                    Opcode::Call,
                    keys_arr_reg.0,
                    keys_fn_reg.0,
                    1,
                ));

                // Now use GetIterator over keys_arr
                let iter_reg = self.alloc_register();
                let result_reg = self.alloc_register();
                let done_reg = self.alloc_register();
                let val_reg = self.alloc_register();

                self.emit(Instruction::new_rr(
                    Opcode::GetIterator,
                    iter_reg.0,
                    keys_arr_reg.0,
                ));

                let loop_start = self.current_offset();
                self.loop_stack.push(LoopContext {
                    break_targets: Vec::new(),
                    continue_targets: Vec::new(),
                });

                self.emit(Instruction::new_rr(
                    Opcode::IterNext,
                    result_reg.0,
                    iter_reg.0,
                ));
                self.emit(Instruction::new_rr(
                    Opcode::IterDone,
                    done_reg.0,
                    result_reg.0,
                ));
                let exit_jump =
                    self.emit(Instruction::new_r_offset(Opcode::JmpTrue, done_reg.0, 0));
                self.emit(Instruction::new_rr(
                    Opcode::IterValue,
                    val_reg.0,
                    result_reg.0,
                ));

                match &for_in.left {
                    ForInLeft::VariableDeclaration(decl) => {
                        for declarator in decl.declarations {
                            if let crate::parser::Pattern::Identifier(id) = &declarator.id {
                                self.declare_var(id.name, decl.kind, true);
                                if let Some((depth, slot)) = self.lookup_var(id.name) {
                                    if depth == 0 {
                                        self.emit(Instruction::new_rr(
                                            Opcode::SetLocal,
                                            slot,
                                            val_reg.0,
                                        ));
                                    } else {
                                        self.emit(Instruction::new_rrr(
                                            Opcode::SetCapture,
                                            depth,
                                            slot,
                                            val_reg.0,
                                        ));
                                    }
                                }
                            }
                        }
                    }
                    ForInLeft::Pattern(crate::parser::Pattern::Identifier(id)) => {
                        if let Some((depth, slot)) = self.lookup_var(id.name) {
                            if depth == 0 {
                                self.emit(Instruction::new_rr(Opcode::SetLocal, slot, val_reg.0));
                            } else {
                                self.emit(Instruction::new_rrr(
                                    Opcode::SetCapture,
                                    depth,
                                    slot,
                                    val_reg.0,
                                ));
                            }
                        } else {
                            let str_id = self.intern_string(id.raw);
                            let const_idx = self.chunk.add_constant(Constant::String(str_id));
                            self.emit(Instruction::new_ri(Opcode::SetGlobal, val_reg.0, const_idx));
                        }
                    }
                    ForInLeft::Pattern(_) => {}
                }

                let continue_target = self.current_offset();
                self.compile_statement(for_in.body)?;

                let back_offset = (loop_start as i32) - (self.current_offset() as i32) - 1;
                self.emit(Instruction::new_offset(Opcode::Jmp, back_offset));
                self.patch_jump(exit_jump);
                self.emit(Instruction::new_r(Opcode::IterClose, iter_reg.0));

                // UNWRAP-OK: compiler invariant: we pushed a LoopContext at the top of this
                // ForIn arm and have not popped it; loop_stack is non-empty here.
                let loop_ctx = self.loop_stack.pop().unwrap();
                for brk in loop_ctx.break_targets {
                    self.patch_jump(brk.offset);
                }
                for cont in loop_ctx.continue_targets {
                    let rel = (continue_target as i32) - (cont.offset as i32) - 1;
                    self.chunk.instructions[cont.offset] =
                        Instruction::new_offset(Opcode::Jmp, rel);
                }

                self.free_registers_to(checkpoint);
                self.exit_scope();
            }
            _ => {}
        }
        Ok(())
    }

    /*
     * compile_pattern_binding -- bind `value_reg` to a destructuring pattern.
     *
     * WHY: `const { a, b } = obj` and `const [x, y] = arr` use Pattern nodes
     * instead of plain Identifier nodes. This helper handles all pattern kinds:
     *   Identifier: SetLocal/SetCapture/SetGlobal on the named slot
     *   Object: for each property, GetProp from value_reg, then recurse
     *   Array: for each element, GetElem[i] from value_reg, then recurse
     *   Assignment: check if value is undefined, use default if so
     *
     * kind: the VariableKind to use when declaring new identifiers.
     *
     * See: compile_var_declaration for how this is called
     * See: Statement::ForOf for loop variable binding (same pattern)
     */
    fn compile_pattern_binding(
        &mut self,
        pattern: &crate::parser::Pattern<'src, 'arena>,
        value_reg: Register,
        kind: VariableKind,
    ) -> CompileResult<()> {
        use crate::parser::{ObjectPatternProperty, Pattern, PropertyKey};
        match pattern {
            Pattern::Identifier(id) => {
                if kind != VariableKind::Var {
                    self.declare_var(id.name, kind, true);
                }
                if let Some((depth, slot)) = self.lookup_var(id.name) {
                    if depth == 0 {
                        self.emit(Instruction::new_rr(Opcode::SetLocal, slot, value_reg.0));
                    } else {
                        self.emit(Instruction::new_rrr(
                            Opcode::SetCapture,
                            depth,
                            slot,
                            value_reg.0,
                        ));
                    }
                } else {
                    let str_id = self.intern_string(id.raw);
                    let const_idx = self.chunk.add_constant(Constant::String(str_id));
                    self.emit(Instruction::new_ri(
                        Opcode::SetGlobal,
                        value_reg.0,
                        const_idx,
                    ));
                }
            }
            Pattern::Object(obj_pat) => {
                for prop in obj_pat.properties {
                    match prop {
                        ObjectPatternProperty::Property {
                            key,
                            value,
                            computed,
                            ..
                        } => {
                            let prop_reg = self.alloc_register();
                            if *computed {
                                let key_reg = if let PropertyKey::Computed(expr) = key {
                                    self.compile_expression(expr)?
                                } else {
                                    let r = self.alloc_register();
                                    self.emit(Instruction::new_r(Opcode::LoadUndefined, r.0));
                                    r
                                };
                                self.emit(Instruction::new_rrr(
                                    Opcode::GetElem,
                                    prop_reg.0,
                                    value_reg.0,
                                    key_reg.0,
                                ));
                            } else {
                                let prop_name = match key {
                                    PropertyKey::Identifier(id) => id.raw,
                                    PropertyKey::Literal(crate::parser::Literal::String(s)) => {
                                        s.value
                                    }
                                    _ => "",
                                };
                                let str_id = self.intern_string(prop_name);
                                let const_idx = self.chunk.add_constant(Constant::String(str_id));
                                self.emit(Instruction::new_rrr(
                                    Opcode::GetProp,
                                    prop_reg.0,
                                    value_reg.0,
                                    const_idx as u8,
                                ));
                            }
                            self.compile_pattern_binding(value, prop_reg, kind)?;
                        }
                        ObjectPatternProperty::Rest(rest) => {
                            // Rest element: bind rest of object -- simplified as the whole object
                            self.compile_pattern_binding(rest.argument, value_reg, kind)?;
                        }
                    }
                }
            }
            Pattern::Array(arr_pat) => {
                for (i, elem) in arr_pat.elements.iter().enumerate() {
                    if let Some(pat) = elem {
                        let elem_reg = self.alloc_register();
                        let idx_reg = self.alloc_register();
                        self.emit(Instruction::new_ri(Opcode::LoadSmi, idx_reg.0, i as u16));
                        self.emit(Instruction::new_rrr(
                            Opcode::GetElem,
                            elem_reg.0,
                            value_reg.0,
                            idx_reg.0,
                        ));
                        self.compile_pattern_binding(pat, elem_reg, kind)?;
                    }
                }
            }
            Pattern::Assignment(assign_pat) => {
                // `{ a = 5 }` -- use default if value is undefined
                // Emit: if value_reg === undefined, load default, else use value_reg
                let default_reg = self.compile_expression(assign_pat.right)?;
                let final_reg = self.alloc_register();
                // JmpNotNullish: if value_reg is not null/undefined, skip default
                let skip_default = self.emit(Instruction::new_r_offset(
                    Opcode::JmpNotNullish,
                    value_reg.0,
                    0,
                ));
                // Use default
                self.emit(Instruction::new_rr(Opcode::Mov, final_reg.0, default_reg.0));
                let skip_original = self.emit(Instruction::new_offset(Opcode::Jmp, 0));
                self.patch_jump(skip_default);
                // Use value_reg
                self.emit(Instruction::new_rr(Opcode::Mov, final_reg.0, value_reg.0));
                self.patch_jump(skip_original);
                self.compile_pattern_binding(assign_pat.left, final_reg, kind)?;
            }
            Pattern::Rest(rest) => {
                // Rest element: simplified -- bind the whole value
                self.compile_pattern_binding(rest.argument, value_reg, kind)?;
            }
        }
        Ok(())
    }

    fn compile_var_declaration(
        &mut self,
        decl: &VariableDeclaration<'src, 'arena>,
    ) -> CompileResult<()> {
        for declarator in decl.declarations {
            if let crate::parser::Pattern::Identifier(id) = &declarator.id {
                if decl.kind != VariableKind::Var {
                    self.declare_var(id.name, decl.kind, false);
                }

                if let Some(init) = declarator.init.as_ref() {
                    let value_reg = self.compile_expression(init)?;

                    if let Some((depth, slot)) = self.lookup_var(id.name) {
                        if depth == 0 {
                            self.emit(Instruction::new_rr(Opcode::SetLocal, slot, value_reg.0));
                        } else {
                            self.emit(Instruction::new_rrr(
                                Opcode::SetCapture,
                                depth,
                                slot,
                                value_reg.0,
                            ));
                        }
                    }

                    if let Some(binding) =
                        self.scopes[self.current_scope].bindings.get_mut(&id.name)
                    {
                        binding.initialized = true;
                    }
                } else if decl.kind == VariableKind::Var
                    && let Some((depth, slot)) = self.lookup_var(id.name)
                {
                    let reg = self.alloc_register();
                    self.emit(Instruction::new_r(Opcode::LoadUndefined, reg.0));
                    if depth == 0 {
                        self.emit(Instruction::new_rr(Opcode::SetLocal, slot, reg.0));
                    }
                }
            } else if let Some(init) = &declarator.init {
                // Destructuring pattern -- compile the RHS and bind via pattern
                let value_reg = self.compile_expression(init)?;
                self.compile_pattern_binding(&declarator.id, value_reg, decl.kind)?;
            }
        }
        Ok(())
    }

    // Expression compilation

    fn compile_expression(&mut self, expr: &Expression<'src, 'arena>) -> CompileResult<Register> {
        match expr {
            Expression::Literal(lit) => self.compile_literal(lit),
            Expression::Identifier(id) => self.compile_identifier(id),
            Expression::Binary(bin) => self.compile_binary(bin),
            Expression::Unary(unary) => self.compile_unary(unary),
            Expression::Assignment(assign) => self.compile_assignment(assign),
            Expression::Logical(logical) => self.compile_logical(logical),
            Expression::Conditional(cond) => self.compile_conditional(cond),
            Expression::Call(call) => self.compile_call(call),
            Expression::Member(member) => self.compile_member(member),
            Expression::Array(arr) => self.compile_array(arr),
            Expression::Object(obj) => self.compile_object(obj),
            Expression::Update(update) => self.compile_update(update),
            Expression::This(span) => {
                let reg = self.alloc_register();
                self.emit_at(
                    Instruction::new_rr(Opcode::GetLocal, reg.0, Register::THIS.0),
                    *span,
                );
                Ok(reg)
            }
            Expression::Parenthesized(paren) => self.compile_expression(paren.expression),
            Expression::Sequence(seq) => {
                let mut last_reg = self.alloc_register();
                for (i, e) in seq.expressions.iter().enumerate() {
                    if i == seq.expressions.len() - 1 {
                        last_reg = self.compile_expression(e)?;
                    } else {
                        let _ = self.compile_expression(e)?;
                    }
                }
                Ok(last_reg)
            }
            /*
             * Expression::Function -- function expression (IIFEs, callbacks, etc.)
             *
             * WHY: `!function(){}()`, `var f = function(){}`, `arr.map(function(){})`
             * all produce function expressions. The body is compiled into a separate
             * Chunk. The chunk index is stored as a Constant::Function, and
             * NewFunction emits a Value::Function at runtime.
             *
             * ROOT CAUSE FIX: Previously fell through to _ => LoadUndefined,
             * causing all function expressions (including IIFEs) to evaluate to
             * Undefined. This was the real cause of ChatGPT's TypeError("not a
             * function") on `!function(){...}()` patterns.
             *
             * See: op_new_function (vm/mod.rs) for runtime Function creation
             * See: op_call (vm/mod.rs) for function invocation
             */
            /*
             * Expression::Function -- compile function expression with shared string pool.
             *
             * WHY: Creating a fresh Compiler::new() gives the child its own
             * string pool starting at ID 0. These child string IDs collide with
             * the parent's IDs when main.rs builds str_map from the parent pool
             * alone. Using new_with_pool() seeds the child with the parent's
             * existing pool, so new strings get unique IDs that are valid in
             * the parent context.
             *
             * After compilation, into_parts() returns the child's chunk,
             * any nested function chunks, and the merged pool. We merge the pool
             * back into self so future intern calls produce consistent IDs.
             * Nested function chunks (grandchildren) are added to self.child_chunks
             * before the child chunk itself; their Function constants are offset
             * by base_offset to be parent-relative.
             *
             * See: new_with_pool for the shared pool constructor
             * See: into_parts for the chunk+pool extraction
             */
            Expression::Function(func) => {
                let result_reg = self.alloc_register();
                let parent_locals = self.collect_locals_snapshot();
                let mut child = Compiler::new_with_pool_and_parent(
                    self.string_pool.clone(),
                    self.next_string_id,
                    parent_locals,
                );
                // Propagate the async marker so Statement::Return inside the
                // body emits AsyncReturn instead of Ret.
                child.is_async_function = func.is_async;
                // Declare parameters in child scope (two-pass; see FunctionDeclaration)
                let mut destructuring_params = Vec::new();
                for param in func.params {
                    if let crate::parser::Pattern::Identifier(id) = param {
                        child.declare_var(id.name, VariableKind::Let, true);
                    } else {
                        let slot = child.alloc_register();
                        destructuring_params.push((slot.0, param));
                    }
                }
                for &(slot, pattern) in &destructuring_params {
                    child.compile_pattern_binding(pattern, Register(slot), VariableKind::Let)?;
                }
                // Hoist var declarations and function declarations in the body
                child.collect_declarations(func.body.body);
                for stmt in func.body.body {
                    child.compile_statement(stmt)?;
                }
                // Implicit fall-off-the-end return -- async wraps in
                // Promise.resolve(undefined); see Statement::FunctionDeclaration
                // for the same pattern.
                if func.is_async {
                    let undef_reg = child.alloc_register();
                    child
                        .chunk
                        .emit(Instruction::new_r(Opcode::LoadUndefined, undef_reg.0));
                    child
                        .chunk
                        .emit(Instruction::new_r(Opcode::AsyncReturn, undef_reg.0));
                } else {
                    child.chunk.emit(Instruction::new(Opcode::RetUndefined));
                }
                let (mut child_chunk, nested, merged_pool, merged_next, child_upvalues) =
                    child.into_parts();
                // Merge child string pool additions back into parent
                self.string_pool = merged_pool;
                self.next_string_id = merged_next;
                /*
                 * Flag the child chunk as a generator body so the VM's
                 * op_call diverts invocations to invoke_generator.  See the
                 * matching block in Statement::FunctionDeclaration for the
                 * full rationale.
                 */
                if func.is_generator {
                    child_chunk.is_generator = true;
                }
                // Flatten nested function chunks into parent's child_chunks.
                // base_offset is where nested[0] will sit in self.child_chunks.
                let base_offset = self.child_chunks.len() as u32;
                let n_nested = nested.len() as u32;
                for nc in nested {
                    self.child_chunks.push(nc);
                }
                // Remap Function constants in child_chunk from child-local indices
                // (0..n_nested) to parent-local indices (base_offset..base_offset+n_nested).
                for constant in child_chunk.constants_mut() {
                    if let Constant::Function(idx) = constant {
                        *idx += base_offset;
                    }
                }
                let func_idx = base_offset + n_nested;
                self.child_chunks.push(child_chunk);
                let const_idx = self.chunk.add_constant(Constant::Function(func_idx));
                /*
                 * Emit NewGenerator for `function*` expressions; see the
                 * matching emission in Statement::FunctionDeclaration for
                 * the full rationale.
                 */
                let new_op = if func.is_generator {
                    Opcode::NewGenerator
                } else {
                    Opcode::NewFunction
                };
                self.emit(Instruction::new_ri(new_op, result_reg.0, const_idx));
                // BindCapture per upvalue -- see FunctionDeclaration for rationale.
                for upvalue in &child_upvalues {
                    self.emit(Instruction::new_rr(
                        Opcode::BindCapture,
                        result_reg.0,
                        upvalue.parent_slot,
                    ));
                }
                Ok(result_reg)
            }
            /*
             * Expression::Arrow -- same shared-pool fix as Expression::Function.
             * Arrow functions capture lexical `this` but otherwise compile the
             * same way for our purposes (ChatGPT scripts don't use `this`).
             */
            Expression::Arrow(arrow) => {
                let result_reg = self.alloc_register();
                let parent_locals = self.collect_locals_snapshot();
                let mut child = Compiler::new_with_pool_and_parent(
                    self.string_pool.clone(),
                    self.next_string_id,
                    parent_locals,
                );
                // Propagate the async marker so Statement::Return inside the
                // body emits AsyncReturn, and so the implicit fall-off-the-end
                // return wraps the value in Promise.resolve.
                child.is_async_function = arrow.is_async;
                // Two-pass parameter binding -- same as FunctionDeclaration/Expression
                let mut destructuring_params = Vec::new();
                for param in arrow.params {
                    if let crate::parser::Pattern::Identifier(id) = param {
                        child.declare_var(id.name, VariableKind::Let, true);
                    } else {
                        let slot = child.alloc_register();
                        destructuring_params.push((slot.0, param));
                    }
                }
                for &(slot, pattern) in &destructuring_params {
                    child.compile_pattern_binding(pattern, Register(slot), VariableKind::Let)?;
                }
                match &arrow.body {
                    crate::parser::ArrowBody::Expression(expr) => {
                        let val_reg = child.compile_expression(expr)?;
                        // Concise-body arrow: the expression IS the return
                        // value.  Async arrows must wrap in Promise.resolve.
                        let op = if arrow.is_async {
                            Opcode::AsyncReturn
                        } else {
                            Opcode::Ret
                        };
                        child.chunk.emit(Instruction::new_r(op, val_reg.0));
                    }
                    crate::parser::ArrowBody::Block(block) => {
                        child.collect_declarations(block.body);
                        for stmt in block.body {
                            child.compile_statement(stmt)?;
                        }
                        if arrow.is_async {
                            let undef_reg = child.alloc_register();
                            child
                                .chunk
                                .emit(Instruction::new_r(Opcode::LoadUndefined, undef_reg.0));
                            child
                                .chunk
                                .emit(Instruction::new_r(Opcode::AsyncReturn, undef_reg.0));
                        } else {
                            child.chunk.emit(Instruction::new(Opcode::RetUndefined));
                        }
                    }
                }
                let (mut child_chunk, nested, merged_pool, merged_next, child_upvalues) =
                    child.into_parts();
                self.string_pool = merged_pool;
                self.next_string_id = merged_next;
                let base_offset = self.child_chunks.len() as u32;
                let n_nested = nested.len() as u32;
                for nc in nested {
                    self.child_chunks.push(nc);
                }
                for constant in child_chunk.constants_mut() {
                    if let Constant::Function(idx) = constant {
                        *idx += base_offset;
                    }
                }
                let func_idx = base_offset + n_nested;
                self.child_chunks.push(child_chunk);
                let const_idx = self.chunk.add_constant(Constant::Function(func_idx));
                self.emit(Instruction::new_ri(
                    Opcode::NewFunction,
                    result_reg.0,
                    const_idx,
                ));
                // BindCapture per upvalue -- arrow closures capture lexical
                // scope identically to function expressions for our purposes.
                for upvalue in &child_upvalues {
                    self.emit(Instruction::new_rr(
                        Opcode::BindCapture,
                        result_reg.0,
                        upvalue.parent_slot,
                    ));
                }
                Ok(result_reg)
            }
            /*
             * Expression::TemplateLiteral -- `` `hello ${name}` ``
             *
             * WHY: Template literals are pervasive in modern JS for string
             * interpolation. Without compilation they evaluate to Undefined,
             * breaking any code that uses them.
             *
             * Compile as a chain of Add operations:
             *   result = quasi[0] + expr[0] + quasi[1] + expr[1] + ... + quasi[n]
             *
             * Since Add(string, anything) = string concat in our VM, this
             * correctly converts each interpolated value via JS ToString rules.
             * Empty quasi strings are elided to reduce Add instructions.
             */
            Expression::TemplateLiteral(tmpl) => {
                let result_reg = self.alloc_register();
                let mut acc_reg = result_reg;
                let mut initialized = false;

                let n_quasis = tmpl.quasis.len();
                let n_exprs = tmpl.expressions.len();

                for i in 0..n_quasis {
                    let quasi = &tmpl.quasis[i];
                    let cooked = quasi.cooked.unwrap_or(quasi.raw);

                    // Load the quasi string (may be empty)
                    if !cooked.is_empty() || !initialized {
                        let str_id = self.intern_string(cooked);
                        let const_idx = self.chunk.add_constant(Constant::String(str_id));
                        let quasi_reg = self.alloc_register();
                        self.emit(Instruction::new_ri(
                            Opcode::LoadConst,
                            quasi_reg.0,
                            const_idx,
                        ));
                        if initialized {
                            // Concat: acc = acc + quasi
                            let new_reg = self.alloc_register();
                            self.emit(Instruction::new_rrr(
                                Opcode::Add,
                                new_reg.0,
                                acc_reg.0,
                                quasi_reg.0,
                            ));
                            acc_reg = new_reg;
                        } else {
                            // First piece: just move to result
                            self.emit(Instruction::new_rr(Opcode::Mov, acc_reg.0, quasi_reg.0));
                            initialized = true;
                        }
                    }

                    // Add the interpolated expression if present
                    if i < n_exprs {
                        let expr_reg = self.compile_expression(&tmpl.expressions[i])?;
                        if initialized {
                            let new_reg = self.alloc_register();
                            self.emit(Instruction::new_rrr(
                                Opcode::Add,
                                new_reg.0,
                                acc_reg.0,
                                expr_reg.0,
                            ));
                            acc_reg = new_reg;
                        } else {
                            self.emit(Instruction::new_rr(Opcode::Mov, acc_reg.0, expr_reg.0));
                            initialized = true;
                        }
                    }
                }

                if !initialized {
                    // Empty template literal ``
                    let str_id = self.intern_string("");
                    let const_idx = self.chunk.add_constant(Constant::String(str_id));
                    self.emit(Instruction::new_ri(Opcode::LoadConst, acc_reg.0, const_idx));
                } else if acc_reg != result_reg {
                    self.emit(Instruction::new_rr(Opcode::Mov, result_reg.0, acc_reg.0));
                }
                Ok(result_reg)
            }
            /*
             * Expression::TaggedTemplate -- fn`template`
             *
             * WHY: Tagged templates call a tag function with (strings_array, ...values).
             * Simplified: compile the quasi strings as an array, then call the tag function.
             * Used by some libraries (e.g. graphql tag, styled-components, etc.).
             */
            Expression::TaggedTemplate(tagged) => {
                let result_reg = self.alloc_register();
                let tag_reg = self.compile_expression(tagged.tag)?;

                // Build the strings array from quasis
                let strings_reg = self.alloc_register();
                self.emit(Instruction::new_r(Opcode::NewArray, strings_reg.0));
                for (i, quasi) in tagged.quasi.quasis.iter().enumerate() {
                    let cooked = quasi.cooked.unwrap_or(quasi.raw);
                    let str_id = self.intern_string(cooked);
                    let const_idx = self.chunk.add_constant(Constant::String(str_id));
                    let str_reg = self.alloc_register();
                    self.emit(Instruction::new_ri(Opcode::LoadConst, str_reg.0, const_idx));
                    let idx_reg = self.alloc_register();
                    self.emit(Instruction::new_ri(Opcode::LoadSmi, idx_reg.0, i as u16));
                    self.emit(Instruction::new_rrr(
                        Opcode::SetElem,
                        strings_reg.0,
                        idx_reg.0,
                        str_reg.0,
                    ));
                }

                // Compile the interpolated values as additional args after strings_reg
                for expr in tagged.quasi.expressions {
                    let _ = self.compile_expression(expr)?;
                }
                let argc = 1 + tagged.quasi.expressions.len() as u8;
                self.emit_at(
                    Instruction::new_rrr(Opcode::Call, result_reg.0, tag_reg.0, argc),
                    tagged.span,
                );
                Ok(result_reg)
            }
            Expression::Class(_) => {
                let reg = self.alloc_register();
                self.emit(Instruction::new_r(Opcode::NewObject, reg.0));
                Ok(reg)
            }
            /*
             * Expression::New -- `new Constructor(args)`.
             *
             * For NativeFunction constructors (ReadableStream, Error, etc.),
             * this compiles as a regular Call. The constructor NativeFunction
             * returns the constructed object.
             *
             * TODO: For JS function constructors, should create a new object,
             * set its prototype, call the constructor with `this` bound to
             * the new object, and return the object.
             */
            Expression::New(new_expr) => {
                let result_reg = self.alloc_register();
                let callee_reg = self.compile_expression(new_expr.callee)?;
                let arg_base = self.next_register;
                for arg in new_expr.arguments {
                    match arg {
                        Argument::Expression(expr) => {
                            let _ = self.compile_expression(expr)?;
                        }
                        Argument::Spread(_) => {}
                    }
                }
                let argc = new_expr.arguments.len() as u8;
                self.emit_at(
                    Instruction::new_rrr(Opcode::Call, result_reg.0, callee_reg.0, argc),
                    new_expr.span,
                );
                self.free_registers_to(arg_base);
                Ok(result_reg)
            }
            /*
             * Expression::Await -- `await expr` inside an async function.
             *
             * WHY: Without compilation `await x` evaluated to undefined and
             * any code that depended on the resolved value silently saw
             * undefined.  Now we compile the inner expression, allocate a
             * destination register, and emit Await(dst, src) which the VM
             * resolves synchronously: a Promise wrapper has its current state
             * read via the INTERNAL_SLOT_KEY introspect function and either
             * the fulfillment value lands in dst, the rejection reason is
             * thrown, or (for a non-Promise value) the value passes through.
             *
             * Note: this is a synchronous-await model -- the frame does not
             * suspend; pending Promises currently resolve to undefined.  Real
             * suspending await requires resumable frames (out of scope here;
             * see vm/mod.rs op_await for the full rationale).
             */
            Expression::Await(await_expr) => {
                let value_reg = self.compile_expression(await_expr.argument)?;
                let result_reg = self.alloc_register();
                self.emit_at(
                    Instruction::new_rr(Opcode::Await, result_reg.0, value_reg.0),
                    await_expr.span,
                );
                Ok(result_reg)
            }
            /*
             * Expression::Yield -- `yield expr` or `yield* expr` inside a
             * generator function body.
             *
             * WHY: The VM's op_yield appends r[src] to the active generator
             * yield buffer (top of vm.generator_yield_stack), and op_yield_star
             * delegates: it drains an iterable into the same buffer.  The
             * compiler's job is to:
             *   1. Evaluate the argument (or LoadUndefined for bare `yield`).
             *   2. Allocate a destination register for the yield expression's
             *      own value (always undefined in eager mode -- documented
             *      limitation: .next(value) is not plumbed back).
             *   3. Emit Yield(dst, src) or YieldStar(dst, src) depending on
             *      whether `yield*` (delegate) is in play.
             *
             * Yield only makes sense inside a generator body; the parser
             * accepts it everywhere syntactically, but at runtime a stray
             * Yield with no buffer on the stack silently no-ops (and the
             * dst register still receives undefined).
             *
             * See: vm/mod.rs op_yield / op_yield_star.
             * See: vm/generator.rs build_generator and yield_star_flatten.
             */
            Expression::Yield(yield_expr) => {
                // Source register: argument value (or undefined for `yield;`).
                let src_reg = if let Some(arg) = &yield_expr.argument {
                    self.compile_expression(arg)?
                } else {
                    let reg = self.alloc_register();
                    self.emit_at(
                        Instruction::new_r(Opcode::LoadUndefined, reg.0),
                        yield_expr.span,
                    );
                    reg
                };
                let dst_reg = self.alloc_register();
                let op = if yield_expr.delegate {
                    Opcode::YieldStar
                } else {
                    Opcode::Yield
                };
                self.emit_at(
                    Instruction::new_rr(op, dst_reg.0, src_reg.0),
                    yield_expr.span,
                );
                Ok(dst_reg)
            }
            _ => {
                let reg = self.alloc_register();
                self.emit(Instruction::new_r(Opcode::LoadUndefined, reg.0));
                Ok(reg)
            }
        }
    }

    fn compile_literal(&mut self, lit: &Literal<'src>) -> CompileResult<Register> {
        let reg = self.alloc_register();

        match lit {
            Literal::Null(span) => {
                self.emit_at(Instruction::new_r(Opcode::LoadNull, reg.0), *span);
            }
            Literal::Boolean(b) => {
                let op = if b.value {
                    Opcode::LoadTrue
                } else {
                    Opcode::LoadFalse
                };
                self.emit_at(Instruction::new_r(op, reg.0), b.span);
            }
            Literal::Number(n) => {
                if n.value.fract() == 0.0
                    && n.value >= f64::from(i16::MIN)
                    && n.value <= f64::from(i16::MAX)
                {
                    let smi = n.value as i16;
                    if smi == 0 {
                        self.emit_at(Instruction::new_r(Opcode::LoadZero, reg.0), n.span);
                    } else if smi == 1 {
                        self.emit_at(Instruction::new_r(Opcode::LoadOne, reg.0), n.span);
                    } else if smi == -1 {
                        self.emit_at(Instruction::new_r(Opcode::LoadMinusOne, reg.0), n.span);
                    } else {
                        self.emit_at(
                            Instruction::new_ri(Opcode::LoadSmi, reg.0, smi as u16),
                            n.span,
                        );
                    }
                } else {
                    let idx = self.chunk.add_number(n.value);
                    self.emit_at(Instruction::new_ri(Opcode::LoadConst, reg.0, idx), n.span);
                }
            }
            Literal::String(s) => {
                let str_id = self.intern_string(s.value);
                let idx = self.chunk.add_constant(Constant::String(str_id));
                self.emit_at(Instruction::new_ri(Opcode::LoadConst, reg.0, idx), s.span);
            }
            Literal::RegExp(r) => {
                let const_idx = self.chunk.add_constant(Constant::RegExp {
                    pattern: 0,
                    flags: 0,
                });
                self.emit_at(
                    Instruction::new_ri(Opcode::NewRegExp, reg.0, const_idx),
                    r.span,
                );
            }
            Literal::BigInt(b) => {
                let const_idx = self.chunk.add_constant(Constant::BigInt(Vec::new()));
                self.emit_at(
                    Instruction::new_ri(Opcode::LoadConst, reg.0, const_idx),
                    b.span,
                );
            }
        }
        Ok(reg)
    }

    fn compile_identifier(&mut self, id: &Identifier<'src>) -> CompileResult<Register> {
        let reg = self.alloc_register();

        if let Some((depth, slot)) = self.lookup_var(id.name) {
            if depth == 0 {
                self.emit_at(Instruction::new_rr(Opcode::GetLocal, reg.0, slot), id.span);
            } else {
                self.emit_at(
                    Instruction::new_rrr(Opcode::GetCapture, reg.0, depth, slot),
                    id.span,
                );
            }
        } else if let Some(captures_idx) = self.resolve_upvalue(id.name) {
            // depth=0 dispatches op_get_capture into its upvalue mode and
            // reads CallFrame.captures[captures_idx].
            self.emit_at(
                Instruction::new_rrr(Opcode::GetCapture, reg.0, 0, captures_idx),
                id.span,
            );
        } else {
            let str_id = self.intern_string(id.raw);
            let name_idx = self.chunk.add_constant(Constant::String(str_id));
            self.emit_at(
                Instruction::new_ri(Opcode::GetGlobal, reg.0, name_idx),
                id.span,
            );
        }
        Ok(reg)
    }

    fn compile_binary(
        &mut self,
        bin: &crate::parser::BinaryExpression<'src, 'arena>,
    ) -> CompileResult<Register> {
        let left_reg = self.compile_expression(bin.left)?;
        let right_reg = self.compile_expression(bin.right)?;
        let result_reg = self.alloc_register();

        let opcode = match bin.operator {
            BinaryOperator::Add => Opcode::Add,
            BinaryOperator::Sub => Opcode::Sub,
            BinaryOperator::Mul => Opcode::Mul,
            BinaryOperator::Div => Opcode::Div,
            BinaryOperator::Mod => Opcode::Mod,
            BinaryOperator::Pow => Opcode::Pow,
            BinaryOperator::Eq => Opcode::Eq,
            BinaryOperator::Ne => Opcode::Ne,
            BinaryOperator::StrictEq => Opcode::StrictEq,
            BinaryOperator::StrictNe => Opcode::StrictNe,
            BinaryOperator::Lt => Opcode::Lt,
            BinaryOperator::Le => Opcode::Le,
            BinaryOperator::Gt => Opcode::Gt,
            BinaryOperator::Ge => Opcode::Ge,
            BinaryOperator::BitwiseAnd => Opcode::BitAnd,
            BinaryOperator::BitwiseOr => Opcode::BitOr,
            BinaryOperator::BitwiseXor => Opcode::BitXor,
            BinaryOperator::ShiftLeft => Opcode::Shl,
            BinaryOperator::ShiftRight => Opcode::Shr,
            BinaryOperator::UnsignedShiftRight => Opcode::Ushr,
            BinaryOperator::In => Opcode::In,
            BinaryOperator::InstanceOf => Opcode::Instanceof,
        };

        self.emit_at(
            Instruction::new_rrr(opcode, result_reg.0, left_reg.0, right_reg.0),
            bin.span,
        );
        Ok(result_reg)
    }

    fn compile_unary(
        &mut self,
        unary: &crate::parser::UnaryExpression<'src, 'arena>,
    ) -> CompileResult<Register> {
        let arg_reg = self.compile_expression(unary.argument)?;
        let result_reg = self.alloc_register();

        let opcode = match unary.operator {
            UnaryOperator::Minus => Opcode::Neg,
            UnaryOperator::Plus => {
                self.emit(Instruction::new_rr(Opcode::Mov, result_reg.0, arg_reg.0));
                return Ok(result_reg);
            }
            UnaryOperator::Not => Opcode::Not,
            UnaryOperator::BitwiseNot => Opcode::BitNot,
            UnaryOperator::Typeof => Opcode::Typeof,
            UnaryOperator::Void => {
                self.emit(Instruction::new_r(Opcode::LoadUndefined, result_reg.0));
                return Ok(result_reg);
            }
            UnaryOperator::Delete => {
                self.emit(Instruction::new_r(Opcode::LoadTrue, result_reg.0));
                return Ok(result_reg);
            }
        };

        self.emit_at(
            Instruction::new_rr(opcode, result_reg.0, arg_reg.0),
            unary.span,
        );
        Ok(result_reg)
    }

    fn compile_logical(
        &mut self,
        logical: &crate::parser::LogicalExpression<'src, 'arena>,
    ) -> CompileResult<Register> {
        let left_reg = self.compile_expression(logical.left)?;
        let result_reg = self.alloc_register();

        self.emit(Instruction::new_rr(Opcode::Mov, result_reg.0, left_reg.0));

        let skip_right = match logical.operator {
            LogicalOperator::And => {
                self.emit(Instruction::new_r_offset(Opcode::JmpFalse, result_reg.0, 0))
            }
            LogicalOperator::Or => {
                self.emit(Instruction::new_r_offset(Opcode::JmpTrue, result_reg.0, 0))
            }
            LogicalOperator::NullishCoalescing => self.emit(Instruction::new_r_offset(
                Opcode::JmpNotNullish,
                result_reg.0,
                0,
            )),
        };

        let right_reg = self.compile_expression(logical.right)?;
        self.emit(Instruction::new_rr(Opcode::Mov, result_reg.0, right_reg.0));
        self.patch_jump(skip_right);

        Ok(result_reg)
    }

    fn compile_conditional(
        &mut self,
        cond: &crate::parser::ConditionalExpression<'src, 'arena>,
    ) -> CompileResult<Register> {
        let test_reg = self.compile_expression(cond.test)?;
        let result_reg = self.alloc_register();

        let else_jump = self.emit(Instruction::new_r_offset(Opcode::JmpFalse, test_reg.0, 0));
        let cons_reg = self.compile_expression(cond.consequent)?;
        self.emit(Instruction::new_rr(Opcode::Mov, result_reg.0, cons_reg.0));
        let end_jump = self.emit(Instruction::new_offset(Opcode::Jmp, 0));

        self.patch_jump(else_jump);
        let alt_reg = self.compile_expression(cond.alternate)?;
        self.emit(Instruction::new_rr(Opcode::Mov, result_reg.0, alt_reg.0));
        self.patch_jump(end_jump);

        Ok(result_reg)
    }

    fn compile_assignment(
        &mut self,
        assign: &crate::parser::AssignmentExpression<'src, 'arena>,
    ) -> CompileResult<Register> {
        let value_reg = self.compile_expression(assign.right)?;
        let result_reg = self.alloc_register();

        let final_value = if assign.operator == AssignmentOperator::Assign {
            value_reg
        } else {
            let current_reg = match &assign.left {
                AssignmentTarget::Identifier(id) => self.compile_identifier(id)?,
                AssignmentTarget::Member(member) => self.compile_member(member)?,
                AssignmentTarget::Pattern(_) => {
                    self.errors.push(CompileError::new(
                        "pattern assignment not yet supported",
                        assign.span,
                    ));
                    return Ok(result_reg);
                }
            };

            let computed_reg = self.alloc_register();
            let opcode = match assign.operator {
                AssignmentOperator::AddAssign => Opcode::Add,
                AssignmentOperator::SubAssign => Opcode::Sub,
                AssignmentOperator::MulAssign => Opcode::Mul,
                AssignmentOperator::DivAssign => Opcode::Div,
                AssignmentOperator::ModAssign => Opcode::Mod,
                AssignmentOperator::PowAssign => Opcode::Pow,
                AssignmentOperator::BitwiseAndAssign => Opcode::BitAnd,
                AssignmentOperator::BitwiseOrAssign => Opcode::BitOr,
                AssignmentOperator::BitwiseXorAssign => Opcode::BitXor,
                AssignmentOperator::ShiftLeftAssign => Opcode::Shl,
                AssignmentOperator::ShiftRightAssign => Opcode::Shr,
                AssignmentOperator::UnsignedShiftRightAssign => Opcode::Ushr,
                /*
                 * Logical assignment operators (&&=, ||=, ??=) should
                 * short-circuit, but for now we evaluate the RHS unconditionally
                 * and assign. This is semantically close enough for ChatGPT's
                 * `window.ReactQueryError ??= class ReactQueryError extends Error {}`
                 * which just needs to assign if the LHS is nullish.
                 *
                 * TODO: Implement proper short-circuit via conditional jump.
                 * See: Opcode::JmpNullish for ??= conditional skip
                 *
                 * AssignmentOperator::Assign is handled by the outer `if` branch above;
                 * reaching this arm with Assign is unreachable in practice, but the
                 * compiler cannot prove it so we list it explicitly rather than using `_`.
                 */
                AssignmentOperator::NullishAssign
                | AssignmentOperator::LogicalAndAssign
                | AssignmentOperator::LogicalOrAssign
                | AssignmentOperator::Assign => Opcode::Mov,
            };

            self.emit(Instruction::new_rrr(
                opcode,
                computed_reg.0,
                current_reg.0,
                value_reg.0,
            ));
            computed_reg
        };

        match &assign.left {
            AssignmentTarget::Identifier(id) => {
                if let Some((depth, slot)) = self.lookup_var(id.name) {
                    if depth == 0 {
                        self.emit(Instruction::new_rr(Opcode::SetLocal, slot, final_value.0));
                    } else {
                        self.emit(Instruction::new_rrr(
                            Opcode::SetCapture,
                            depth,
                            slot,
                            final_value.0,
                        ));
                    }
                } else if let Some(captures_idx) = self.resolve_upvalue(id.name) {
                    // depth=0 selects op_set_capture's upvalue mode, which
                    // writes into CallFrame.captures[captures_idx].
                    self.emit(Instruction::new_rrr(
                        Opcode::SetCapture,
                        0,
                        captures_idx,
                        final_value.0,
                    ));
                } else {
                    let str_id = self.intern_string(id.raw);
                    let name_idx = self.chunk.add_constant(Constant::String(str_id));
                    self.emit(Instruction::new_ri(
                        Opcode::SetGlobal,
                        final_value.0,
                        name_idx,
                    ));
                }
            }
            AssignmentTarget::Member(member) => {
                let obj_reg = self.compile_expression(member.object)?;
                if member.computed {
                    let key_reg = self.compile_expression(member.property)?;
                    self.emit(Instruction::new_rrr(
                        Opcode::SetElem,
                        obj_reg.0,
                        key_reg.0,
                        final_value.0,
                    ));
                } else {
                    let prop_name = match member.property {
                        Expression::Identifier(id) => id.raw,
                        _ => "",
                    };
                    let str_id = self.intern_string(prop_name);
                    let name_idx = self.chunk.add_constant(Constant::String(str_id));
                    self.emit(Instruction::new_rrr(
                        Opcode::SetProp,
                        obj_reg.0,
                        name_idx as u8,
                        final_value.0,
                    ));
                }
            }
            AssignmentTarget::Pattern(_) => {}
        }

        self.emit(Instruction::new_rr(
            Opcode::Mov,
            result_reg.0,
            final_value.0,
        ));
        Ok(result_reg)
    }

    fn compile_update(
        &mut self,
        update: &crate::parser::UpdateExpression<'src, 'arena>,
    ) -> CompileResult<Register> {
        let result_reg = self.alloc_register();

        if let Expression::Identifier(id) = update.argument {
            let current_reg = self.compile_identifier(id)?;

            if !update.prefix {
                self.emit(Instruction::new_rr(
                    Opcode::Mov,
                    result_reg.0,
                    current_reg.0,
                ));
            }

            let updated_reg = self.alloc_register();
            let opcode = match update.operator {
                UpdateOperator::Increment => Opcode::Inc,
                UpdateOperator::Decrement => Opcode::Dec,
            };
            self.emit(Instruction::new_rr(opcode, updated_reg.0, current_reg.0));

            /*
             * Write the updated value BACK to the binding.
             *
             * WHY: Without this write, `count++` inside a closure would
             * read count, compute count+1, but never propagate the result
             * to the slot/capture/global -- so the next read sees the same
             * stale value. This was the silent half of the closure-counter
             * bug: even after JsFunction.captures became Rc-shared
             * (vm/value.rs), the captured slot was never updated by `++`.
             *
             * Mirror compile_assignment's three-way dispatch:
             *   1. local / intra-function block-scope binding -- SetLocal
             *      (depth 0) or SetCapture (depth >= 1).
             *   2. captured-from-parent upvalue -- SetCapture(depth=0,
             *      captures_idx). depth=0 selects op_set_capture's upvalue
             *      mode against CallFrame.captures.
             *   3. unresolved -- SetGlobal by interned name. Matches the
             *      read side in compile_identifier.
             *
             * See: compile_assignment for the canonical write dispatch.
             */
            if let Some((depth, slot)) = self.lookup_var(id.name) {
                if depth == 0 {
                    self.emit(Instruction::new_rr(Opcode::SetLocal, slot, updated_reg.0));
                } else {
                    self.emit(Instruction::new_rrr(
                        Opcode::SetCapture,
                        depth,
                        slot,
                        updated_reg.0,
                    ));
                }
            } else if let Some(captures_idx) = self.resolve_upvalue(id.name) {
                self.emit(Instruction::new_rrr(
                    Opcode::SetCapture,
                    0,
                    captures_idx,
                    updated_reg.0,
                ));
            } else {
                let str_id = self.intern_string(id.raw);
                let name_idx = self.chunk.add_constant(Constant::String(str_id));
                self.emit(Instruction::new_ri(
                    Opcode::SetGlobal,
                    updated_reg.0,
                    name_idx,
                ));
            }

            if update.prefix {
                self.emit(Instruction::new_rr(
                    Opcode::Mov,
                    result_reg.0,
                    updated_reg.0,
                ));
            }
        } else {
            self.errors
                .push(CompileError::new("invalid update target", update.span));
        }

        Ok(result_reg)
    }

    fn compile_call(
        &mut self,
        call: &crate::parser::CallExpression<'src, 'arena>,
    ) -> CompileResult<Register> {
        let result_reg = self.alloc_register();
        let callee_reg = self.compile_expression(call.callee)?;

        let has_spread = call
            .arguments
            .iter()
            .any(|a| matches!(a, Argument::Spread(_)));

        if has_spread {
            /*
             * Spread call: f(a, b, ...rest) or f(...arr)
             *
             * WHY: argc is unknown at compile time when spread args are present.
             * Strategy: build a single args array containing all arguments
             * (spreading the spread arguments inline), then emit SpreadCall.
             *
             * Pattern:
             *   args_arr = []          (NewArray)
             *   push(args_arr, a)      (for each normal arg)
             *   concat(args_arr, rest) (for each spread arg -- args_arr = args_arr.concat(rest))
             *   SpreadCall(result, callee, args_arr)
             */
            let args_reg = self.alloc_register();
            self.emit(Instruction::new_r(Opcode::NewArray, args_reg.0));
            // Seed length = 0
            let zero_reg = self.alloc_register();
            self.emit(Instruction::new(Opcode::LoadZero));
            self.emit(Instruction::new_rr(Opcode::Mov, zero_reg.0, 0));

            for arg in call.arguments {
                match arg {
                    Argument::Expression(expr) => {
                        // args_arr.push(val)
                        let val_reg = self.compile_expression(expr)?;
                        let push_fn_reg = self.alloc_register();
                        let push_str_id = self.intern_string("push");
                        let push_const = self.chunk.add_constant(Constant::String(push_str_id));
                        self.emit(Instruction::new_rrr(
                            Opcode::GetProp,
                            push_fn_reg.0,
                            args_reg.0,
                            push_const as u8,
                        ));
                        // Call push: callee=push_fn, arg1=val (laid out after push_fn_reg)
                        let arg_slot = self.alloc_register();
                        self.emit(Instruction::new_rr(Opcode::Mov, arg_slot.0, val_reg.0));
                        self.emit(Instruction::new_rrr(
                            Opcode::Call,
                            zero_reg.0,
                            push_fn_reg.0,
                            1,
                        ));
                    }
                    Argument::Spread(spread_elem) => {
                        // args_arr = args_arr.concat(spread_val)
                        let spread_reg = self.compile_expression(spread_elem.argument)?;
                        let concat_fn_reg = self.alloc_register();
                        let concat_str_id = self.intern_string("concat");
                        let concat_const = self.chunk.add_constant(Constant::String(concat_str_id));
                        self.emit(Instruction::new_rrr(
                            Opcode::GetProp,
                            concat_fn_reg.0,
                            args_reg.0,
                            concat_const as u8,
                        ));
                        let arg_slot = self.alloc_register();
                        self.emit(Instruction::new_rr(Opcode::Mov, arg_slot.0, spread_reg.0));
                        self.emit(Instruction::new_rrr(
                            Opcode::Call,
                            args_reg.0,
                            concat_fn_reg.0,
                            1,
                        ));
                    }
                }
            }

            self.emit_at(
                Instruction::new_rrr(Opcode::SpreadCall, result_reg.0, callee_reg.0, args_reg.0),
                call.span,
            );
            self.free_registers_to(self.next_register);
        } else {
            let arg_base = self.next_register;
            for arg in call.arguments {
                if let Argument::Expression(expr) = arg {
                    let _ = self.compile_expression(expr)?;
                }
            }
            let argc = call.arguments.len() as u8;
            self.emit_at(
                Instruction::new_rrr(Opcode::Call, result_reg.0, callee_reg.0, argc),
                call.span,
            );
            self.free_registers_to(arg_base);
        }

        Ok(result_reg)
    }

    fn compile_member(
        &mut self,
        member: &crate::parser::MemberExpression<'src, 'arena>,
    ) -> CompileResult<Register> {
        let obj_reg = self.compile_expression(member.object)?;
        let result_reg = self.alloc_register();

        if member.computed {
            let key_reg = self.compile_expression(member.property)?;
            self.emit_at(
                Instruction::new_rrr(Opcode::GetElem, result_reg.0, obj_reg.0, key_reg.0),
                member.span,
            );
        } else {
            // Extract property name from the AST identifier
            let prop_name = match member.property {
                Expression::Identifier(id) => id.raw,
                _ => "",
            };
            let str_id = self.intern_string(prop_name);
            let name_idx = self.chunk.add_constant(Constant::String(str_id));
            self.emit_at(
                Instruction::new_rrr(Opcode::GetProp, result_reg.0, obj_reg.0, name_idx as u8),
                member.span,
            );
        }

        Ok(result_reg)
    }

    fn compile_array(
        &mut self,
        arr: &crate::parser::ArrayExpression<'src, 'arena>,
    ) -> CompileResult<Register> {
        let result_reg = self.alloc_register();
        let len = arr.elements.len() as u16;

        self.emit_at(
            Instruction::new_ri(Opcode::NewArray, result_reg.0, len),
            arr.span,
        );

        for (i, elem) in arr.elements.iter().enumerate() {
            match elem {
                ArrayElement::Expression(expr) => {
                    let val_reg = self.compile_expression(expr)?;
                    let idx_reg = self.alloc_register();
                    self.emit(Instruction::new_ri(Opcode::LoadSmi, idx_reg.0, i as u16));
                    self.emit(Instruction::new_rrr(
                        Opcode::SetElem,
                        result_reg.0,
                        idx_reg.0,
                        val_reg.0,
                    ));
                }
                ArrayElement::Spread(_) | ArrayElement::Hole => {
                    // TODO: Handle spread and hole/elision
                }
            }
        }

        Ok(result_reg)
    }

    fn compile_object(
        &mut self,
        obj: &crate::parser::ObjectExpression<'src, 'arena>,
    ) -> CompileResult<Register> {
        let result_reg = self.alloc_register();
        self.emit_at(
            Instruction::new_r(Opcode::NewObject, result_reg.0),
            obj.span,
        );

        for prop in obj.properties {
            match prop {
                ObjectProperty::Property(p) => {
                    let val_reg = self.compile_expression(p.value)?;

                    match &p.key {
                        PropertyKey::Identifier(id) => {
                            let str_id = self.intern_string(id.raw);
                            let name_idx = self.chunk.add_constant(Constant::String(str_id));
                            self.emit(Instruction::new_rrr(
                                Opcode::SetProp,
                                result_reg.0,
                                name_idx as u8,
                                val_reg.0,
                            ));
                        }
                        PropertyKey::Literal(lit) => {
                            if let Literal::String(s) = lit {
                                let str_id = self.intern_string(s.value);
                                let name_idx = self.chunk.add_constant(Constant::String(str_id));
                                self.emit(Instruction::new_rrr(
                                    Opcode::SetProp,
                                    result_reg.0,
                                    name_idx as u8,
                                    val_reg.0,
                                ));
                            }
                        }
                        PropertyKey::Computed(expr) => {
                            let key_reg = self.compile_expression(expr)?;
                            self.emit(Instruction::new_rrr(
                                Opcode::SetElem,
                                result_reg.0,
                                key_reg.0,
                                val_reg.0,
                            ));
                        }
                    }
                }
                ObjectProperty::SpreadProperty(_) => {
                    // TODO: Handle spread
                }
            }
        }

        Ok(result_reg)
    }
}

impl Default for Compiler<'_, '_> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Parser;
    use crate::parser::ast_arena::AstArena;

    fn compile(source: &str) -> CompileResult<Chunk> {
        let arena = AstArena::new();
        let parser = Parser::new(source, &arena);
        let (program, errors) = parser.parse();
        assert!(errors.is_empty(), "Parse errors: {errors:?}");
        Compiler::new().compile(&program)
    }

    #[test]
    fn test_compile_literal() {
        // UNWRAP-OK: hardcoded valid JS literal "42;" parses and compiles successfully
        let chunk = compile("42;").unwrap();
        assert!(!chunk.instructions.is_empty());
    }

    #[test]
    fn test_compile_binary() {
        // UNWRAP-OK: hardcoded valid JS expression "1 + 2;" parses and compiles successfully
        let chunk = compile("1 + 2;").unwrap();
        let disasm = chunk.disassemble();
        assert!(disasm.contains("ADD"));
    }

    #[test]
    fn test_compile_variable() {
        // UNWRAP-OK: hardcoded valid JS variable decl + use; parses and compiles successfully
        let chunk = compile("let x = 10; x;").unwrap();
        let disasm = chunk.disassemble();
        assert!(disasm.contains("SET_LOCAL") || disasm.contains("GET_LOCAL"));
    }

    #[test]
    fn test_compile_if() {
        // UNWRAP-OK: hardcoded valid JS if/else statement; parses and compiles successfully
        let chunk = compile("if (true) { 1; } else { 2; }").unwrap();
        let disasm = chunk.disassemble();
        assert!(disasm.contains("JMP"));
    }

    #[test]
    fn test_compile_while() {
        // UNWRAP-OK: hardcoded valid JS while-loop source; parses and compiles successfully
        let chunk = compile("let i = 0; while (i < 10) { i = i + 1; }").unwrap();
        let disasm = chunk.disassemble();
        assert!(disasm.contains("JMP"));
        assert!(disasm.contains("LT"));
    }

    #[test]
    fn test_compile_for() {
        // UNWRAP-OK: hardcoded valid JS for-loop source; parses and compiles successfully
        let chunk = compile("for (let i = 0; i < 10; i = i + 1) { i; }").unwrap();
        let disasm = chunk.disassemble();
        assert!(disasm.contains("JMP"));
    }
}
