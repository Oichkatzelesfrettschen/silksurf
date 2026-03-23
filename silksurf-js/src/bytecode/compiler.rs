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
    ForInit, ForInLeft, Identifier, Literal, LogicalOperator, ObjectProperty, Program, PropertyKey,
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

/// Bytecode compiler
pub struct Compiler<'src, 'arena> {
    chunk: Chunk,
    /// Child chunks for nested function expressions / arrow functions
    child_chunks: Vec<Chunk>,
    /// String intern table for property names and identifiers.
    /// Maps string content -> u32 index in the VM's StringTable.
    string_pool: HashMap<String, u32>,
    next_string_id: u32,
    scopes: Vec<Scope>,
    current_scope: usize,
    next_register: u8,
    max_register: u8,
    loop_stack: Vec<LoopContext>,
    strict: bool,
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
            errors: Vec::new(),
            _phantom: std::marker::PhantomData,
        }
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
    /// Must be called after compile() on a second Compiler instance,
    /// or via compile_with_children().
    /// Compile and return (main_chunk, child_chunks, string_pool).
    pub fn compile_with_children(
        mut self,
        program: &Program<'src, 'arena>,
    ) -> CompileResult<(Chunk, Vec<Chunk>, Vec<(u32, String)>)> {
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
    fn into_parts(mut self) -> (Chunk, Vec<Chunk>, HashMap<String, u32>, u32) {
        self.chunk.register_count = self.max_register + 1;
        (self.chunk, self.child_chunks, self.string_pool, self.next_string_id)
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

    /// Get the string pool for loading into the VM's StringTable.
    pub fn get_string_pool(&self) -> Vec<(u32, String)> {
        self.string_pool.iter().map(|(s, &id)| (id, s.clone())).collect()
    }

    fn check_strict_directive(&mut self, program: &Program<'src, 'arena>) {
        if let Some(Statement::Expression(expr_stmt)) = program.body.first() {
            if let Expression::Literal(Literal::String(s)) = expr_stmt.expression {
                if s.value == "use strict" {
                    self.strict = true;
                }
            }
        }
    }

    fn collect_declarations(&mut self, stmts: &[Statement<'src, 'arena>]) {
        for stmt in stmts {
            match stmt {
                Statement::VariableDeclaration(decl) => {
                    if decl.kind == VariableKind::Var {
                        for declarator in decl.declarations {
                            if let crate::parser::Pattern::Identifier(id) = &declarator.id {
                                self.declare_var(id.name, VariableKind::Var, false);
                            }
                        }
                    }
                }
                // Hoist function declarations: pre-allocate their slots so the
                // function is available before its textual position.
                Statement::FunctionDeclaration(func) => {
                    if let Some(ref id) = func.id {
                        if self.lookup_var(id.name).is_none() {
                            self.declare_var(id.name, VariableKind::Var, false);
                        }
                    }
                }
                _ => {}
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
             * Uses EnterTry/LeaveTry/EnterCatch opcodes.
             * Exception value placed in register 0 for catch block.
             */
            Statement::Try(try_stmt) => {
                // Emit EnterTry with offset to catch block
                let enter_try = self.emit(Instruction::new_ri(
                    Opcode::EnterTry,
                    0,
                    0, // patch later
                ));

                // Compile try body
                for stmt in try_stmt.block.body {
                    self.compile_statement(stmt)?;
                }
                self.emit(Instruction::new(Opcode::LeaveTry));

                // Jump over catch block
                let skip_catch = self.emit(Instruction::new_offset(Opcode::Jmp, 0));

                // Patch EnterTry to point here (catch start)
                let catch_offset = self.current_offset() - enter_try - 1;
                self.chunk.instructions[enter_try] = Instruction::new_ri(
                    Opcode::EnterTry,
                    0,
                    catch_offset as u16,
                );
                self.emit(Instruction::new(Opcode::EnterCatch));

                // Compile catch body (if present)
                if let Some(ref handler) = try_stmt.handler {
                    // Exception is in r0; bind to catch variable
                    if let Some(crate::parser::Pattern::Identifier(ref id)) = handler.param {
                        self.declare_var(id.name, VariableKind::Let, true);
                    }
                    for stmt in handler.body.body {
                        self.compile_statement(stmt)?;
                    }
                }

                // Patch skip_catch jump
                self.patch_jump(skip_catch);

                // Compile finally block (if present)
                if let Some(ref finalizer) = try_stmt.finalizer {
                    for stmt in finalizer.body {
                        self.compile_statement(stmt)?;
                    }
                }
            }
            Statement::Return(ret) => {
                if let Some(arg) = ret.argument.as_ref() {
                    let reg = self.compile_expression(arg)?;
                    self.emit_at(Instruction::new_r(Opcode::Ret, reg.0), ret.span);
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
                    let mut child =
                        Compiler::new_with_pool(self.string_pool.clone(), self.next_string_id);
                    for param in func.params {
                        if let crate::parser::Pattern::Identifier(pid) = param {
                            child.declare_var(pid.name, VariableKind::Let, true);
                        }
                    }
                    child.collect_declarations(func.body.body);
                    for stmt in func.body.body {
                        child.compile_statement(stmt)?;
                    }
                    child.chunk.emit(Instruction::new(Opcode::RetUndefined));
                    let (mut child_chunk, nested, merged_pool, merged_next) = child.into_parts();
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
                    let tmp = self.alloc_register();
                    self.emit(Instruction::new_ri(Opcode::NewFunction, tmp.0, const_idx));
                    // Store into the pre-hoisted slot
                    if let Some((depth, slot)) = self.lookup_var(id.name) {
                        if depth == 0 {
                            self.emit(Instruction::new_rr(Opcode::SetLocal, slot, tmp.0));
                        } else {
                            self.emit(Instruction::new_rrr(
                                Opcode::SetCapture,
                                depth,
                                slot,
                                tmp.0,
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

                self.emit(Instruction::new_rr(Opcode::GetIterator, iter_reg.0, iter_src.0));

                let loop_start = self.current_offset();
                self.loop_stack.push(LoopContext {
                    break_targets: Vec::new(),
                    continue_targets: Vec::new(),
                });

                // result = iter.next()
                self.emit(Instruction::new_rr(Opcode::IterNext, result_reg.0, iter_reg.0));
                // done = result.done
                self.emit(Instruction::new_rr(Opcode::IterDone, done_reg.0, result_reg.0));
                // if done: exit
                let exit_jump = self.emit(Instruction::new_r_offset(Opcode::JmpTrue, done_reg.0, 0));
                // value = result.value
                self.emit(Instruction::new_rr(Opcode::IterValue, val_reg.0, result_reg.0));

                // Bind loop variable
                match &for_of.left {
                    ForInLeft::VariableDeclaration(decl) => {
                        for declarator in decl.declarations {
                            if let crate::parser::Pattern::Identifier(id) = &declarator.id {
                                self.declare_var(id.name, decl.kind, true);
                                if let Some((depth, slot)) = self.lookup_var(id.name) {
                                    if depth == 0 {
                                        self.emit(Instruction::new_rr(
                                            Opcode::SetLocal, slot, val_reg.0,
                                        ));
                                    } else {
                                        self.emit(Instruction::new_rrr(
                                            Opcode::SetCapture, depth, slot, val_reg.0,
                                        ));
                                    }
                                }
                            }
                        }
                    }
                    ForInLeft::Pattern(crate::parser::Pattern::Identifier(id)) => {
                        if let Some((depth, slot)) = self.lookup_var(id.name) {
                            if depth == 0 {
                                self.emit(Instruction::new_rr(
                                    Opcode::SetLocal, slot, val_reg.0,
                                ));
                            } else {
                                self.emit(Instruction::new_rrr(
                                    Opcode::SetCapture, depth, slot, val_reg.0,
                                ));
                            }
                        } else {
                            // Fall back to global assignment
                            let str_id = self.intern_string(id.raw);
                            let const_idx = self.chunk.add_constant(Constant::String(str_id));
                            self.emit(Instruction::new_ri(
                                Opcode::SetGlobal, val_reg.0, const_idx,
                            ));
                        }
                    }
                    _ => {}
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
                self.emit(Instruction::new_ri(Opcode::GetGlobal, obj_global_reg.0, obj_const_idx));

                let keys_str_id = self.intern_string("keys");
                let keys_const_idx = self.chunk.add_constant(Constant::String(keys_str_id));
                self.emit(Instruction::new_rrr(
                    Opcode::GetProp, keys_fn_reg.0, obj_global_reg.0, keys_const_idx as u8,
                ));
                // Call keys_fn(obj) -> emit Call: dst=keys_arr, fn=keys_fn, argc=1, argv=obj_reg
                // Call encoding: dst=r[keys_arr], src1=r[keys_fn], src2=argc(1)
                // Args are in consecutive registers starting at keys_fn+1, so mov obj_reg there
                let arg_reg = self.alloc_register();
                self.emit(Instruction::new_rr(Opcode::Mov, arg_reg.0, obj_reg.0));
                self.emit(Instruction::new_rrr(Opcode::Call, keys_arr_reg.0, keys_fn_reg.0, 1));

                // Now use GetIterator over keys_arr
                let iter_reg = self.alloc_register();
                let result_reg = self.alloc_register();
                let done_reg = self.alloc_register();
                let val_reg = self.alloc_register();

                self.emit(Instruction::new_rr(Opcode::GetIterator, iter_reg.0, keys_arr_reg.0));

                let loop_start = self.current_offset();
                self.loop_stack.push(LoopContext {
                    break_targets: Vec::new(),
                    continue_targets: Vec::new(),
                });

                self.emit(Instruction::new_rr(Opcode::IterNext, result_reg.0, iter_reg.0));
                self.emit(Instruction::new_rr(Opcode::IterDone, done_reg.0, result_reg.0));
                let exit_jump = self.emit(Instruction::new_r_offset(Opcode::JmpTrue, done_reg.0, 0));
                self.emit(Instruction::new_rr(Opcode::IterValue, val_reg.0, result_reg.0));

                match &for_in.left {
                    ForInLeft::VariableDeclaration(decl) => {
                        for declarator in decl.declarations {
                            if let crate::parser::Pattern::Identifier(id) = &declarator.id {
                                self.declare_var(id.name, decl.kind, true);
                                if let Some((depth, slot)) = self.lookup_var(id.name) {
                                    if depth == 0 {
                                        self.emit(Instruction::new_rr(Opcode::SetLocal, slot, val_reg.0));
                                    } else {
                                        self.emit(Instruction::new_rrr(Opcode::SetCapture, depth, slot, val_reg.0));
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
                                self.emit(Instruction::new_rrr(Opcode::SetCapture, depth, slot, val_reg.0));
                            }
                        } else {
                            let str_id = self.intern_string(id.raw);
                            let const_idx = self.chunk.add_constant(Constant::String(str_id));
                            self.emit(Instruction::new_ri(Opcode::SetGlobal, val_reg.0, const_idx));
                        }
                    }
                    _ => {}
                }

                let continue_target = self.current_offset();
                self.compile_statement(for_in.body)?;

                let back_offset = (loop_start as i32) - (self.current_offset() as i32) - 1;
                self.emit(Instruction::new_offset(Opcode::Jmp, back_offset));
                self.patch_jump(exit_jump);
                self.emit(Instruction::new_r(Opcode::IterClose, iter_reg.0));

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
                } else if decl.kind == VariableKind::Var {
                    if let Some((depth, slot)) = self.lookup_var(id.name) {
                        let reg = self.alloc_register();
                        self.emit(Instruction::new_r(Opcode::LoadUndefined, reg.0));
                        if depth == 0 {
                            self.emit(Instruction::new_rr(Opcode::SetLocal, slot, reg.0));
                        }
                    }
                }
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
                self.emit_at(Instruction::new_rr(Opcode::GetLocal, reg.0, Register::THIS.0), *span);
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
                let mut child =
                    Compiler::new_with_pool(self.string_pool.clone(), self.next_string_id);
                // Declare parameters in child scope
                for param in func.params {
                    if let crate::parser::Pattern::Identifier(id) = param {
                        child.declare_var(id.name, VariableKind::Let, true);
                    }
                }
                // Hoist var declarations and function declarations in the body
                child.collect_declarations(func.body.body);
                for stmt in func.body.body {
                    child.compile_statement(stmt)?;
                }
                child.chunk.emit(Instruction::new(Opcode::RetUndefined));
                let (mut child_chunk, nested, merged_pool, merged_next) = child.into_parts();
                // Merge child string pool additions back into parent
                self.string_pool = merged_pool;
                self.next_string_id = merged_next;
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
                self.emit(Instruction::new_ri(Opcode::NewFunction, result_reg.0, const_idx));
                Ok(result_reg)
            }
            /*
             * Expression::Arrow -- same shared-pool fix as Expression::Function.
             * Arrow functions capture lexical `this` but otherwise compile the
             * same way for our purposes (ChatGPT scripts don't use `this`).
             */
            Expression::Arrow(arrow) => {
                let result_reg = self.alloc_register();
                let mut child =
                    Compiler::new_with_pool(self.string_pool.clone(), self.next_string_id);
                for param in arrow.params {
                    if let crate::parser::Pattern::Identifier(id) = param {
                        child.declare_var(id.name, VariableKind::Let, true);
                    }
                }
                match &arrow.body {
                    crate::parser::ArrowBody::Expression(expr) => {
                        let val_reg = child.compile_expression(expr)?;
                        child.chunk.emit(Instruction::new_r(Opcode::Ret, val_reg.0));
                    }
                    crate::parser::ArrowBody::Block(block) => {
                        child.collect_declarations(block.body);
                        for stmt in block.body {
                            child.compile_statement(stmt)?;
                        }
                        child.chunk.emit(Instruction::new(Opcode::RetUndefined));
                    }
                }
                let (mut child_chunk, nested, merged_pool, merged_next) = child.into_parts();
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
                self.emit(Instruction::new_ri(Opcode::NewFunction, result_reg.0, const_idx));
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
                self.emit_at(Instruction::new_ri(Opcode::NewRegExp, reg.0, const_idx), r.span);
            }
            Literal::BigInt(b) => {
                let const_idx = self.chunk.add_constant(Constant::BigInt(Vec::new()));
                self.emit_at(Instruction::new_ri(Opcode::LoadConst, reg.0, const_idx), b.span);
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
                self.emit_at(Instruction::new_rrr(Opcode::GetCapture, reg.0, depth, slot), id.span);
            }
        } else {
            let str_id = self.intern_string(id.raw);
            let name_idx = self.chunk.add_constant(Constant::String(str_id));
            self.emit_at(Instruction::new_ri(Opcode::GetGlobal, reg.0, name_idx), id.span);
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

        self.emit_at(Instruction::new_rrr(opcode, result_reg.0, left_reg.0, right_reg.0), bin.span);
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

        self.emit_at(Instruction::new_rr(opcode, result_reg.0, arg_reg.0), unary.span);
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
            LogicalOperator::NullishCoalescing => {
                self.emit(Instruction::new_r_offset(Opcode::JmpNotNullish, result_reg.0, 0))
            }
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
                 */
                AssignmentOperator::NullishAssign
                | AssignmentOperator::LogicalAndAssign
                | AssignmentOperator::LogicalOrAssign => Opcode::Mov,
                _ => Opcode::Mov,
            };

            self.emit(Instruction::new_rrr(opcode, computed_reg.0, current_reg.0, value_reg.0));
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
                } else {
                    let str_id = self.intern_string(id.raw);
                    let name_idx = self.chunk.add_constant(Constant::String(str_id));
                    self.emit(Instruction::new_ri(Opcode::SetGlobal, final_value.0, name_idx));
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

        self.emit(Instruction::new_rr(Opcode::Mov, result_reg.0, final_value.0));
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
                self.emit(Instruction::new_rr(Opcode::Mov, result_reg.0, current_reg.0));
            }

            let updated_reg = self.alloc_register();
            let opcode = match update.operator {
                UpdateOperator::Increment => Opcode::Inc,
                UpdateOperator::Decrement => Opcode::Dec,
            };
            self.emit(Instruction::new_rr(opcode, updated_reg.0, current_reg.0));

            if let Some((depth, slot)) = self.lookup_var(id.name) {
                if depth == 0 {
                    self.emit(Instruction::new_rr(Opcode::SetLocal, slot, updated_reg.0));
                } else {
                    self.emit(Instruction::new_rrr(Opcode::SetCapture, depth, slot, updated_reg.0));
                }
            }

            if update.prefix {
                self.emit(Instruction::new_rr(Opcode::Mov, result_reg.0, updated_reg.0));
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

        let has_spread = call.arguments.iter().any(|a| matches!(a, Argument::Spread(_)));

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
                            Opcode::GetProp, push_fn_reg.0, args_reg.0, push_const as u8,
                        ));
                        // Call push: callee=push_fn, arg1=val (laid out after push_fn_reg)
                        let arg_slot = self.alloc_register();
                        self.emit(Instruction::new_rr(Opcode::Mov, arg_slot.0, val_reg.0));
                        self.emit(Instruction::new_rrr(Opcode::Call, zero_reg.0, push_fn_reg.0, 1));
                    }
                    Argument::Spread(spread_elem) => {
                        // args_arr = args_arr.concat(spread_val)
                        let spread_reg = self.compile_expression(spread_elem.argument)?;
                        let concat_fn_reg = self.alloc_register();
                        let concat_str_id = self.intern_string("concat");
                        let concat_const = self.chunk.add_constant(Constant::String(concat_str_id));
                        self.emit(Instruction::new_rrr(
                            Opcode::GetProp, concat_fn_reg.0, args_reg.0, concat_const as u8,
                        ));
                        let arg_slot = self.alloc_register();
                        self.emit(Instruction::new_rr(Opcode::Mov, arg_slot.0, spread_reg.0));
                        self.emit(Instruction::new_rrr(Opcode::Call, args_reg.0, concat_fn_reg.0, 1));
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

        self.emit_at(Instruction::new_ri(Opcode::NewArray, result_reg.0, len), arr.span);

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
        self.emit_at(Instruction::new_r(Opcode::NewObject, result_reg.0), obj.span);

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
        assert!(errors.is_empty(), "Parse errors: {:?}", errors);
        Compiler::new().compile(&program)
    }

    #[test]
    fn test_compile_literal() {
        let chunk = compile("42;").unwrap();
        assert!(!chunk.instructions.is_empty());
    }

    #[test]
    fn test_compile_binary() {
        let chunk = compile("1 + 2;").unwrap();
        let disasm = chunk.disassemble();
        assert!(disasm.contains("ADD"));
    }

    #[test]
    fn test_compile_variable() {
        let chunk = compile("let x = 10; x;").unwrap();
        let disasm = chunk.disassemble();
        assert!(disasm.contains("SET_LOCAL") || disasm.contains("GET_LOCAL"));
    }

    #[test]
    fn test_compile_if() {
        let chunk = compile("if (true) { 1; } else { 2; }").unwrap();
        let disasm = chunk.disassemble();
        assert!(disasm.contains("JMP"));
    }

    #[test]
    fn test_compile_while() {
        let chunk = compile("let i = 0; while (i < 10) { i = i + 1; }").unwrap();
        let disasm = chunk.disassemble();
        assert!(disasm.contains("JMP"));
        assert!(disasm.contains("LT"));
    }

    #[test]
    fn test_compile_for() {
        let chunk = compile("for (let i = 0; i < 10; i = i + 1) { i; }").unwrap();
        let disasm = chunk.disassemble();
        assert!(disasm.contains("JMP"));
    }
}
