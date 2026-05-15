//! Bytecode chunk - a compiled unit of JavaScript code
//!
//! A chunk contains:
//! - Instruction sequence
//! - Constant pool (numbers, strings, functions)
//! - Debug information (optional)

use super::instruction::Instruction;
use super::opcode::Opcode;

/// A constant value in the constant pool
#[derive(Debug, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum Constant {
    /// IEEE 754 double-precision float
    Number(f64),
    /// Interned string (index into string table)
    String(u32),
    /// Function reference (index into function table)
    Function(u32),
    /// `BigInt` (stored as bytes for arbitrary precision)
    BigInt(Vec<u8>),
    /// Regular expression (pattern index, flags)
    RegExp { pattern: u32, flags: u16 },
}

/// Source location for debugging
#[derive(Debug, Clone, Copy, Default, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct SourceLocation {
    /// Byte offset in source
    pub offset: u32,
    /// Line number (1-indexed)
    pub line: u32,
    /// Column number (1-indexed)
    pub column: u32,
}

/// Debug information for a chunk
#[derive(Debug, Default, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct DebugInfo {
    /// Source file name (interned string index)
    pub source_name: Option<u32>,
    /// Mapping from instruction index to source location
    pub locations: Vec<(usize, SourceLocation)>,
    /// Local variable names for debugging
    pub local_names: Vec<(u8, u32)>, // (slot, name_idx)
}

/// Handler entry for exception handling
#[derive(Debug, Clone, Copy, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct ExceptionHandler {
    /// Start of try block (instruction index)
    pub try_start: u32,
    /// End of try block (instruction index)
    pub try_end: u32,
    /// Catch handler (instruction index), if present
    pub catch_target: Option<u32>,
    /// Finally handler (instruction index), if present
    pub finally_target: Option<u32>,
    /// Register to store caught exception
    pub exception_reg: u8,
}

/// A compiled bytecode chunk
#[derive(Debug, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct Chunk {
    /// The bytecode instructions
    pub instructions: Vec<Instruction>,
    /// Constant pool
    pub constants: Vec<Constant>,
    /// Number of registers needed
    pub register_count: u8,
    /// Number of parameters (excluding 'this')
    pub param_count: u8,
    /// Is this a strict mode function?
    pub strict: bool,
    /// Is this a generator function?
    pub is_generator: bool,
    /// Is this an async function?
    pub is_async: bool,
    /// Exception handlers
    pub handlers: Vec<ExceptionHandler>,
    /// Debug information (only in debug builds)
    pub debug_info: Option<DebugInfo>,
}

impl Chunk {
    /// Create a new empty chunk
    #[must_use]
    pub fn new() -> Self {
        Self {
            instructions: Vec::new(),
            constants: Vec::new(),
            register_count: 0,
            param_count: 0,
            strict: false,
            is_generator: false,
            is_async: false,
            handlers: Vec::new(),
            debug_info: None,
        }
    }

    /// Add a constant and return its index
    pub fn add_constant(&mut self, constant: Constant) -> u16 {
        let idx = self.constants.len();
        assert!(u16::try_from(idx).is_ok(), "constant pool overflow");
        self.constants.push(constant);
        idx as u16
    }

    /// Add a number constant
    pub fn add_number(&mut self, value: f64) -> u16 {
        // Check for existing identical constant to deduplicate
        for (i, c) in self.constants.iter().enumerate() {
            if let Constant::Number(n) = c
                && n.to_bits() == value.to_bits()
            {
                return i as u16;
            }
        }
        self.add_constant(Constant::Number(value))
    }

    /// Add a string constant (by interned index)
    pub fn add_string(&mut self, string_idx: u32) -> u16 {
        // Deduplicate
        for (i, c) in self.constants.iter().enumerate() {
            if let Constant::String(idx) = c
                && *idx == string_idx
            {
                return i as u16;
            }
        }
        self.add_constant(Constant::String(string_idx))
    }

    /// Emit an instruction and return its offset
    pub fn emit(&mut self, instr: Instruction) -> usize {
        let offset = self.instructions.len();
        self.instructions.push(instr);
        offset
    }

    /// Emit instruction with source location
    pub fn emit_with_loc(&mut self, instr: Instruction, loc: SourceLocation) -> usize {
        let offset = self.emit(instr);
        if let Some(ref mut debug) = self.debug_info {
            debug.locations.push((offset, loc));
        }
        offset
    }

    /// Get instruction at offset
    #[must_use]
    pub fn get(&self, offset: usize) -> Option<Instruction> {
        self.instructions.get(offset).copied()
    }

    /// Get constant at index
    #[must_use]
    pub fn get_constant(&self, idx: u16) -> Option<&Constant> {
        self.constants.get(idx as usize)
    }

    /// Mutable access to constants (for patching function chunk indices).
    pub fn constants_mut(&mut self) -> &mut [Constant] {
        &mut self.constants
    }

    /// Current instruction count
    #[must_use]
    pub fn len(&self) -> usize {
        self.instructions.len()
    }

    /// Check if empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.instructions.is_empty()
    }

    /// Enable debug info collection
    pub fn enable_debug(&mut self) {
        if self.debug_info.is_none() {
            self.debug_info = Some(DebugInfo::default());
        }
    }

    /// Add exception handler
    pub fn add_handler(&mut self, handler: ExceptionHandler) {
        self.handlers.push(handler);
    }

    /// Find handler for instruction at given offset
    #[must_use]
    pub fn find_handler(&self, offset: usize) -> Option<&ExceptionHandler> {
        let offset = offset as u32;
        self.handlers
            .iter()
            .find(|h| offset >= h.try_start && offset < h.try_end)
    }

    /// Disassemble the chunk into human-readable form
    #[must_use]
    pub fn disassemble(&self) -> String {
        use std::fmt::Write as _;
        let mut output = String::new();
        // UNWRAP-OK: writeln! into a String never fails (fmt::Write for String is infallible)
        writeln!(
            output,
            "; Chunk: {} instructions, {} constants, {} registers",
            self.instructions.len(),
            self.constants.len(),
            self.register_count
        )
        // UNWRAP-OK: writeln! into String is infallible (fmt::Write for String never errs).
        .unwrap();

        if self.strict {
            output.push_str("; strict mode\n");
        }
        if self.is_generator {
            output.push_str("; generator\n");
        }
        if self.is_async {
            output.push_str("; async\n");
        }

        output.push_str("\n; Constants:\n");
        for (i, c) in self.constants.iter().enumerate() {
            // UNWRAP-OK: writeln! into a String never fails (fmt::Write for String is infallible)
            writeln!(output, "  #{i}: {c:?}").unwrap();
        }

        output.push_str("\n; Instructions:\n");
        for (offset, instr) in self.instructions.iter().enumerate() {
            // UNWRAP-OK: writeln! into a String never fails (fmt::Write for String is infallible)
            writeln!(
                output,
                "  {:04x}: {}",
                offset,
                Self::disassemble_instruction(*instr)
            )
            .unwrap();
        }

        output
    }

    /// Disassemble a single instruction
    fn disassemble_instruction(instr: Instruction) -> String {
        let Some(op) = instr.opcode_enum() else {
            return format!("UNKNOWN(0x{:02x})", instr.opcode());
        };

        use Opcode::{
            Add, BitAnd, BitNot, BitOr, BitXor, Call, CallMethod, Debugger, Dec, DeleteElem,
            DeleteProp, Div, Eq, Ge, GetCapture, GetElem, GetGlobal, GetLocal, GetProp, Gt, Halt,
            In, Inc, Instanceof, Jmp, JmpFalse, JmpNotNullish, JmpNullish, JmpTrue, Le, LoadConst,
            LoadFalse, LoadMinusOne, LoadNull, LoadOne, LoadSmi, LoadTrue, LoadUndefined, LoadZero,
            Lt, Mod, Mov, Mul, Ne, Neg, NewArray, NewArrow, NewAsync, NewClass, NewFunction,
            NewGenerator, NewObject, NewRegExp, Nop, Not, Pow, Ret, RetUndefined, SetCapture,
            SetElem, SetGlobal, SetLocal, SetProp, Shl, Shr, StrictEq, StrictNe, Sub, TailCall,
            Throw, Typeof, Ushr,
        };
        match op {
            // No operands
            Nop => "NOP".to_string(),
            Halt => "HALT".to_string(),
            RetUndefined => "RET_UNDEFINED".to_string(),
            Debugger => "DEBUGGER".to_string(),

            // 1 register
            LoadTrue => format!("LOAD_TRUE r{}", instr.dst()),
            LoadFalse => format!("LOAD_FALSE r{}", instr.dst()),
            LoadNull => format!("LOAD_NULL r{}", instr.dst()),
            LoadUndefined => format!("LOAD_UNDEFINED r{}", instr.dst()),
            LoadZero => format!("LOAD_ZERO r{}", instr.dst()),
            LoadOne => format!("LOAD_ONE r{}", instr.dst()),
            LoadMinusOne => format!("LOAD_MINUS_ONE r{}", instr.dst()),
            Ret => format!("RET r{}", instr.dst()),
            Throw => format!("THROW r{}", instr.dst()),

            // Register + constant
            LoadConst => format!("LOAD_CONST r{}, #{}", instr.dst(), instr.const_idx()),
            LoadSmi => format!("LOAD_SMI r{}, {}", instr.dst(), instr.offset16()),

            // 2 registers
            Mov => format!("MOV r{}, r{}", instr.dst(), instr.src1()),
            Neg => format!("NEG r{}, r{}", instr.dst(), instr.src1()),
            Inc => format!("INC r{}, r{}", instr.dst(), instr.src1()),
            Dec => format!("DEC r{}, r{}", instr.dst(), instr.src1()),
            Not => format!("NOT r{}, r{}", instr.dst(), instr.src1()),
            BitNot => format!("BITNOT r{}, r{}", instr.dst(), instr.src1()),
            Typeof => format!("TYPEOF r{}, r{}", instr.dst(), instr.src1()),

            // 3 registers (arithmetic)
            Add => format!("ADD r{}, r{}, r{}", instr.dst(), instr.src1(), instr.src2()),
            Sub => format!("SUB r{}, r{}, r{}", instr.dst(), instr.src1(), instr.src2()),
            Mul => format!("MUL r{}, r{}, r{}", instr.dst(), instr.src1(), instr.src2()),
            Div => format!("DIV r{}, r{}, r{}", instr.dst(), instr.src1(), instr.src2()),
            Mod => format!("MOD r{}, r{}, r{}", instr.dst(), instr.src1(), instr.src2()),
            Pow => format!("POW r{}, r{}, r{}", instr.dst(), instr.src1(), instr.src2()),

            // Comparison
            Eq => format!("EQ r{}, r{}, r{}", instr.dst(), instr.src1(), instr.src2()),
            StrictEq => format!(
                "STRICT_EQ r{}, r{}, r{}",
                instr.dst(),
                instr.src1(),
                instr.src2()
            ),
            Ne => format!("NE r{}, r{}, r{}", instr.dst(), instr.src1(), instr.src2()),
            StrictNe => format!(
                "STRICT_NE r{}, r{}, r{}",
                instr.dst(),
                instr.src1(),
                instr.src2()
            ),
            Lt => format!("LT r{}, r{}, r{}", instr.dst(), instr.src1(), instr.src2()),
            Le => format!("LE r{}, r{}, r{}", instr.dst(), instr.src1(), instr.src2()),
            Gt => format!("GT r{}, r{}, r{}", instr.dst(), instr.src1(), instr.src2()),
            Ge => format!("GE r{}, r{}, r{}", instr.dst(), instr.src1(), instr.src2()),

            // Bitwise
            BitAnd => format!(
                "BITAND r{}, r{}, r{}",
                instr.dst(),
                instr.src1(),
                instr.src2()
            ),
            BitOr => format!(
                "BITOR r{}, r{}, r{}",
                instr.dst(),
                instr.src1(),
                instr.src2()
            ),
            BitXor => format!(
                "BITXOR r{}, r{}, r{}",
                instr.dst(),
                instr.src1(),
                instr.src2()
            ),
            Shl => format!("SHL r{}, r{}, r{}", instr.dst(), instr.src1(), instr.src2()),
            Shr => format!("SHR r{}, r{}, r{}", instr.dst(), instr.src1(), instr.src2()),
            Ushr => format!(
                "USHR r{}, r{}, r{}",
                instr.dst(),
                instr.src1(),
                instr.src2()
            ),

            // Jumps
            Jmp => format!("JMP {}", instr.offset24()),
            JmpTrue => format!("JMP_TRUE r{}, {}", instr.dst(), instr.offset16()),
            JmpFalse => format!("JMP_FALSE r{}, {}", instr.dst(), instr.offset16()),
            JmpNullish => format!("JMP_NULLISH r{}, {}", instr.dst(), instr.offset16()),
            JmpNotNullish => format!("JMP_NOT_NULLISH r{}, {}", instr.dst(), instr.offset16()),

            // Calls
            Call => format!(
                "CALL r{}, r{}, argc={}",
                instr.dst(),
                instr.src1(),
                instr.src2()
            ),
            CallMethod => {
                format!(
                    "CALL_METHOD r{}, r{}, #{}",
                    instr.dst(),
                    instr.src1(),
                    instr.src2()
                )
            }
            TailCall => format!("TAIL_CALL r{}, argc={}", instr.dst(), instr.src1()),

            // Properties
            GetProp => format!(
                "GET_PROP r{}, r{}, #{}",
                instr.dst(),
                instr.src1(),
                instr.src2()
            ),
            SetProp => format!(
                "SET_PROP r{}, #{}, r{}",
                instr.dst(),
                instr.src1(),
                instr.src2()
            ),
            GetElem => format!(
                "GET_ELEM r{}, r{}, r{}",
                instr.dst(),
                instr.src1(),
                instr.src2()
            ),
            SetElem => format!(
                "SET_ELEM r{}, r{}, r{}",
                instr.dst(),
                instr.src1(),
                instr.src2()
            ),
            DeleteProp => {
                format!(
                    "DELETE_PROP r{}, r{}, #{}",
                    instr.dst(),
                    instr.src1(),
                    instr.src2()
                )
            }
            DeleteElem => {
                format!(
                    "DELETE_ELEM r{}, r{}, r{}",
                    instr.dst(),
                    instr.src1(),
                    instr.src2()
                )
            }
            In => format!("IN r{}, r{}, r{}", instr.dst(), instr.src1(), instr.src2()),
            Instanceof => {
                format!(
                    "INSTANCEOF r{}, r{}, r{}",
                    instr.dst(),
                    instr.src1(),
                    instr.src2()
                )
            }

            // Object creation
            NewObject => format!("NEW_OBJECT r{}", instr.dst()),
            NewArray => format!("NEW_ARRAY r{}, len={}", instr.dst(), instr.const_idx()),
            NewFunction => format!("NEW_FUNCTION r{}, #{}", instr.dst(), instr.const_idx()),
            NewArrow => format!("NEW_ARROW r{}, #{}", instr.dst(), instr.const_idx()),
            NewGenerator => format!("NEW_GENERATOR r{}, #{}", instr.dst(), instr.const_idx()),
            NewAsync => format!("NEW_ASYNC r{}, #{}", instr.dst(), instr.const_idx()),
            NewClass => format!("NEW_CLASS r{}, #{}", instr.dst(), instr.const_idx()),
            NewRegExp => format!("NEW_REGEXP r{}, #{}", instr.dst(), instr.const_idx()),

            // Scope
            GetLocal => format!("GET_LOCAL r{}, slot={}", instr.dst(), instr.src1()),
            SetLocal => format!("SET_LOCAL slot={}, r{}", instr.dst(), instr.src1()),
            GetCapture => format!(
                "GET_CAPTURE r{}, depth={}, slot={}",
                instr.dst(),
                instr.src1(),
                instr.src2()
            ),
            SetCapture => format!(
                "SET_CAPTURE depth={}, slot={}, r{}",
                instr.dst(),
                instr.src1(),
                instr.src2()
            ),
            GetGlobal => format!("GET_GLOBAL r{}, #{}", instr.dst(), instr.const_idx()),
            SetGlobal => format!("SET_GLOBAL #{}, r{}", instr.const_idx(), instr.dst()),

            // Default for remaining opcodes
            _ => format!(
                "{:?} r{}, r{}, r{}",
                op,
                instr.dst(),
                instr.src1(),
                instr.src2()
            ),
        }
    }

    /// Serialize chunk to bytes for caching
    ///
    /// Uses rkyv for zero-copy deserialization. The returned bytes can be
    /// written to disk and memory-mapped for instant loading.
    // UNWRAP-OK: Chunk consists of POD/Vec/Option of rkyv-derived types only;
    // rkyv serialization for this shape can only fail under allocator OOM,
    // which would already abort. No I/O or fallible user types involved.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        rkyv::to_bytes::<rkyv::rancor::Error>(self)
            .expect("chunk serialization failed")
            .to_vec()
    }

    /// Deserialize chunk from bytes
    ///
    /// # Safety
    /// The bytes must have been produced by `to_bytes()` from a valid Chunk.
    /// This performs zero-copy deserialization for efficiency.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ChunkDeserializeError> {
        let archived = rkyv::access::<ArchivedChunk, rkyv::rancor::Error>(bytes)
            .map_err(|_| ChunkDeserializeError::InvalidArchive)?;
        rkyv::deserialize::<Chunk, rkyv::rancor::Error>(archived)
            .map_err(|_| ChunkDeserializeError::DeserializeFailed)
    }

    /// Access archived chunk directly without deserialization (zero-copy)
    ///
    /// This is the fastest way to read bytecode - no copying or allocation.
    /// The archived chunk can be used directly if the data is memory-mapped.
    pub fn access_archived(bytes: &[u8]) -> Result<&ArchivedChunk, ChunkDeserializeError> {
        rkyv::access::<ArchivedChunk, rkyv::rancor::Error>(bytes)
            .map_err(|_| ChunkDeserializeError::InvalidArchive)
    }
}

/// Error during chunk deserialization
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkDeserializeError {
    /// The byte buffer is not a valid rkyv archive
    InvalidArchive,
    /// Deserialization to owned Chunk failed
    DeserializeFailed,
}

impl std::fmt::Display for ChunkDeserializeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidArchive => write!(f, "invalid bytecode archive"),
            Self::DeserializeFailed => write!(f, "bytecode deserialization failed"),
        }
    }
}

impl std::error::Error for ChunkDeserializeError {}

impl Default for Chunk {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_constants() {
        let mut chunk = Chunk::new();

        let idx1 = chunk.add_number(std::f64::consts::PI);
        let idx2 = chunk.add_number(42.0);
        let idx3 = chunk.add_number(std::f64::consts::PI); // Deduplicated

        assert_eq!(idx1, 0);
        assert_eq!(idx2, 1);
        assert_eq!(idx3, 0); // Same as idx1
    }

    #[test]
    fn test_chunk_emit() {
        let mut chunk = Chunk::new();

        let const_idx = chunk.add_number(42.0);
        chunk.emit(Instruction::new_ri(Opcode::LoadConst, 0, const_idx));
        chunk.emit(Instruction::new_rrr(Opcode::Add, 0, 0, 1));
        chunk.emit(Instruction::new_r(Opcode::Ret, 0));

        assert_eq!(chunk.len(), 3);
    }

    #[test]
    fn test_disassemble() {
        let mut chunk = Chunk::new();
        chunk.register_count = 4;

        let const_idx = chunk.add_number(42.0);
        chunk.emit(Instruction::new_ri(Opcode::LoadConst, 0, const_idx));
        chunk.emit(Instruction::new_rrr(Opcode::Add, 2, 0, 1));
        chunk.emit(Instruction::new_r(Opcode::Ret, 2));

        let disasm = chunk.disassemble();
        assert!(disasm.contains("LOAD_CONST r0, #0"));
        assert!(disasm.contains("ADD r2, r0, r1"));
        assert!(disasm.contains("RET r2"));
    }

    #[test]
    fn test_chunk_serialize_roundtrip() {
        let mut chunk = Chunk::new();
        chunk.register_count = 4;
        chunk.param_count = 2;
        chunk.strict = true;

        // Add various constant types
        chunk.add_number(std::f64::consts::PI);
        chunk.add_number(42.0);
        chunk.add_string(123); // Interned string index
        chunk.add_constant(Constant::BigInt(vec![0x01, 0x02, 0x03]));
        chunk.add_constant(Constant::RegExp {
            pattern: 456,
            flags: 0x07,
        });

        // Add instructions
        chunk.emit(Instruction::new_ri(Opcode::LoadConst, 0, 0));
        chunk.emit(Instruction::new_rrr(Opcode::Add, 2, 0, 1));
        chunk.emit(Instruction::new_r_offset(Opcode::JmpFalse, 2, -5));
        chunk.emit(Instruction::new_r(Opcode::Ret, 2));

        // Add exception handler
        chunk.add_handler(ExceptionHandler {
            try_start: 0,
            try_end: 3,
            catch_target: Some(10),
            finally_target: None,
            exception_reg: 5,
        });

        // Serialize and deserialize
        let bytes = chunk.to_bytes();
        // UNWRAP-OK: bytes were just produced by to_bytes() above; round-trip is always valid
        let restored = Chunk::from_bytes(&bytes).expect("deserialization failed");

        // Verify all fields match
        assert_eq!(restored.instructions.len(), chunk.instructions.len());
        assert_eq!(restored.constants.len(), chunk.constants.len());
        assert_eq!(restored.register_count, chunk.register_count);
        assert_eq!(restored.param_count, chunk.param_count);
        assert_eq!(restored.strict, chunk.strict);
        assert_eq!(restored.handlers.len(), chunk.handlers.len());

        // Verify instruction encoding
        for (orig, rest) in chunk.instructions.iter().zip(restored.instructions.iter()) {
            assert_eq!(orig.raw(), rest.raw());
        }

        // Verify constant values
        for (orig, rest) in chunk.constants.iter().zip(restored.constants.iter()) {
            match (orig, rest) {
                (Constant::Number(a), Constant::Number(b)) => {
                    assert_eq!(a.to_bits(), b.to_bits());
                }
                (Constant::String(a), Constant::String(b))
                | (Constant::Function(a), Constant::Function(b)) => assert_eq!(a, b),
                (Constant::BigInt(a), Constant::BigInt(b)) => assert_eq!(a, b),
                (
                    Constant::RegExp {
                        pattern: p1,
                        flags: f1,
                    },
                    Constant::RegExp {
                        pattern: p2,
                        flags: f2,
                    },
                ) => {
                    assert_eq!(p1, p2);
                    assert_eq!(f1, f2);
                }
                _ => panic!("constant type mismatch"),
            }
        }
    }

    #[test]
    fn test_chunk_access_archived() {
        let mut chunk = Chunk::new();
        chunk.register_count = 8;
        chunk.emit(Instruction::new(Opcode::Nop));
        chunk.emit(Instruction::new_r(Opcode::LoadOne, 0));
        chunk.emit(Instruction::new_r(Opcode::Ret, 0));
        chunk.add_number(99.5);

        let bytes = chunk.to_bytes();

        // UNWRAP-OK: bytes were just produced by to_bytes() above; archive is well-formed
        let archived = Chunk::access_archived(&bytes).expect("access failed");

        // Verify we can read archived data without allocation
        assert_eq!(archived.instructions.len(), 3);
        assert_eq!(archived.register_count, 8);
        assert_eq!(archived.constants.len(), 1);
    }

    #[test]
    fn test_chunk_serialization_empty() {
        let chunk = Chunk::new();
        let bytes = chunk.to_bytes();
        // UNWRAP-OK: bytes were just produced by to_bytes() above; round-trip is always valid
        let restored = Chunk::from_bytes(&bytes).expect("deserialization failed");

        assert!(restored.instructions.is_empty());
        assert!(restored.constants.is_empty());
        assert_eq!(restored.register_count, 0);
    }

    #[test]
    fn test_chunk_invalid_bytes() {
        let garbage = vec![0x00, 0x01, 0x02, 0x03];
        let result = Chunk::from_bytes(&garbage);
        assert!(result.is_err());
        // UNWRAP-OK: assert!(result.is_err()) above guarantees Err variant
        assert_eq!(result.unwrap_err(), ChunkDeserializeError::InvalidArchive);
    }
}
