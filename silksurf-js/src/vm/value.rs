/*
 * value.rs -- JavaScript value representation (tagged enum).
 *
 * WHY: Every JS value (number, string, object, function, etc.) must be
 * representable as a single Rust type. Value is a tagged enum with
 * variants for each JS type. Strings use Rc<JsString> with SSO (Small
 * String Optimization -- inline up to 22 bytes, heap above).
 *
 * Type coercions per ES spec:
 *   to_number(): Undefined->NaN, Null->0, Bool->0/1, String->parse
 *   to_i32(): ES 7.1.6 ToInt32 with 2^32 modulo wrapping
 *   to_u32(): ES 7.1.7 ToUint32 (same modulo, unsigned)
 *   to_js_string(): ToString for each type (number formatting, etc.)
 *
 * PropertyKey: String(Rc<JsString>) | Index(u32) for object property access.
 * Objects use HashMap<PropertyKey, Value> with prototype chain lookup.
 *
 * NativeFunction: Rust closures callable from JS (Box<dyn Fn(&[Value])->Value>).
 * HostObject: native Rust objects exposed to JS via HostObject trait.
 *
 * Memory: Value is ~32-40 bytes (enum tag + largest variant).
 * Phase 5 target: NaN-boxing to compress to 8 bytes (see: nanbox.rs).
 *
 * See: string.rs JsString for SSO implementation
 * See: host.rs HostObject trait for native object dispatch
 * See: vm/mod.rs for register file (Vec<Value>)
 */

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use super::host::HostObjectRef;
use super::string::JsString;

/// Property key for object properties (string or symbol).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PropertyKey {
    /// String key (most common case)
    String(Rc<JsString>),
    /// Integer index (for array-like access)
    Index(u32),
}

impl PropertyKey {
    /// Create a string property key.
    pub fn from_str(s: &str) -> Self {
        PropertyKey::String(Rc::new(JsString::new(s)))
    }

    /// Create from an integer index.
    pub fn from_index(idx: u32) -> Self {
        PropertyKey::Index(idx)
    }

    /// Try to convert to a string representation.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            PropertyKey::String(s) => s.as_str(),
            PropertyKey::Index(_) => None,
        }
    }
}

/// JavaScript value (simple representation for Phase 4)
///
/// Phase 5 will replace this with NaN-boxed representation for performance.
#[derive(Debug, Clone, Default)]
pub enum Value {
    /// undefined
    #[default]
    Undefined,
    /// null
    Null,
    /// Boolean true/false
    Boolean(bool),
    /// IEEE 754 double-precision number
    Number(f64),
    /// String value with SSO
    String(Rc<JsString>),
    /// Object reference
    Object(Rc<RefCell<Object>>),
    /// Function reference
    Function(Rc<JsFunction>),
    /// Native function (built-in)
    NativeFunction(Rc<NativeFunction>),
    /// Host object (native Rust object exposed to JS)
    HostObject(HostObjectRef),
}

impl Value {
    /// Create undefined
    #[inline]
    #[must_use]
    pub const fn undefined() -> Self {
        Value::Undefined
    }

    /// Create null
    #[inline]
    #[must_use]
    pub const fn null() -> Self {
        Value::Null
    }

    /// Create boolean
    #[inline]
    #[must_use]
    pub const fn boolean(b: bool) -> Self {
        Value::Boolean(b)
    }

    /// Create number
    #[inline]
    #[must_use]
    pub const fn number(n: f64) -> Self {
        Value::Number(n)
    }

    /// Create string from &str
    #[inline]
    #[must_use]
    pub fn string(s: &str) -> Self {
        Value::String(Rc::new(JsString::new(s)))
    }

    /// Create string from owned String
    #[inline]
    #[must_use]
    pub fn string_owned(s: String) -> Self {
        Value::String(Rc::new(JsString::from_string(s)))
    }

    /// Check if value is truthy (`ToBoolean`)
    #[inline]
    #[must_use]
    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Undefined | Value::Null => false,
            Value::Boolean(b) => *b,
            Value::Number(n) => *n != 0.0 && !n.is_nan(),
            Value::String(s) => !s.is_empty(),
            Value::Object(_)
            | Value::Function(_)
            | Value::NativeFunction(_)
            | Value::HostObject(_) => true,
        }
    }

    /// Check if value is nullish (null or undefined)
    #[inline]
    #[must_use]
    pub fn is_nullish(&self) -> bool {
        matches!(self, Value::Undefined | Value::Null)
    }

    /// `ToNumber` conversion per ES spec
    #[must_use]
    pub fn to_number(&self) -> f64 {
        match self {
            Value::Null => 0.0,
            Value::Boolean(b) => {
                if *b {
                    1.0
                } else {
                    0.0
                }
            }
            Value::Number(n) => *n,
            Value::String(s) => {
                let text = s.as_str().unwrap_or("");
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    0.0
                } else {
                    trimmed.parse::<f64>().unwrap_or(f64::NAN)
                }
            }
            Value::Undefined
            | Value::Object(_)
            | Value::Function(_)
            | Value::NativeFunction(_)
            | Value::HostObject(_) => f64::NAN,
        }
    }

    /// `ToInt32` conversion per ECMA-262 7.1.6.
    ///
    /// Uses num-traits FloatCore for correct modulo arithmetic on f64,
    /// handling -0.0, large magnitudes, and sign preservation per spec.
    #[must_use]
    pub fn to_i32(&self) -> i32 {
        let n = self.to_number();
        if n.is_nan() || n.is_infinite() || n == 0.0 {
            return 0;
        }
        // Step 3: int = sign(n) * floor(abs(n))
        let int = n.signum() * n.abs().floor();
        // Step 4: int32bit = int modulo 2^32 (always positive remainder)
        const TWO_32: f64 = 4_294_967_296.0; // 2^32
        let int32bit = ((int % TWO_32) + TWO_32) % TWO_32;
        // Step 5: if int32bit >= 2^31, return int32bit - 2^32
        if int32bit >= 2_147_483_648.0 {
            (int32bit - TWO_32) as i32
        } else {
            int32bit as i32
        }
    }

    /// `ToUint32` conversion per ECMA-262 7.1.7.
    #[must_use]
    pub fn to_u32(&self) -> u32 {
        let n = self.to_number();
        if n.is_nan() || n.is_infinite() || n == 0.0 {
            return 0;
        }
        let int = n.signum() * n.abs().floor();
        const TWO_32: f64 = 4_294_967_296.0;
        (((int % TWO_32) + TWO_32) % TWO_32) as u32
    }

    /// `ToString` conversion per ES spec
    #[must_use]
    pub fn to_js_string(&self) -> Rc<JsString> {
        match self {
            Value::Undefined => Rc::new(JsString::new("undefined")),
            Value::Null => Rc::new(JsString::new("null")),
            Value::Boolean(b) => {
                if *b {
                    Rc::new(JsString::new("true"))
                } else {
                    Rc::new(JsString::new("false"))
                }
            }
            Value::Number(n) => {
                if n.is_nan() {
                    Rc::new(JsString::new("NaN"))
                } else if n.is_infinite() {
                    if *n > 0.0 {
                        Rc::new(JsString::new("Infinity"))
                    } else {
                        Rc::new(JsString::new("-Infinity"))
                    }
                } else if *n == 0.0 {
                    Rc::new(JsString::new("0"))
                } else {
                    // Format number without trailing zeros
                    let s = if n.fract() == 0.0 && n.abs() < 1e20 {
                        format!("{}", *n as i64)
                    } else {
                        format!("{n}")
                    };
                    Rc::new(JsString::from_string(s))
                }
            }
            Value::String(s) => Rc::clone(s),
            Value::Object(_) | Value::HostObject(_) => Rc::new(JsString::new("[object Object]")),
            Value::Function(_) | Value::NativeFunction(_) => {
                Rc::new(JsString::new("function () { [native code] }"))
            }
        }
    }

    /// Get string content if this is a string value.
    #[must_use]
    pub fn as_js_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => s.as_str(),
            _ => None,
        }
    }

    /// Type of value (for typeof operator)
    #[must_use]
    pub fn type_of(&self) -> &'static str {
        match self {
            Value::Undefined => "undefined",
            Value::Null | Value::Object(_) => "object",
            Value::Boolean(_) => "boolean",
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Function(_) | Value::NativeFunction(_) => "function",
            Value::HostObject(_) => "object",
        }
    }
}

/// Simple object representation with string-keyed properties.
#[derive(Debug)]
pub struct Object {
    /// Properties indexed by PropertyKey
    pub properties: HashMap<PropertyKey, Value>,
    /// Prototype (for prototype chain)
    pub prototype: Option<Rc<RefCell<Object>>>,
}

impl Object {
    /// Create empty object
    #[must_use]
    pub fn new() -> Self {
        Self {
            properties: HashMap::new(),
            prototype: None,
        }
    }

    /// Get property by PropertyKey
    #[must_use]
    pub fn get_by_key(&self, key: &PropertyKey) -> Value {
        if let Some(val) = self.properties.get(key) {
            val.clone()
        } else if let Some(ref proto) = self.prototype {
            proto.borrow().get_by_key(key)
        } else {
            Value::Undefined
        }
    }

    /// Get property by string name
    #[must_use]
    pub fn get_by_str(&self, name: &str) -> Value {
        // Check own properties by iterating (since we match on string content)
        for (key, val) in &self.properties {
            if let PropertyKey::String(s) = key {
                if s.as_str() == Some(name) {
                    return val.clone();
                }
            }
        }
        if let Some(ref proto) = self.prototype {
            proto.borrow().get_by_str(name)
        } else {
            Value::Undefined
        }
    }

    /// Get property by integer index (for array-like access and backward compat)
    #[must_use]
    pub fn get(&self, key: u32) -> Value {
        self.get_by_key(&PropertyKey::Index(key))
    }

    /// Set property by PropertyKey
    pub fn set_by_key(&mut self, key: PropertyKey, value: Value) {
        self.properties.insert(key, value);
    }

    /// Set property by string name
    pub fn set_by_str(&mut self, name: &str, value: Value) {
        self.set_by_key(PropertyKey::from_str(name), value);
    }

    /// Set property by integer index (backward compat)
    pub fn set(&mut self, key: u32, value: Value) {
        self.set_by_key(PropertyKey::Index(key), value);
    }
}

impl Default for Object {
    fn default() -> Self {
        Self::new()
    }
}

/// Native (built-in) function callable from JS.
pub struct NativeFunction {
    /// Function name
    pub name: String,
    /// Implementation
    pub func: Box<dyn Fn(&[Value]) -> Value>,
}

impl NativeFunction {
    /// Create a new native function.
    pub fn new(name: impl Into<String>, func: impl Fn(&[Value]) -> Value + 'static) -> Self {
        Self {
            name: name.into(),
            func: Box::new(func),
        }
    }

    /// Call the native function.
    pub fn call(&self, args: &[Value]) -> Value {
        (self.func)(args)
    }
}

impl std::fmt::Debug for NativeFunction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "NativeFunction({})", self.name)
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
    #[must_use]
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
        // String truthiness
        assert!(!Value::string("").is_truthy());
        assert!(Value::string("hello").is_truthy());
    }

    #[test]
    fn test_to_number() {
        assert_eq!(Value::Null.to_number(), 0.0);
        assert!(Value::Undefined.to_number().is_nan());
        assert_eq!(Value::Boolean(true).to_number(), 1.0);
        assert_eq!(Value::Boolean(false).to_number(), 0.0);
        assert_eq!(Value::Number(42.0).to_number(), 42.0);
        // String to number
        assert_eq!(Value::string("42").to_number(), 42.0);
        assert_eq!(Value::string("  3.14  ").to_number(), 3.14);
        assert_eq!(Value::string("").to_number(), 0.0);
        assert!(Value::string("abc").to_number().is_nan());
    }

    #[test]
    fn test_to_i32() {
        assert_eq!(Value::Number(42.5).to_i32(), 42);
        assert_eq!(Value::Number(-42.5).to_i32(), -42);
        assert_eq!(Value::Number(f64::NAN).to_i32(), 0);
        assert_eq!(Value::Number(f64::INFINITY).to_i32(), 0);
        assert_eq!(Value::Number(f64::NEG_INFINITY).to_i32(), 0);
        assert_eq!(Value::Number(-0.0).to_i32(), 0);
        // ES spec: 2^32 wraps to 0, 2^31 wraps to -2^31
        assert_eq!(Value::Number(4_294_967_296.0).to_i32(), 0); // 2^32
        assert_eq!(Value::Number(2_147_483_648.0).to_i32(), -2_147_483_648); // 2^31
        assert_eq!(Value::Number(4_294_967_295.0).to_i32(), -1); // 2^32 - 1
        // Negative modulo
        assert_eq!(Value::Number(-1.0).to_i32(), -1);
        assert_eq!(Value::Number(-2_147_483_649.0).to_i32(), 2_147_483_647); // -(2^31+1)
    }

    #[test]
    fn test_to_u32() {
        assert_eq!(Value::Number(42.5).to_u32(), 42);
        assert_eq!(Value::Number(-1.0).to_u32(), 4_294_967_295); // 2^32 - 1
        assert_eq!(Value::Number(4_294_967_296.0).to_u32(), 0); // 2^32
        assert_eq!(Value::Number(f64::NAN).to_u32(), 0);
        assert_eq!(Value::Number(-0.0).to_u32(), 0);
    }

    #[test]
    fn test_to_js_string() {
        assert_eq!(Value::Undefined.to_js_string().as_str(), Some("undefined"));
        assert_eq!(Value::Null.to_js_string().as_str(), Some("null"));
        assert_eq!(Value::Boolean(true).to_js_string().as_str(), Some("true"));
        assert_eq!(Value::Number(42.0).to_js_string().as_str(), Some("42"));
        assert_eq!(Value::Number(3.14).to_js_string().as_str(), Some("3.14"));
        assert_eq!(Value::string("hello").to_js_string().as_str(), Some("hello"));
    }

    #[test]
    fn test_property_key() {
        let key1 = PropertyKey::from_str("name");
        let key2 = PropertyKey::from_str("name");
        assert_eq!(key1, key2);

        let mut obj = Object::new();
        obj.set_by_str("name", Value::string("test"));
        assert_eq!(obj.get_by_str("name").as_js_str(), Some("test"));
    }
}
