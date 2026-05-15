//! Instruction encoding for `SilkSurfJS` bytecode
//!
//! Fixed-width 32-bit instructions for cache efficiency.
//! Supports both 3-operand (dst, src1, src2) and wide constant addressing.

use static_assertions::{assert_eq_size, const_assert_eq};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

use super::opcode::Opcode;

/// A single bytecode instruction (32 bits)
///
/// Encoding formats:
/// ```text
/// Standard (3-operand):
/// +--------+--------+--------+--------+
/// | opcode |  dst   |  src1  |  src2  |
/// | 8 bits | 8 bits | 8 bits | 8 bits |
/// +--------+--------+--------+--------+
///
/// Wide constant:
/// +--------+--------+------------------+
/// | opcode |  dst   |   constant_idx   |
/// | 8 bits | 8 bits |     16 bits      |
/// +--------+--------+------------------+
/// ```
///
/// # Zero-Copy Support
/// - `FromBytes` allows safe construction from raw byte slices
/// - `IntoBytes` allows safe conversion to byte slices
/// - Enables memory-mapped bytecode access without deserialization
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    FromBytes,
    IntoBytes,
    KnownLayout,
    Immutable,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
#[rkyv(compare(PartialEq))]
pub struct Instruction(u32);

// Compile-time size verification - instruction must be exactly 4 bytes
assert_eq_size!(Instruction, u32);
const_assert_eq!(std::mem::size_of::<Instruction>(), 4);

impl Instruction {
    /// Create instruction with 3 register operands
    #[inline]
    #[must_use]
    pub const fn new_rrr(opcode: Opcode, dst: u8, src1: u8, src2: u8) -> Self {
        Self((opcode as u32) | ((dst as u32) << 8) | ((src1 as u32) << 16) | ((src2 as u32) << 24))
    }

    /// Create instruction with 2 register operands
    #[inline]
    #[must_use]
    pub const fn new_rr(opcode: Opcode, dst: u8, src: u8) -> Self {
        Self((opcode as u32) | ((dst as u32) << 8) | ((src as u32) << 16))
    }

    /// Create instruction with 1 register operand
    #[inline]
    #[must_use]
    pub const fn new_r(opcode: Opcode, reg: u8) -> Self {
        Self((opcode as u32) | ((reg as u32) << 8))
    }

    /// Create instruction with register and 16-bit constant index
    #[inline]
    #[must_use]
    pub const fn new_ri(opcode: Opcode, dst: u8, idx: u16) -> Self {
        Self((opcode as u32) | ((dst as u32) << 8) | ((idx as u32) << 16))
    }

    /// Create instruction with 24-bit offset (for jumps)
    #[inline]
    #[must_use]
    pub const fn new_offset(opcode: Opcode, offset: i32) -> Self {
        // Store as signed 24-bit offset
        let offset_bits = (offset as u32) & 0x00FF_FFFF;
        Self((opcode as u32) | (offset_bits << 8))
    }

    /// Create instruction with register and 16-bit signed offset (for conditional jumps)
    #[inline]
    #[must_use]
    pub const fn new_r_offset(opcode: Opcode, reg: u8, offset: i16) -> Self {
        Self((opcode as u32) | ((reg as u32) << 8) | ((offset as u16 as u32) << 16))
    }

    /// Create no-operand instruction
    #[inline]
    #[must_use]
    pub const fn new(opcode: Opcode) -> Self {
        Self(opcode as u32)
    }

    /// Get the opcode
    #[inline]
    #[must_use]
    pub const fn opcode(self) -> u8 {
        (self.0 & 0xFF) as u8
    }

    /// Get decoded opcode enum
    #[inline]
    #[must_use]
    pub fn opcode_enum(self) -> Option<Opcode> {
        Opcode::from_byte(self.opcode())
    }

    /// Get first register (dst)
    #[inline]
    #[must_use]
    pub const fn dst(self) -> u8 {
        ((self.0 >> 8) & 0xFF) as u8
    }

    /// Get second register (src1)
    #[inline]
    #[must_use]
    pub const fn src1(self) -> u8 {
        ((self.0 >> 16) & 0xFF) as u8
    }

    /// Get third register (src2)
    #[inline]
    #[must_use]
    pub const fn src2(self) -> u8 {
        ((self.0 >> 24) & 0xFF) as u8
    }

    /// Get 16-bit constant index
    #[inline]
    #[must_use]
    pub const fn const_idx(self) -> u16 {
        ((self.0 >> 16) & 0xFFFF) as u16
    }

    /// Get 16-bit signed offset
    #[inline]
    #[must_use]
    pub const fn offset16(self) -> i16 {
        ((self.0 >> 16) & 0xFFFF) as i16
    }

    /// Get 24-bit signed offset
    #[inline]
    #[must_use]
    pub fn offset24(self) -> i32 {
        let raw = (self.0 >> 8) & 0x00FF_FFFF;
        // Sign extend from 24 bits
        if raw & 0x0080_0000 != 0 {
            (raw | 0xFF00_0000) as i32
        } else {
            raw as i32
        }
    }

    /// Get raw instruction bits
    #[inline]
    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }

    /// Create from raw bits
    #[inline]
    #[must_use]
    pub const fn from_raw(bits: u32) -> Self {
        Self(bits)
    }

    /// Read instruction from a byte slice (zero-copy)
    ///
    /// Uses zerocopy's `FromBytes` for safe, direct memory access.
    /// Ideal for memory-mapped bytecode.
    #[inline]
    #[must_use]
    pub fn from_bytes(bytes: &[u8; 4]) -> Self {
        // UNWRAP-OK: Instruction is repr(transparent) over u32 (4 bytes) and derives FromBytes;
        // ref_from_bytes on a &[u8; 4] always succeeds because size and alignment requirements
        // are satisfied by the array type. zerocopy proves this at the type level.
        *Self::ref_from_bytes(bytes).unwrap()
    }

    /// Read multiple instructions from a byte slice (zero-copy)
    ///
    /// Returns a slice of Instructions directly referencing the byte buffer.
    /// Requires proper alignment (4-byte) and length divisible by 4.
    #[inline]
    #[must_use]
    pub fn slice_from_bytes(bytes: &[u8]) -> Option<&[Self]> {
        if !bytes.len().is_multiple_of(4) {
            return None;
        }
        let count = bytes.len() / 4;
        <[Self]>::ref_from_bytes_with_elems(bytes, count).ok()
    }

    /// Convert instruction to bytes
    #[inline]
    #[must_use]
    pub fn to_bytes(&self) -> [u8; 4] {
        let mut bytes = [0u8; 4];
        bytes.copy_from_slice(self.as_bytes());
        bytes
    }
}

/// Register identifier (0-255)
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
)]
#[rkyv(compare(PartialEq))]
#[repr(transparent)]
pub struct Register(pub u8);

impl Register {
    /// The accumulator register (r0)
    pub const ACCUMULATOR: Register = Register(0);

    /// The this register (r1)
    pub const THIS: Register = Register(1);

    /// The first argument register (r2)
    pub const ARG0: Register = Register(2);

    /// Maximum register index
    pub const MAX: u8 = 255;

    #[inline]
    #[must_use]
    pub const fn new(idx: u8) -> Self {
        Self(idx)
    }

    #[inline]
    #[must_use]
    pub const fn index(self) -> u8 {
        self.0
    }
}

impl From<u8> for Register {
    fn from(idx: u8) -> Self {
        Self(idx)
    }
}

impl From<Register> for u8 {
    fn from(reg: Register) -> Self {
        reg.0
    }
}

/// Builder for constructing bytecode sequences
#[derive(Debug, Default)]
pub struct InstructionBuilder {
    instructions: Vec<Instruction>,
}

impl InstructionBuilder {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Emit instruction and return its offset
    pub fn emit(&mut self, instr: Instruction) -> usize {
        let offset = self.instructions.len();
        self.instructions.push(instr);
        offset
    }

    /// Emit load constant
    pub fn load_const(&mut self, dst: Register, idx: u16) -> usize {
        self.emit(Instruction::new_ri(Opcode::LoadConst, dst.0, idx))
    }

    /// Emit binary operation
    pub fn binary_op(&mut self, op: Opcode, dst: Register, lhs: Register, rhs: Register) -> usize {
        self.emit(Instruction::new_rrr(op, dst.0, lhs.0, rhs.0))
    }

    /// Emit unconditional jump (placeholder, to be patched)
    pub fn jmp_placeholder(&mut self) -> usize {
        self.emit(Instruction::new_offset(Opcode::Jmp, 0))
    }

    /// Emit conditional jump (placeholder)
    pub fn jmp_false_placeholder(&mut self, cond: Register) -> usize {
        self.emit(Instruction::new_r_offset(Opcode::JmpFalse, cond.0, 0))
    }

    /// Patch a jump instruction with actual offset
    pub fn patch_jump(&mut self, instr_offset: usize, target: usize) {
        let relative_offset = (target as i32) - (instr_offset as i32) - 1;
        let instr = self.instructions[instr_offset];
        let opcode = instr.opcode();

        // Determine if this is a conditional or unconditional jump
        if opcode == Opcode::Jmp as u8 {
            self.instructions[instr_offset] = Instruction::new_offset(Opcode::Jmp, relative_offset);
        } else {
            // Conditional jump with register operand
            let reg = instr.dst();
            // UNWRAP-OK: builder invariant: patch_jump targets an offset previously emitted
            // by jmp_*_placeholder (or another emit() with a typed Opcode). The byte we read
            // back here was written from a valid Opcode discriminant, so the round-trip
            // Opcode::from_byte cannot fail.
            self.instructions[instr_offset] = Instruction::new_r_offset(
                Opcode::from_byte(opcode).unwrap(),
                reg,
                relative_offset as i16,
            );
        }
    }

    /// Get current instruction count
    #[must_use]
    pub fn len(&self) -> usize {
        self.instructions.len()
    }

    /// Check if empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.instructions.is_empty()
    }

    /// Finish building and return instructions
    #[must_use]
    pub fn finish(self) -> Vec<Instruction> {
        self.instructions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_instruction_encoding_rrr() {
        let instr = Instruction::new_rrr(Opcode::Add, 0, 1, 2);
        assert_eq!(instr.opcode(), Opcode::Add as u8);
        assert_eq!(instr.dst(), 0);
        assert_eq!(instr.src1(), 1);
        assert_eq!(instr.src2(), 2);
    }

    #[test]
    fn test_instruction_encoding_ri() {
        let instr = Instruction::new_ri(Opcode::LoadConst, 5, 1000);
        assert_eq!(instr.opcode(), Opcode::LoadConst as u8);
        assert_eq!(instr.dst(), 5);
        assert_eq!(instr.const_idx(), 1000);
    }

    #[test]
    fn test_instruction_offset24() {
        // Positive offset
        let instr = Instruction::new_offset(Opcode::Jmp, 100);
        assert_eq!(instr.offset24(), 100);

        // Negative offset
        let instr = Instruction::new_offset(Opcode::Jmp, -50);
        assert_eq!(instr.offset24(), -50);
    }

    #[test]
    fn test_builder_patch_jump() {
        let mut builder = InstructionBuilder::new();

        // emit some instructions
        builder.emit(Instruction::new(Opcode::Nop)); // 0
        let jmp_offset = builder.jmp_placeholder(); // 1
        builder.emit(Instruction::new(Opcode::Nop)); // 2
        builder.emit(Instruction::new(Opcode::Nop)); // 3
        let target = builder.len(); // 4

        builder.patch_jump(jmp_offset, target);

        let instrs = builder.finish();
        let jmp = instrs[1];
        assert_eq!(jmp.opcode(), Opcode::Jmp as u8);
        assert_eq!(jmp.offset24(), 2); // target(4) - jmp_offset(1) - 1 = 2
    }

    #[test]
    fn test_zerocopy_from_bytes() {
        // Create instruction and convert to bytes
        let instr = Instruction::new_rrr(Opcode::Add, 0, 1, 2);
        let bytes = instr.to_bytes();

        // Read back using zerocopy
        let restored = Instruction::from_bytes(&bytes);
        assert_eq!(restored.opcode(), Opcode::Add as u8);
        assert_eq!(restored.dst(), 0);
        assert_eq!(restored.src1(), 1);
        assert_eq!(restored.src2(), 2);
    }

    #[test]
    fn test_zerocopy_slice() {
        // Create multiple instructions
        let instrs = [
            Instruction::new_ri(Opcode::LoadConst, 0, 100),
            Instruction::new_rrr(Opcode::Add, 0, 1, 2),
            Instruction::new_r(Opcode::Ret, 0),
        ];

        // Convert to aligned bytes (simulating memory-mapped file)
        let mut bytes = Vec::with_capacity(12);
        for instr in &instrs {
            bytes.extend_from_slice(instr.as_bytes());
        }

        // Zero-copy read
        if let Some(slice) = Instruction::slice_from_bytes(&bytes) {
            assert_eq!(slice.len(), 3);
            assert_eq!(slice[0].opcode(), Opcode::LoadConst as u8);
            assert_eq!(slice[1].opcode(), Opcode::Add as u8);
            assert_eq!(slice[2].opcode(), Opcode::Ret as u8);
        }
    }
}
