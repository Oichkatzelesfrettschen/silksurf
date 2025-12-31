//! IR Builder for bytecode-to-Cranelift translation
//!
//! Translates SilkSurfJS bytecode instructions to Cranelift IR.

use cranelift_codegen::ir::{self, InstBuilder, Value};
use cranelift_codegen::ir::types::I64;
use cranelift_frontend::FunctionBuilder;
use cranelift_module::Module;

use crate::bytecode::{Chunk, Instruction, Opcode};
use super::compiler::JitError;

/// Builds Cranelift IR from bytecode
pub struct IrBuilder<'a, 'b, M: Module> {
    /// Function builder
    builder: &'b mut FunctionBuilder<'a>,
    /// Module reference (for function calls)
    #[allow(dead_code)]
    module: &'b M,
    /// Virtual registers (bytecode reg -> IR value)
    registers: Vec<Option<Value>>,
    /// Track if we've emitted a terminator (return, trap, etc.)
    has_terminator: bool,
}

impl<'a, 'b, M: Module> IrBuilder<'a, 'b, M> {
    /// Create a new IR builder
    pub fn new(builder: &'b mut FunctionBuilder<'a>, module: &'b M) -> Self {
        Self {
            builder,
            module,
            registers: vec![None; 256],
            has_terminator: false,
        }
    }

    /// Build IR for a complete function (caller must finalize the builder)
    pub fn build_function(mut self, chunk: &Chunk) -> Result<(), JitError> {
        // Create entry block
        let entry_block = self.builder.create_block();
        self.builder.switch_to_block(entry_block);
        self.builder.seal_block(entry_block);

        // Translate each instruction
        for instr in &chunk.instructions {
            self.translate_instruction(*instr)?;
        }

        // Ensure function returns (default return 0)
        if !self.has_terminator {
            let zero = self.builder.ins().iconst(I64, 0);
            self.builder.ins().return_(&[zero]);
        }

        // Note: caller must call builder.finalize() after this returns
        Ok(())
    }

    /// Translate a single bytecode instruction
    fn translate_instruction(&mut self, instr: Instruction) -> Result<(), JitError> {
        // Get opcode as enum, fall back for unknown
        let Some(opcode) = instr.opcode_enum() else {
            // Unknown opcode - trap
            self.builder.ins().trap(ir::TrapCode::user(0).unwrap());
            self.has_terminator = true;
            return Ok(());
        };

        match opcode {
            Opcode::Nop => {
                // No operation
            }

            Opcode::LoadConst => {
                // LoadConst dst, const_idx
                let dst = instr.dst();
                let const_idx = instr.const_idx();
                // For now, load as 64-bit integer (simplified)
                let val = self.builder.ins().iconst(I64, const_idx as i64);
                self.set_reg(dst, val);
            }

            Opcode::LoadUndefined => {
                let dst = instr.dst();
                // Undefined represented as special NaN-boxed value
                let val = self.builder.ins().iconst(I64, 0x7FF8_0000_0000_0000u64 as i64);
                self.set_reg(dst, val);
            }

            Opcode::LoadNull => {
                let dst = instr.dst();
                // Null represented as special NaN-boxed value
                let val = self.builder.ins().iconst(I64, 0x7FF9_0000_0000_0000u64 as i64);
                self.set_reg(dst, val);
            }

            Opcode::LoadTrue => {
                let dst = instr.dst();
                let val = self.builder.ins().iconst(I64, 1);
                self.set_reg(dst, val);
            }

            Opcode::LoadFalse => {
                let dst = instr.dst();
                let val = self.builder.ins().iconst(I64, 0);
                self.set_reg(dst, val);
            }

            Opcode::LoadSmi => {
                let dst = instr.dst();
                let imm = instr.offset16();
                let val = self.builder.ins().iconst(I64, imm as i64);
                self.set_reg(dst, val);
            }

            Opcode::Mov => {
                let dst = instr.dst();
                let src = instr.src1();
                if let Some(val) = self.get_reg(src) {
                    self.set_reg(dst, val);
                }
            }

            Opcode::Add => {
                let dst = instr.dst();
                let lhs = instr.src1();
                let rhs = instr.src2();
                if let (Some(l), Some(r)) = (self.get_reg(lhs), self.get_reg(rhs)) {
                    let result = self.builder.ins().iadd(l, r);
                    self.set_reg(dst, result);
                }
            }

            Opcode::Sub => {
                let dst = instr.dst();
                let lhs = instr.src1();
                let rhs = instr.src2();
                if let (Some(l), Some(r)) = (self.get_reg(lhs), self.get_reg(rhs)) {
                    let result = self.builder.ins().isub(l, r);
                    self.set_reg(dst, result);
                }
            }

            Opcode::Mul => {
                let dst = instr.dst();
                let lhs = instr.src1();
                let rhs = instr.src2();
                if let (Some(l), Some(r)) = (self.get_reg(lhs), self.get_reg(rhs)) {
                    let result = self.builder.ins().imul(l, r);
                    self.set_reg(dst, result);
                }
            }

            Opcode::Div => {
                let dst = instr.dst();
                let lhs = instr.src1();
                let rhs = instr.src2();
                if let (Some(l), Some(r)) = (self.get_reg(lhs), self.get_reg(rhs)) {
                    // Signed division
                    let result = self.builder.ins().sdiv(l, r);
                    self.set_reg(dst, result);
                }
            }

            Opcode::Mod => {
                let dst = instr.dst();
                let lhs = instr.src1();
                let rhs = instr.src2();
                if let (Some(l), Some(r)) = (self.get_reg(lhs), self.get_reg(rhs)) {
                    let result = self.builder.ins().srem(l, r);
                    self.set_reg(dst, result);
                }
            }

            Opcode::Neg => {
                let dst = instr.dst();
                let src = instr.src1();
                if let Some(val) = self.get_reg(src) {
                    let result = self.builder.ins().ineg(val);
                    self.set_reg(dst, result);
                }
            }

            Opcode::BitAnd => {
                let dst = instr.dst();
                let lhs = instr.src1();
                let rhs = instr.src2();
                if let (Some(l), Some(r)) = (self.get_reg(lhs), self.get_reg(rhs)) {
                    let result = self.builder.ins().band(l, r);
                    self.set_reg(dst, result);
                }
            }

            Opcode::BitOr => {
                let dst = instr.dst();
                let lhs = instr.src1();
                let rhs = instr.src2();
                if let (Some(l), Some(r)) = (self.get_reg(lhs), self.get_reg(rhs)) {
                    let result = self.builder.ins().bor(l, r);
                    self.set_reg(dst, result);
                }
            }

            Opcode::BitXor => {
                let dst = instr.dst();
                let lhs = instr.src1();
                let rhs = instr.src2();
                if let (Some(l), Some(r)) = (self.get_reg(lhs), self.get_reg(rhs)) {
                    let result = self.builder.ins().bxor(l, r);
                    self.set_reg(dst, result);
                }
            }

            Opcode::BitNot => {
                let dst = instr.dst();
                let src = instr.src1();
                if let Some(val) = self.get_reg(src) {
                    let result = self.builder.ins().bnot(val);
                    self.set_reg(dst, result);
                }
            }

            Opcode::Shl => {
                let dst = instr.dst();
                let lhs = instr.src1();
                let rhs = instr.src2();
                if let (Some(l), Some(r)) = (self.get_reg(lhs), self.get_reg(rhs)) {
                    let result = self.builder.ins().ishl(l, r);
                    self.set_reg(dst, result);
                }
            }

            Opcode::Shr => {
                let dst = instr.dst();
                let lhs = instr.src1();
                let rhs = instr.src2();
                if let (Some(l), Some(r)) = (self.get_reg(lhs), self.get_reg(rhs)) {
                    // Signed right shift
                    let result = self.builder.ins().sshr(l, r);
                    self.set_reg(dst, result);
                }
            }

            Opcode::Ushr => {
                let dst = instr.dst();
                let lhs = instr.src1();
                let rhs = instr.src2();
                if let (Some(l), Some(r)) = (self.get_reg(lhs), self.get_reg(rhs)) {
                    // Unsigned right shift
                    let result = self.builder.ins().ushr(l, r);
                    self.set_reg(dst, result);
                }
            }

            Opcode::Not => {
                let dst = instr.dst();
                let src = instr.src1();
                if let Some(val) = self.get_reg(src) {
                    // Logical not: 0 -> 1, non-zero -> 0
                    let zero = self.builder.ins().iconst(I64, 0);
                    let cmp = self.builder.ins().icmp(ir::condcodes::IntCC::Equal, val, zero);
                    let result = self.builder.ins().uextend(I64, cmp);
                    self.set_reg(dst, result);
                }
            }

            Opcode::Eq => {
                let dst = instr.dst();
                let lhs = instr.src1();
                let rhs = instr.src2();
                if let (Some(l), Some(r)) = (self.get_reg(lhs), self.get_reg(rhs)) {
                    let cmp = self.builder.ins().icmp(ir::condcodes::IntCC::Equal, l, r);
                    let result = self.builder.ins().uextend(I64, cmp);
                    self.set_reg(dst, result);
                }
            }

            Opcode::Ne => {
                let dst = instr.dst();
                let lhs = instr.src1();
                let rhs = instr.src2();
                if let (Some(l), Some(r)) = (self.get_reg(lhs), self.get_reg(rhs)) {
                    let cmp = self.builder.ins().icmp(ir::condcodes::IntCC::NotEqual, l, r);
                    let result = self.builder.ins().uextend(I64, cmp);
                    self.set_reg(dst, result);
                }
            }

            Opcode::Lt => {
                let dst = instr.dst();
                let lhs = instr.src1();
                let rhs = instr.src2();
                if let (Some(l), Some(r)) = (self.get_reg(lhs), self.get_reg(rhs)) {
                    let cmp = self.builder.ins().icmp(ir::condcodes::IntCC::SignedLessThan, l, r);
                    let result = self.builder.ins().uextend(I64, cmp);
                    self.set_reg(dst, result);
                }
            }

            Opcode::Le => {
                let dst = instr.dst();
                let lhs = instr.src1();
                let rhs = instr.src2();
                if let (Some(l), Some(r)) = (self.get_reg(lhs), self.get_reg(rhs)) {
                    let cmp = self.builder.ins().icmp(ir::condcodes::IntCC::SignedLessThanOrEqual, l, r);
                    let result = self.builder.ins().uextend(I64, cmp);
                    self.set_reg(dst, result);
                }
            }

            Opcode::Gt => {
                let dst = instr.dst();
                let lhs = instr.src1();
                let rhs = instr.src2();
                if let (Some(l), Some(r)) = (self.get_reg(lhs), self.get_reg(rhs)) {
                    let cmp = self.builder.ins().icmp(ir::condcodes::IntCC::SignedGreaterThan, l, r);
                    let result = self.builder.ins().uextend(I64, cmp);
                    self.set_reg(dst, result);
                }
            }

            Opcode::Ge => {
                let dst = instr.dst();
                let lhs = instr.src1();
                let rhs = instr.src2();
                if let (Some(l), Some(r)) = (self.get_reg(lhs), self.get_reg(rhs)) {
                    let cmp = self.builder.ins().icmp(ir::condcodes::IntCC::SignedGreaterThanOrEqual, l, r);
                    let result = self.builder.ins().uextend(I64, cmp);
                    self.set_reg(dst, result);
                }
            }

            Opcode::Ret => {
                let src = instr.dst();
                if let Some(val) = self.get_reg(src) {
                    self.builder.ins().return_(&[val]);
                } else {
                    let zero = self.builder.ins().iconst(I64, 0);
                    self.builder.ins().return_(&[zero]);
                }
                self.has_terminator = true;
            }

            Opcode::RetUndefined => {
                // Return undefined (special NaN-boxed value)
                let undefined = self.builder.ins().iconst(I64, 0x7FF8_0000_0000_0000u64 as i64);
                self.builder.ins().return_(&[undefined]);
                self.has_terminator = true;
            }

            // For unsupported opcodes, we skip them (interpreter will handle)
            _ => {
                // Most opcodes need interpreter support - just skip in JIT
            }
        }

        Ok(())
    }

    /// Get value for a virtual register
    fn get_reg(&self, reg: u8) -> Option<Value> {
        self.registers.get(reg as usize).copied().flatten()
    }

    /// Set value for a virtual register
    fn set_reg(&mut self, reg: u8, val: Value) {
        if (reg as usize) < self.registers.len() {
            self.registers[reg as usize] = Some(val);
        }
    }
}

#[cfg(test)]
mod tests {
    // IR builder tests would require full Cranelift setup
}
