//! NaN-boxed value representation for efficient 64-bit JavaScript values
//!
//! IEEE 754 double-precision floats have a large "NaN space" that can encode
//! non-number values. This technique is used by `SpiderMonkey`, `JavaScriptCore`,
//! and `LuaJIT` for efficient value representation.
//!
//! Uses bytemuck for safe bit-level transmutes between u64 and f64.
//!
//! Encoding scheme:
//! ```text
//! Doubles:     Normal IEEE 754 doubles (when not matching our tag patterns)
//! Tagged:      Uses quiet NaN space (exponent=0x7FF, bit 51=1)
//!
//! Bit layout for tagged values:
//! 63        51  48  47                                    0
//! +----------+----+----------------------------------------+
//! | 0x7FF8   |tag |              payload (48 bits)         |
//! +----------+----+----------------------------------------+
//!
//! Tags (bits 48-50):
//!   000 (0) = Object pointer
//!   001 (1) = String pointer
//!   010 (2) = Function pointer
//!   011 (3) = Symbol
//!   100 (4) = undefined
//!   101 (5) = null
//!   110 (6) = boolean (payload bit 0 = value)
//!   111 (7) = Small integer (SMI, signed 48-bit)
//! ```

use bytemuck::{Pod, Zeroable};
use static_assertions::{assert_eq_size, const_assert_eq};

/// Tagged value marker: we use negative quiet NaN space
/// Sign=1, Exponent=0x7FF (all 1s), Quiet bit=1
/// This ensures no collision with positive doubles or the canonical NaN
const TAG_BASE: u64 = 0xFFF8_0000_0000_0000;

/// Mask to check just the negative quiet NaN signature (bits 63-51)
/// This ignores the tag bits (50-48) and payload (47-0)
const TAG_BASE_MASK: u64 = 0xFFF8_0000_0000_0000;

/// Tag bits are in bits 50-48 (3 bits = 8 possible tags)
const TAG_MASK: u64 = 0x0007_0000_0000_0000;
const TAG_SHIFT: u32 = 48;

/// Payload mask (lower 48 bits)
const PAYLOAD_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

/// Tag values
const TAG_OBJECT: u64 = 0;
const TAG_STRING: u64 = 1;
const TAG_FUNCTION: u64 = 2;
const TAG_SYMBOL: u64 = 3;
const TAG_UNDEFINED: u64 = 4;
const TAG_NULL: u64 = 5;
const TAG_BOOLEAN: u64 = 6;
const TAG_SMI: u64 = 7;

/// Check if a raw u64 is a tagged value (not a regular double)
#[inline]
const fn is_tagged(bits: u64) -> bool {
    // It's tagged if it has the negative quiet NaN signature
    // The tag bits and payload don't affect this check
    (bits & TAG_BASE_MASK) == TAG_BASE
}

/// A NaN-boxed JavaScript value
///
/// Fits in 64 bits, allowing efficient register allocation and
/// avoiding heap allocation for primitives.
///
/// Derives `Pod` and `Zeroable` via bytemuck for safe transmutes
/// and zero-initialization (zeroed = 0.0 f64).
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(transparent)]
pub struct NanBoxedValue(u64);

// Compile-time size verification - value must be exactly 8 bytes
assert_eq_size!(NanBoxedValue, u64);
const_assert_eq!(std::mem::size_of::<NanBoxedValue>(), 8);

impl NanBoxedValue {
    // ========================================================================
    // Constructors
    // ========================================================================

    /// Create undefined value
    #[inline]
    #[must_use]
    pub const fn undefined() -> Self {
        Self(TAG_BASE | (TAG_UNDEFINED << TAG_SHIFT))
    }

    /// Create null value
    #[inline]
    #[must_use]
    pub const fn null() -> Self {
        Self(TAG_BASE | (TAG_NULL << TAG_SHIFT))
    }

    /// Create boolean value
    #[inline]
    #[must_use]
    pub const fn boolean(b: bool) -> Self {
        Self(TAG_BASE | (TAG_BOOLEAN << TAG_SHIFT) | (b as u64))
    }

    /// Create number value from f64
    ///
    /// All IEEE 754 doubles are stored directly, including NaN and Infinity.
    /// Our tagged values use a different bit pattern (negative quiet NaN space)
    /// that won't conflict with normal doubles.
    ///
    /// Uses bytemuck for safe bit-level transmute.
    #[inline]
    #[must_use]
    pub fn number(n: f64) -> Self {
        // Just store the bits directly - our tag prefix is in negative NaN space
        // which normal positive doubles (including canonical NaN) won't match
        Self(bytemuck::cast(n))
    }

    /// Create small integer (48-bit signed)
    #[inline]
    #[must_use]
    pub const fn smi(n: i64) -> Self {
        // Truncate to 48 bits (sign-extended on read)
        let payload = (n as u64) & PAYLOAD_MASK;
        Self(TAG_BASE | (TAG_SMI << TAG_SHIFT) | payload)
    }

    /// Create from object pointer
    ///
    /// # Safety
    /// Pointer must be valid and properly aligned. The caller is responsible
    /// for ensuring the pointed-to object outlives this value.
    #[inline]
    pub fn object(ptr: *mut ()) -> Self {
        let addr = ptr as u64;
        debug_assert!(addr <= PAYLOAD_MASK, "pointer too large for NaN-boxing");
        Self(TAG_BASE | (TAG_OBJECT << TAG_SHIFT) | addr)
    }

    /// Create from string pointer
    #[inline]
    pub fn string(ptr: *mut ()) -> Self {
        let addr = ptr as u64;
        debug_assert!(addr <= PAYLOAD_MASK, "pointer too large for NaN-boxing");
        Self(TAG_BASE | (TAG_STRING << TAG_SHIFT) | addr)
    }

    /// Create from function pointer
    #[inline]
    pub fn function(ptr: *mut ()) -> Self {
        let addr = ptr as u64;
        debug_assert!(addr <= PAYLOAD_MASK, "pointer too large for NaN-boxing");
        Self(TAG_BASE | (TAG_FUNCTION << TAG_SHIFT) | addr)
    }

    /// Create from symbol (interned string index)
    #[inline]
    #[must_use]
    pub const fn symbol(idx: u32) -> Self {
        Self(TAG_BASE | (TAG_SYMBOL << TAG_SHIFT) | (idx as u64))
    }

    // ========================================================================
    // Type checks
    // ========================================================================

    /// Check if value is a number (including NaN, Infinity)
    #[inline]
    #[must_use]
    pub const fn is_number(self) -> bool {
        !is_tagged(self.0)
    }

    /// Check if value is undefined
    #[inline]
    #[must_use]
    pub const fn is_undefined(self) -> bool {
        self.0 == Self::undefined().0
    }

    /// Check if value is null
    #[inline]
    #[must_use]
    pub const fn is_null(self) -> bool {
        self.0 == Self::null().0
    }

    /// Check if value is nullish (null or undefined)
    #[inline]
    #[must_use]
    pub const fn is_nullish(self) -> bool {
        self.is_null() || self.is_undefined()
    }

    /// Check if value is a boolean
    #[inline]
    #[must_use]
    pub const fn is_boolean(self) -> bool {
        is_tagged(self.0) && self.tag() == TAG_BOOLEAN
    }

    /// Check if value is a small integer
    #[inline]
    #[must_use]
    pub const fn is_smi(self) -> bool {
        is_tagged(self.0) && self.tag() == TAG_SMI
    }

    /// Check if value is an object pointer
    #[inline]
    #[must_use]
    pub const fn is_object(self) -> bool {
        is_tagged(self.0) && self.tag() == TAG_OBJECT
    }

    /// Check if value is a string pointer
    #[inline]
    #[must_use]
    pub const fn is_string(self) -> bool {
        is_tagged(self.0) && self.tag() == TAG_STRING
    }

    /// Check if value is a function pointer
    #[inline]
    #[must_use]
    pub const fn is_function(self) -> bool {
        is_tagged(self.0) && self.tag() == TAG_FUNCTION
    }

    /// Check if value is a symbol
    #[inline]
    #[must_use]
    pub const fn is_symbol(self) -> bool {
        is_tagged(self.0) && self.tag() == TAG_SYMBOL
    }

    /// Get the tag (0-7) for tagged values
    #[inline]
    const fn tag(self) -> u64 {
        (self.0 & TAG_MASK) >> TAG_SHIFT
    }

    /// Get the payload (lower 48 bits)
    #[inline]
    const fn payload(self) -> u64 {
        self.0 & PAYLOAD_MASK
    }

    // ========================================================================
    // Value extraction
    // ========================================================================

    /// Extract as f64 number
    ///
    /// Uses bytemuck for safe bit-level transmute.
    #[inline]
    #[must_use]
    pub fn as_number(self) -> Option<f64> {
        if self.is_number() {
            Some(bytemuck::cast(self.0))
        } else if self.is_smi() {
            // UNWRAP-OK: is_smi() just verified the SMI tag, so as_smi() returns Some.
            Some(self.as_smi().unwrap() as f64)
        } else {
            None
        }
    }

    /// Extract as boolean
    #[inline]
    #[must_use]
    pub const fn as_boolean(self) -> Option<bool> {
        if self.is_boolean() {
            Some((self.payload() & 1) != 0)
        } else {
            None
        }
    }

    /// Extract as small integer
    #[inline]
    #[must_use]
    pub const fn as_smi(self) -> Option<i64> {
        if self.is_smi() {
            // Sign-extend from 48 bits
            let payload = self.payload();
            let sign_bit = payload & 0x0000_8000_0000_0000;
            if sign_bit != 0 {
                // Negative: extend sign
                Some((payload | 0xFFFF_0000_0000_0000) as i64)
            } else {
                Some(payload as i64)
            }
        } else {
            None
        }
    }

    /// Extract as object pointer
    #[inline]
    #[must_use]
    pub fn as_object_ptr(self) -> Option<*mut ()> {
        if self.is_object() {
            Some(self.payload() as *mut ())
        } else {
            None
        }
    }

    /// Extract as string pointer
    #[inline]
    #[must_use]
    pub fn as_string_ptr(self) -> Option<*mut ()> {
        if self.is_string() {
            Some(self.payload() as *mut ())
        } else {
            None
        }
    }

    /// Extract as function pointer
    #[inline]
    #[must_use]
    pub fn as_function_ptr(self) -> Option<*mut ()> {
        if self.is_function() {
            Some(self.payload() as *mut ())
        } else {
            None
        }
    }

    /// Extract as symbol index
    #[inline]
    #[must_use]
    pub const fn as_symbol(self) -> Option<u32> {
        if self.is_symbol() {
            Some(self.payload() as u32)
        } else {
            None
        }
    }

    // ========================================================================
    // JavaScript semantics
    // ========================================================================

    /// `ToBoolean` - check if value is truthy
    #[inline]
    #[must_use]
    pub fn is_truthy(self) -> bool {
        if self.is_number() {
            let n: f64 = bytemuck::cast(self.0);
            n != 0.0 && !n.is_nan()
        } else if self.is_boolean() {
            // UNWRAP-OK: is_boolean() just verified the BOOLEAN tag, so as_boolean() returns Some.
            self.as_boolean().unwrap()
        } else if self.is_smi() {
            // UNWRAP-OK: is_smi() just verified the SMI tag, so as_smi() returns Some.
            self.as_smi().unwrap() != 0
        } else if self.is_nullish() {
            false
        } else {
            // Objects, strings (non-empty), functions, symbols are truthy
            true
        }
    }

    /// `ToNumber` conversion
    #[inline]
    #[must_use]
    pub fn to_number(self) -> f64 {
        if self.is_number() {
            bytemuck::cast(self.0)
        } else if self.is_smi() {
            // UNWRAP-OK: is_smi() just verified the SMI tag, so as_smi() returns Some.
            self.as_smi().unwrap() as f64
        } else if self.is_boolean() {
            // UNWRAP-OK: is_boolean() just verified the BOOLEAN tag, so as_boolean() returns Some.
            if self.as_boolean().unwrap() { 1.0 } else { 0.0 }
        } else if self.is_null() {
            0.0
        } else {
            f64::NAN
        }
    }

    /// `ToInt32` conversion
    #[inline]
    #[must_use]
    pub fn to_i32(self) -> i32 {
        if self.is_smi() {
            // UNWRAP-OK: is_smi() just verified the SMI tag, so as_smi() returns Some.
            self.as_smi().unwrap() as i32
        } else {
            let n = self.to_number();
            if n.is_nan() || n.is_infinite() || n == 0.0 {
                0
            } else {
                let int = n.trunc();
                let int32 = (int as i64) & 0xFFFF_FFFF;
                int32 as i32
            }
        }
    }

    /// `ToUint32` conversion
    #[inline]
    #[must_use]
    pub fn to_u32(self) -> u32 {
        self.to_i32() as u32
    }

    /// typeof operator result
    #[inline]
    #[must_use]
    pub fn type_of(self) -> &'static str {
        if self.is_undefined() {
            "undefined"
        } else if self.is_null() {
            "object" // Historical quirk
        } else if self.is_boolean() {
            "boolean"
        } else if self.is_number() || self.is_smi() {
            "number"
        } else if self.is_string() {
            "string"
        } else if self.is_symbol() {
            "symbol"
        } else if self.is_function() {
            "function"
        } else {
            "object"
        }
    }

    /// Get raw bits (for debugging/serialization)
    #[inline]
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }

    /// Create from raw bits
    #[inline]
    #[must_use]
    pub const fn from_raw(bits: u64) -> Self {
        Self(bits)
    }
}

impl Default for NanBoxedValue {
    fn default() -> Self {
        Self::undefined()
    }
}

impl PartialEq for NanBoxedValue {
    fn eq(&self, other: &Self) -> bool {
        // Handle NaN comparison specially - NaN !== NaN in JS
        if self.is_number() && other.is_number() {
            let a: f64 = bytemuck::cast(self.0);
            let b: f64 = bytemuck::cast(other.0);
            a == b // NaN != NaN is handled by f64's PartialEq
        } else {
            self.0 == other.0
        }
    }
}

impl std::fmt::Debug for NanBoxedValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_undefined() {
            write!(f, "undefined")
        } else if self.is_null() {
            write!(f, "null")
        } else if self.is_boolean() {
            // UNWRAP-OK: is_boolean() just verified the BOOLEAN tag, so as_boolean() returns Some.
            write!(f, "{}", self.as_boolean().unwrap())
        } else if self.is_smi() {
            // UNWRAP-OK: is_smi() just verified the SMI tag, so as_smi() returns Some.
            write!(f, "{}i", self.as_smi().unwrap())
        } else if self.is_number() {
            let n: f64 = bytemuck::cast(self.0);
            write!(f, "{n}")
        } else if self.is_object() {
            // UNWRAP-OK: is_object() just verified the OBJECT tag, so as_object_ptr() returns Some.
            write!(f, "Object({:p})", self.as_object_ptr().unwrap())
        } else if self.is_string() {
            // UNWRAP-OK: is_string() just verified the STRING tag, so as_string_ptr() returns Some.
            write!(f, "String({:p})", self.as_string_ptr().unwrap())
        } else if self.is_function() {
            // UNWRAP-OK: is_function() just verified the FUNCTION tag, so as_function_ptr() returns Some.
            write!(f, "Function({:p})", self.as_function_ptr().unwrap())
        } else if self.is_symbol() {
            // UNWRAP-OK: is_symbol() just verified the SYMBOL tag, so as_symbol() returns Some.
            write!(f, "Symbol({})", self.as_symbol().unwrap())
        } else {
            write!(f, "Unknown(0x{:016x})", self.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::mem;

    use super::*;

    #[test]
    fn test_size() {
        assert_eq!(mem::size_of::<NanBoxedValue>(), 8);
    }

    #[test]
    fn test_undefined_null() {
        let undef = NanBoxedValue::undefined();
        let null = NanBoxedValue::null();

        assert!(undef.is_undefined());
        assert!(!undef.is_null());
        assert!(undef.is_nullish());

        assert!(null.is_null());
        assert!(!null.is_undefined());
        assert!(null.is_nullish());

        assert!(!undef.is_truthy());
        assert!(!null.is_truthy());
    }

    #[test]
    fn test_boolean() {
        let t = NanBoxedValue::boolean(true);
        let f = NanBoxedValue::boolean(false);

        assert!(t.is_boolean());
        assert!(f.is_boolean());

        assert_eq!(t.as_boolean(), Some(true));
        assert_eq!(f.as_boolean(), Some(false));

        assert!(t.is_truthy());
        assert!(!f.is_truthy());
    }

    #[test]
    fn test_number() {
        let pi = NanBoxedValue::number(std::f64::consts::PI);
        let zero = NanBoxedValue::number(0.0);
        let neg = NanBoxedValue::number(-42.5);
        let inf = NanBoxedValue::number(f64::INFINITY);
        let nan = NanBoxedValue::number(f64::NAN);

        assert!(pi.is_number());
        assert!(zero.is_number());
        assert!(neg.is_number());
        assert!(inf.is_number());
        assert!(nan.is_number());

        assert_eq!(pi.as_number(), Some(std::f64::consts::PI));
        assert_eq!(zero.as_number(), Some(0.0));
        assert_eq!(neg.as_number(), Some(-42.5));
        assert_eq!(inf.as_number(), Some(f64::INFINITY));
        // UNWRAP-OK: nan was just created via NanBoxedValue::number, so as_number() returns Some.
        assert!(nan.as_number().unwrap().is_nan());

        assert!(pi.is_truthy());
        assert!(!zero.is_truthy());
        assert!(neg.is_truthy());
        assert!(!nan.is_truthy());
    }

    #[test]
    fn test_smi() {
        let pos = NanBoxedValue::smi(42);
        let neg = NanBoxedValue::smi(-100);
        let zero = NanBoxedValue::smi(0);
        let large = NanBoxedValue::smi(0x7FFF_FFFF_FFFF); // Max 47-bit positive

        assert!(pos.is_smi());
        assert!(neg.is_smi());
        assert!(zero.is_smi());
        assert!(large.is_smi());

        assert_eq!(pos.as_smi(), Some(42));
        assert_eq!(neg.as_smi(), Some(-100));
        assert_eq!(zero.as_smi(), Some(0));

        // SMI to number conversion
        assert!((pos.to_number() - 42.0).abs() < f64::EPSILON);
        assert!((neg.to_number() - (-100.0)).abs() < f64::EPSILON);

        assert!(pos.is_truthy());
        assert!(neg.is_truthy());
        assert!(!zero.is_truthy());
    }

    #[test]
    fn test_to_i32() {
        assert_eq!(NanBoxedValue::smi(42).to_i32(), 42);
        assert_eq!(NanBoxedValue::smi(-42).to_i32(), -42);
        assert_eq!(NanBoxedValue::number(42.9).to_i32(), 42);
        assert_eq!(NanBoxedValue::number(-42.9).to_i32(), -42);
        assert_eq!(NanBoxedValue::number(f64::NAN).to_i32(), 0);
        assert_eq!(NanBoxedValue::undefined().to_i32(), 0);
    }

    #[test]
    fn test_typeof() {
        assert_eq!(NanBoxedValue::undefined().type_of(), "undefined");
        assert_eq!(NanBoxedValue::null().type_of(), "object");
        assert_eq!(NanBoxedValue::boolean(true).type_of(), "boolean");
        assert_eq!(NanBoxedValue::number(42.0).type_of(), "number");
        assert_eq!(NanBoxedValue::smi(42).type_of(), "number");
        assert_eq!(NanBoxedValue::symbol(1).type_of(), "symbol");
    }

    #[test]
    fn test_equality() {
        assert_eq!(NanBoxedValue::smi(42), NanBoxedValue::smi(42));
        assert_ne!(NanBoxedValue::smi(42), NanBoxedValue::smi(43));
        assert_eq!(NanBoxedValue::undefined(), NanBoxedValue::undefined());
        assert_ne!(NanBoxedValue::null(), NanBoxedValue::undefined());

        // NaN !== NaN
        let nan1 = NanBoxedValue::number(f64::NAN);
        let nan2 = NanBoxedValue::number(f64::NAN);
        assert_ne!(nan1, nan2);
    }

    #[test]
    fn test_bytemuck_pod_zeroable() {
        // Test that Pod and Zeroable traits work correctly

        // Zeroable: zeroed value is 0.0 (valid f64)
        let zeroed: NanBoxedValue = bytemuck::Zeroable::zeroed();
        assert!(zeroed.is_number());
        assert_eq!(zeroed.as_number(), Some(0.0));

        // Pod: can cast slices
        let values = [
            NanBoxedValue::number(1.0),
            NanBoxedValue::number(2.0),
            NanBoxedValue::number(3.0),
        ];
        let bytes: &[u8] = bytemuck::cast_slice(&values);
        assert_eq!(bytes.len(), 24); // 3 * 8 bytes

        // Can cast back
        let restored: &[NanBoxedValue] = bytemuck::cast_slice(bytes);
        assert_eq!(restored.len(), 3);
        assert_eq!(restored[0].as_number(), Some(1.0));
        assert_eq!(restored[1].as_number(), Some(2.0));
        assert_eq!(restored[2].as_number(), Some(3.0));

        // Pod: can read/write as bytes for serialization
        let val = NanBoxedValue::smi(42);
        let bytes: [u8; 8] = bytemuck::cast(val);
        let restored: NanBoxedValue = bytemuck::cast(bytes);
        assert_eq!(restored.as_smi(), Some(42));
    }
}
