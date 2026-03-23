//! Optimized string type with Small String Optimization (SSO) and interning
//!
//! String representations:
//! - Inline: Short strings (<=22 bytes) stored directly in struct
//! - Heap: Longer strings on the heap
//! - Interned: Deduplicated strings referenced by index
//!
//! Design principles from V8's String, `SpiderMonkey`'s `JSLinearString`,
//! and Rust's `SmallVec`.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

/// Maximum bytes for inline storage
const SSO_CAPACITY: usize = 22;

/// Internal string representation
#[derive(Clone)]
enum StringRepr {
    /// Inline small string (up to 22 bytes)
    Inline { bytes: [u8; SSO_CAPACITY], len: u8 },
    /// Heap-allocated string
    Heap(Box<HeapString>),
    /// Interned string reference
    Interned(u32),
}

/// Heap-allocated string with cached hash
#[derive(Clone)]
struct HeapString {
    data: String,
    hash: u64,
}

impl HeapString {
    fn new(s: String) -> Self {
        let hash = hash_str(&s);
        Self { data: s, hash }
    }
}

/// JavaScript string with SSO and interning support
#[derive(Clone)]
pub struct JsString {
    repr: StringRepr,
}

impl JsString {
    /// Create from a string slice
    #[must_use]
    pub fn new(s: &str) -> Self {
        if s.len() <= SSO_CAPACITY {
            let mut bytes = [0u8; SSO_CAPACITY];
            bytes[..s.len()].copy_from_slice(s.as_bytes());
            Self {
                repr: StringRepr::Inline {
                    bytes,
                    len: s.len() as u8,
                },
            }
        } else {
            Self {
                repr: StringRepr::Heap(Box::new(HeapString::new(s.to_string()))),
            }
        }
    }

    /// Create from owned String
    #[must_use]
    pub fn from_string(s: String) -> Self {
        if s.len() <= SSO_CAPACITY {
            let mut bytes = [0u8; SSO_CAPACITY];
            bytes[..s.len()].copy_from_slice(s.as_bytes());
            Self {
                repr: StringRepr::Inline {
                    bytes,
                    len: s.len() as u8,
                },
            }
        } else {
            Self {
                repr: StringRepr::Heap(Box::new(HeapString::new(s))),
            }
        }
    }

    /// Create interned string reference
    #[must_use]
    pub fn interned(index: u32) -> Self {
        Self {
            repr: StringRepr::Interned(index),
        }
    }

    /// Check if inline
    #[must_use]
    pub fn is_inline(&self) -> bool {
        matches!(self.repr, StringRepr::Inline { .. })
    }

    /// Check if heap-allocated
    #[must_use]
    pub fn is_heap(&self) -> bool {
        matches!(self.repr, StringRepr::Heap(_))
    }

    /// Check if interned
    #[must_use]
    pub fn is_interned(&self) -> bool {
        matches!(self.repr, StringRepr::Interned(_))
    }

    /// Get interned index (only valid if `is_interned()`)
    #[must_use]
    pub fn interned_index(&self) -> Option<u32> {
        match &self.repr {
            StringRepr::Interned(idx) => Some(*idx),
            _ => None,
        }
    }

    /// Get string as &str (for non-interned strings)
    ///
    /// For interned strings, use the string table.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match &self.repr {
            StringRepr::Inline { bytes, len } => {
                // SAFETY: We only store valid UTF-8
                Some(unsafe { std::str::from_utf8_unchecked(&bytes[..*len as usize]) })
            }
            StringRepr::Heap(h) => Some(&h.data),
            StringRepr::Interned(_) => None, // Needs lookup
        }
    }

    /// Get string length
    #[must_use]
    pub fn len(&self) -> usize {
        match &self.repr {
            StringRepr::Inline { len, .. } => *len as usize,
            StringRepr::Heap(h) => h.data.len(),
            StringRepr::Interned(_) => 0, // Needs lookup
        }
    }

    /// Check if empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get hash (for non-interned strings)
    #[must_use]
    pub fn hash_value(&self) -> u64 {
        match &self.repr {
            StringRepr::Inline { bytes, len } => hash_bytes(&bytes[..*len as usize]),
            StringRepr::Heap(h) => h.hash,
            StringRepr::Interned(idx) => u64::from(*idx), // Use index as pseudo-hash
        }
    }
}

impl Default for JsString {
    fn default() -> Self {
        JsString::new("")
    }
}

impl PartialEq for JsString {
    fn eq(&self, other: &Self) -> bool {
        // Fast path: both interned with same index
        if let (StringRepr::Interned(a), StringRepr::Interned(b)) = (&self.repr, &other.repr) {
            return a == b;
        }

        // Compare actual strings
        match (self.as_str(), other.as_str()) {
            (Some(a), Some(b)) => a == b,
            _ => false, // Can't compare interned without table
        }
    }
}

impl Eq for JsString {}

impl Hash for JsString {
    fn hash<H: Hasher>(&self, state: &mut H) {
        if let Some(s) = self.as_str() {
            s.hash(state);
        } else if let Some(idx) = self.interned_index() {
            idx.hash(state);
        }
    }
}

impl std::fmt::Debug for JsString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.as_str() {
            Some(s) => write!(f, "JsString({s:?})"),
            None => write!(f, "JsString(interned:{})", self.interned_index().unwrap_or(0)),
        }
    }
}

/// Fast string hash function (FNV-1a)
fn hash_str(s: &str) -> u64 {
    hash_bytes(s.as_bytes())
}

fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325; // FNV-1a offset basis
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3); // FNV-1a prime
    }
    hash
}

/// String intern table for deduplication
#[derive(Debug)]
pub struct StringInternTable {
    /// Interned strings
    strings: Vec<String>,
    /// Hash -> index lookup
    lookup: HashMap<u64, Vec<u32>>,
}

impl StringInternTable {
    /// Create empty intern table
    #[must_use]
    pub fn new() -> Self {
        Self {
            strings: Vec::new(),
            lookup: HashMap::new(),
        }
    }

    /// Create with capacity
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            strings: Vec::with_capacity(capacity),
            lookup: HashMap::with_capacity(capacity),
        }
    }

    /// Intern a string, returning its index
    pub fn intern(&mut self, s: &str) -> u32 {
        let hash = hash_str(s);

        // Check for existing
        if let Some(indices) = self.lookup.get(&hash) {
            for &idx in indices {
                if self.strings[idx as usize] == s {
                    return idx;
                }
            }
        }

        // Add new entry
        let idx = self.strings.len() as u32;
        self.strings.push(s.to_string());
        self.lookup.entry(hash).or_default().push(idx);
        idx
    }

    /// Intern an owned string
    pub fn intern_owned(&mut self, s: String) -> u32 {
        let hash = hash_str(&s);

        // Check for existing
        if let Some(indices) = self.lookup.get(&hash) {
            for &idx in indices {
                if self.strings[idx as usize] == s {
                    return idx;
                }
            }
        }

        // Add new entry
        let idx = self.strings.len() as u32;
        self.lookup.entry(hash).or_default().push(idx);
        self.strings.push(s);
        idx
    }

    /// Get string by index
    pub fn get(&self, idx: u32) -> Option<&str> {
        self.strings.get(idx as usize).map(String::as_str)
    }

    /// Number of interned strings
    #[must_use]
    pub fn len(&self) -> usize {
        self.strings.len()
    }

    /// Check if empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.strings.is_empty()
    }

    /// Create a `JsString` reference to an interned string
    pub fn make_ref(&mut self, s: &str) -> JsString {
        let idx = self.intern(s);
        JsString::interned(idx)
    }
}

impl Default for StringInternTable {
    fn default() -> Self {
        Self::new()
    }
}

/// Concatenation helper - creates a new string or rope
#[must_use]
pub fn concat(a: &JsString, b: &JsString, table: &StringInternTable) -> JsString {
    // Get actual strings
    let s1 = match a.as_str() {
        Some(s) => s.to_string(),
        None => table
            .get(a.interned_index().unwrap_or(0))
            .unwrap_or("")
            .to_string(),
    };
    let s2 = match b.as_str() {
        Some(s) => s,
        None => table.get(b.interned_index().unwrap_or(0)).unwrap_or(""),
    };

    let result = s1 + s2;
    JsString::from_string(result)
}

/// Substring helper
#[must_use]
pub fn substring(s: &JsString, start: usize, end: usize, table: &StringInternTable) -> JsString {
    let str_content = match s.as_str() {
        Some(s) => s,
        None => table.get(s.interned_index().unwrap_or(0)).unwrap_or(""),
    };

    let start = start.min(str_content.len());
    let end = end.min(str_content.len()).max(start);

    JsString::new(&str_content[start..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sso_size() {
        // JsString should be reasonably small
        assert!(std::mem::size_of::<JsString>() <= 32);
    }

    #[test]
    fn test_inline_string() {
        let s = JsString::new("hello");
        assert!(s.is_inline());
        assert!(!s.is_heap());
        assert_eq!(s.as_str(), Some("hello"));
        assert_eq!(s.len(), 5);
    }

    #[test]
    fn test_max_inline() {
        // 22 bytes should still be inline
        let content = "1234567890123456789012"; // 22 chars
        assert_eq!(content.len(), 22);
        let s = JsString::new(content);
        assert!(s.is_inline());
        assert_eq!(s.as_str(), Some(content));
    }

    #[test]
    fn test_heap_string() {
        // 23+ bytes goes to heap
        let content = "12345678901234567890123"; // 23 chars
        assert_eq!(content.len(), 23);
        let s = JsString::new(content);
        assert!(s.is_heap());
        assert_eq!(s.as_str(), Some(content));
    }

    #[test]
    fn test_interned_string() {
        let mut table = StringInternTable::new();
        let idx = table.intern("hello");

        let s = JsString::interned(idx);
        assert!(s.is_interned());
        assert_eq!(s.interned_index(), Some(idx));
        assert_eq!(table.get(idx), Some("hello"));
    }

    #[test]
    fn test_intern_deduplication() {
        let mut table = StringInternTable::new();

        let idx1 = table.intern("hello");
        let idx2 = table.intern("world");
        let idx3 = table.intern("hello"); // Duplicate

        assert_eq!(idx1, idx3); // Same index
        assert_ne!(idx1, idx2);
        assert_eq!(table.len(), 2); // Only 2 unique strings
    }

    #[test]
    fn test_string_equality() {
        let s1 = JsString::new("hello");
        let s2 = JsString::new("hello");
        let s3 = JsString::new("world");

        assert_eq!(s1, s2);
        assert_ne!(s1, s3);
    }

    #[test]
    fn test_string_clone() {
        let s1 = JsString::new("hello");
        let s2 = s1.clone();
        assert_eq!(s1, s2);

        let long = JsString::new("this is a longer string that goes on heap");
        let long2 = long.clone();
        assert_eq!(long, long2);
    }

    #[test]
    fn test_concat() {
        let table = StringInternTable::new();
        let a = JsString::new("hello");
        let b = JsString::new(" world");
        let c = concat(&a, &b, &table);

        assert_eq!(c.as_str(), Some("hello world"));
    }

    #[test]
    fn test_substring() {
        let table = StringInternTable::new();
        let s = JsString::new("hello world");
        let sub = substring(&s, 0, 5, &table);

        assert_eq!(sub.as_str(), Some("hello"));
    }

    #[test]
    fn test_empty_string() {
        let s = JsString::new("");
        assert!(s.is_inline());
        assert!(s.is_empty());
        assert_eq!(s.len(), 0);
    }

    #[test]
    fn test_hash_consistency() {
        let h1 = hash_str("hello");
        let h2 = hash_str("hello");
        let h3 = hash_str("world");

        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
    }
}
