//! JavaScript value representation
//!
//! Simple tagged enum for Phase 4. Phase 5 will implement NaN-boxing
//! for 64-bit pointer-sized values with inline numbers.

use std::rc::Rc;
use std::cell::RefCell;
use std::collections::HashMap;

/// JavaScript value (simple representation for Phase 4)
///
/// Phase 5 will replace this with NaN-boxed representation for performance.
#[derive(Debug, Clone)]
pub enum Value {
    /// undefined
    Undefined,
    /// null
    Null,
    /// Boolean true/false
    Boolean(bool),
    /// IEEE 754 double-precision number
    Number(f64),
    /// String (interned index for now)
    String(u32),
    /// Object reference
    Object(Rc<RefCell<Object>>),
    /// Function reference
    Function(Rc<JsFunction>),
}

impl Default for Value {
    fn default() -> Self {
        Value::Undefined
    }
}

impl Value {
    /// Create undefined
    #[inline]
    pub const fn undefined() -> Self {
        Value::Undefined
    }

    /// Create null
    #[inline]
    pub const fn null() -> Self {
        Value::Null
    }

    /// Create boolean
    #[inline]
    pub const fn boolean(b: bool) -> Self {
        Value::Boolean(b)
    }

    /// Create number
    #[inline]
    pub const fn number(n: f64) -> Self {
        Value::Number(n)
    }

    /// Check if value is truthy (ToBoolean)
    #[inline]
    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Undefined | Value::Null => false,
            Value::Boolean(b) => *b,
            Value::Number(n) => *n != 0.0 && !n.is_nan(),
            Value::String(idx) => *idx != 0, // Assume 0 is empty string
            Value::Object(_) | Value::Function(_) => true,
        }
    }

    /// Check if value is nullish (null or undefined)
    #[inline]
    pub fn is_nullish(&self) -> bool {
        matches!(self, Value::Undefined | Value::Null)
    }

    /// ToNumber conversion
    pub fn to_number(&self) -> f64 {
        match self {
            Value::Undefined => f64::NAN,
            Value::Null => 0.0,
            Value::Boolean(b) => if *b { 1.0 } else { 0.0 },
            Value::Number(n) => *n,
            Value::String(_) => f64::NAN, // Simplified - real impl parses string
            Value::Object(_) | Value::Function(_) => f64::NAN,
        }
    }

    /// ToInt32 conversion
    pub fn to_i32(&self) -> i32 {
        let n = self.to_number();
        if n.is_nan() || n.is_infinite() || n == 0.0 {
            return 0;
        }
        // ES spec ToInt32 algorithm
        let int = n.trunc();
        let int32 = (int as i64) & 0xFFFF_FFFF;
        // Convert to i32, handling the sign bit properly
        int32 as i32
    }

    /// ToUint32 conversion
    pub fn to_u32(&self) -> u32 {
        self.to_i32() as u32
    }

    /// Type of value (for typeof operator)
    pub fn type_of(&self) -> &'static str {
        match self {
            Value::Undefined => "undefined",
            Value::Null => "object", // Historical quirk
            Value::Boolean(_) => "boolean",
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Object(_) => "object",
            Value::Function(_) => "function",
        }
    }
}

/// Simple object representation
#[derive(Debug)]
pub struct Object {
    /// Properties
    pub properties: HashMap<u32, Value>,
    /// Prototype (for prototype chain)
    pub prototype: Option<Rc<RefCell<Object>>>,
}

impl Object {
    /// Create empty object
    pub fn new() -> Self {
        Self {
            properties: HashMap::new(),
            prototype: None,
        }
    }

    /// Get property
    pub fn get(&self, key: u32) -> Value {
        if let Some(val) = self.properties.get(&key) {
            val.clone()
        } else if let Some(ref proto) = self.prototype {
            proto.borrow().get(key)
        } else {
            Value::Undefined
        }
    }

    /// Set property
    pub fn set(&mut self, key: u32, value: Value) {
        self.properties.insert(key, value);
    }
}

impl Default for Object {
    fn default() -> Self {
        Self::new()
    }
}

/// JavaScript function
#[derive(Debug)]
pub struct JsFunction {
    /// Bytecode chunk index
    pub chunk_idx: u32,
    /// Captured variables (closures)
    pub captures: Vec<Value>,
    /// Name (interned string index, None for anonymous)
    pub name: Option<u32>,
}

impl JsFunction {
    /// Create new function
    pub fn new(chunk_idx: u32) -> Self {
        Self {
            chunk_idx,
            captures: Vec::new(),
            name: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truthy() {
        assert!(!Value::Undefined.is_truthy());
        assert!(!Value::Null.is_truthy());
        assert!(!Value::Boolean(false).is_truthy());
        assert!(Value::Boolean(true).is_truthy());
        assert!(!Value::Number(0.0).is_truthy());
        assert!(Value::Number(1.0).is_truthy());
        assert!(Value::Number(-1.0).is_truthy());
        assert!(!Value::Number(f64::NAN).is_truthy());
    }

    #[test]
    fn test_to_number() {
        assert_eq!(Value::Null.to_number(), 0.0);
        assert!(Value::Undefined.to_number().is_nan());
        assert_eq!(Value::Boolean(true).to_number(), 1.0);
        assert_eq!(Value::Boolean(false).to_number(), 0.0);
        assert_eq!(Value::Number(42.0).to_number(), 42.0);
    }

    #[test]
    fn test_to_i32() {
        assert_eq!(Value::Number(42.5).to_i32(), 42);
        assert_eq!(Value::Number(-42.5).to_i32(), -42);
        assert_eq!(Value::Number(f64::NAN).to_i32(), 0);
        assert_eq!(Value::Number(f64::INFINITY).to_i32(), 0);
    }
}
