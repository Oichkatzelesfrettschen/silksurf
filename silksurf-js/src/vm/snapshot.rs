//! VM snapshot/restore for bytecode caching and checkpointing
//!
//! Provides serializable snapshots of VM state for:
//! - Bytecode cache files
//! - REPL checkpointing
//! - Fast startup from saved state

use crate::bytecode::Chunk;

/// A serializable primitive value (no Rc/RefCell)
#[derive(Debug, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum PrimitiveValue {
    /// undefined
    Undefined,
    /// null
    Null,
    /// Boolean
    Boolean(bool),
    /// Number
    Number(f64),
    /// String (interned index)
    String(u32),
    /// Object reference placeholder (index into snapshot's object table)
    ObjectRef(u32),
    /// Function reference (chunk index)
    FunctionRef(u32),
}

impl PrimitiveValue {
    /// Convert from VM Value, replacing object/function refs with indices
    pub fn from_value(value: &super::Value, object_map: &mut ObjectRefMap) -> Self {
        match value {
            super::Value::Undefined => PrimitiveValue::Undefined,
            super::Value::Null => PrimitiveValue::Null,
            super::Value::Boolean(b) => PrimitiveValue::Boolean(*b),
            super::Value::Number(n) => PrimitiveValue::Number(*n),
            super::Value::String(s) => PrimitiveValue::String(*s),
            super::Value::Object(obj) => {
                let idx = object_map.get_or_insert_object(obj);
                PrimitiveValue::ObjectRef(idx)
            }
            super::Value::Function(func) => {
                PrimitiveValue::FunctionRef(func.chunk_idx)
            }
        }
    }
}

/// Tracks object identity for serialization
#[derive(Default)]
pub struct ObjectRefMap {
    /// Pointer -> index mapping (for deduplication)
    pointers: std::collections::HashMap<usize, u32>,
    /// Next available index
    next_idx: u32,
}

impl ObjectRefMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_or_insert_object(
        &mut self,
        obj: &std::rc::Rc<std::cell::RefCell<super::value::Object>>,
    ) -> u32 {
        let ptr = std::rc::Rc::as_ptr(obj) as usize;
        if let Some(&idx) = self.pointers.get(&ptr) {
            idx
        } else {
            let idx = self.next_idx;
            self.pointers.insert(ptr, idx);
            self.next_idx += 1;
            idx
        }
    }
}

/// Serialized call frame
#[derive(Debug, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct SnapshotCallFrame {
    /// Chunk index
    pub chunk_idx: u32,
    /// Program counter
    pub pc: u32,
    /// Base register offset
    pub base: u32,
    /// Return register
    pub return_reg: u8,
}

impl From<&super::CallFrame> for SnapshotCallFrame {
    fn from(frame: &super::CallFrame) -> Self {
        Self {
            chunk_idx: frame.chunk_idx as u32,
            pc: frame.pc as u32,
            base: frame.base as u32,
            return_reg: frame.return_reg,
        }
    }
}

/// Complete VM snapshot (serializable)
#[derive(Debug, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct VmSnapshot {
    /// Primitive register values (first N registers)
    pub registers: Vec<PrimitiveValue>,
    /// Call stack
    pub call_stack: Vec<SnapshotCallFrame>,
    /// Compiled chunks (bytecode)
    pub chunks: Vec<Chunk>,
    /// Interned strings
    pub strings: Vec<String>,
    /// Maximum call stack depth setting
    pub max_stack_depth: u32,
}

/// Error during snapshot operations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotError {
    /// Serialization failed
    SerializeFailed,
    /// Invalid snapshot data
    InvalidSnapshot,
    /// Deserialization failed
    DeserializeFailed,
    /// Snapshot contains unsupported features (e.g., closures)
    UnsupportedFeature,
}

impl std::fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SerializeFailed => write!(f, "snapshot serialization failed"),
            Self::InvalidSnapshot => write!(f, "invalid snapshot data"),
            Self::DeserializeFailed => write!(f, "snapshot deserialization failed"),
            Self::UnsupportedFeature => write!(f, "snapshot contains unsupported features"),
        }
    }
}

impl std::error::Error for SnapshotError {}

impl VmSnapshot {
    /// Create snapshot from VM state
    ///
    /// Note: Object and function references are captured as indices.
    /// Full object graph serialization requires additional work.
    pub fn from_vm(vm: &super::Vm, register_count: usize) -> Self {
        let mut object_map = ObjectRefMap::new();

        let registers: Vec<_> = vm.registers[..register_count]
            .iter()
            .map(|v| PrimitiveValue::from_value(v, &mut object_map))
            .collect();

        let call_stack: Vec<_> = vm.call_stack.iter().map(SnapshotCallFrame::from).collect();

        let strings: Vec<_> = vm.strings.strings.iter().cloned().collect();

        Self {
            registers,
            call_stack,
            chunks: vm.chunks.clone(),
            strings,
            max_stack_depth: vm.max_stack_depth as u32,
        }
    }

    /// Serialize snapshot to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        rkyv::to_bytes::<rkyv::rancor::Error>(self)
            .expect("snapshot serialization failed")
            .to_vec()
    }

    /// Deserialize snapshot from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, SnapshotError> {
        let archived = rkyv::access::<ArchivedVmSnapshot, rkyv::rancor::Error>(bytes)
            .map_err(|_| SnapshotError::InvalidSnapshot)?;
        rkyv::deserialize::<VmSnapshot, rkyv::rancor::Error>(archived)
            .map_err(|_| SnapshotError::DeserializeFailed)
    }

    /// Access archived snapshot without deserialization (zero-copy)
    pub fn access_archived(bytes: &[u8]) -> Result<&ArchivedVmSnapshot, SnapshotError> {
        rkyv::access::<ArchivedVmSnapshot, rkyv::rancor::Error>(bytes)
            .map_err(|_| SnapshotError::InvalidSnapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::{Instruction, Opcode};

    #[test]
    fn test_snapshot_primitive_values() {
        let mut object_map = ObjectRefMap::new();

        // Test primitive conversions
        let undef = PrimitiveValue::from_value(&super::super::Value::Undefined, &mut object_map);
        assert!(matches!(undef, PrimitiveValue::Undefined));

        let null = PrimitiveValue::from_value(&super::super::Value::Null, &mut object_map);
        assert!(matches!(null, PrimitiveValue::Null));

        let bool_val = PrimitiveValue::from_value(&super::super::Value::Boolean(true), &mut object_map);
        assert!(matches!(bool_val, PrimitiveValue::Boolean(true)));

        let num = PrimitiveValue::from_value(&super::super::Value::Number(3.14), &mut object_map);
        if let PrimitiveValue::Number(n) = num {
            assert!((n - 3.14).abs() < f64::EPSILON);
        } else {
            panic!("Expected number");
        }
    }

    #[test]
    fn test_snapshot_roundtrip() {
        let mut vm = super::super::Vm::new();

        // Add a chunk
        let mut chunk = Chunk::new();
        chunk.emit(Instruction::new_r_offset(Opcode::LoadSmi, 0, 42));
        chunk.emit(Instruction::new_r(Opcode::Ret, 0));
        vm.add_chunk(chunk);

        // Set some register values
        vm.registers[0] = super::super::Value::Number(100.0);
        vm.registers[1] = super::super::Value::Boolean(true);
        vm.registers[2] = super::super::Value::String(5);

        // Intern a string
        vm.strings.intern("hello".to_string());

        // Create snapshot
        let snapshot = VmSnapshot::from_vm(&vm, 10);

        // Serialize and deserialize
        let bytes = snapshot.to_bytes();
        let restored = VmSnapshot::from_bytes(&bytes).expect("deserialize failed");

        // Verify
        assert_eq!(restored.registers.len(), 10);
        assert_eq!(restored.chunks.len(), 1);
        assert_eq!(restored.strings.len(), 1);
        assert_eq!(restored.strings[0], "hello");

        // Check register values
        match &restored.registers[0] {
            PrimitiveValue::Number(n) => assert_eq!(*n, 100.0),
            _ => panic!("Expected number"),
        }
        match &restored.registers[1] {
            PrimitiveValue::Boolean(b) => assert!(*b),
            _ => panic!("Expected boolean"),
        }
    }

    #[test]
    fn test_snapshot_empty_vm() {
        let vm = super::super::Vm::new();
        let snapshot = VmSnapshot::from_vm(&vm, 0);

        let bytes = snapshot.to_bytes();
        let restored = VmSnapshot::from_bytes(&bytes).expect("deserialize failed");

        assert!(restored.registers.is_empty());
        assert!(restored.call_stack.is_empty());
        assert!(restored.chunks.is_empty());
    }

    #[test]
    fn test_snapshot_call_frame() {
        let frame = super::super::CallFrame {
            chunk_idx: 5,
            pc: 100,
            base: 256,
            return_reg: 7,
        };

        let snapshot_frame = SnapshotCallFrame::from(&frame);
        assert_eq!(snapshot_frame.chunk_idx, 5);
        assert_eq!(snapshot_frame.pc, 100);
        assert_eq!(snapshot_frame.base, 256);
        assert_eq!(snapshot_frame.return_reg, 7);
    }

    #[test]
    fn test_invalid_snapshot_bytes() {
        let garbage = vec![0xDE, 0xAD, 0xBE, 0xEF];
        let result = VmSnapshot::from_bytes(&garbage);
        assert!(result.is_err());
    }
}
