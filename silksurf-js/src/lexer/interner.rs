//! String interning for O(1) identifier comparison
//!
//! Uses lasso crate for efficient, thread-safe interning.
//! Interned strings become Symbols that compare in O(1).

use lasso::{Rodeo, Spur};

/// An interned string symbol
///
/// Comparing two Symbols is O(1) (integer comparison).
/// Getting the string back is O(1) (lookup in interner).
pub type Symbol = Spur;

/// String interner using lasso
///
/// Thread-safe, arena-allocated string storage.
/// Once interned, strings are never deallocated.
pub struct Interner {
    rodeo: Rodeo,
}

impl Interner {
    /// Create a new interner
    #[must_use]
    pub fn new() -> Self {
        Self {
            rodeo: Rodeo::default(),
        }
    }

    /// Create interner with pre-interned common strings
    #[must_use]
    pub fn with_common_identifiers() -> Self {
        let mut interner = Self::new();

        // Pre-intern common JavaScript identifiers
        // This reduces hash collisions and speeds up lookups
        for s in COMMON_IDENTIFIERS {
            interner.intern(s);
        }

        interner
    }

    /// Intern a string, returning its symbol
    #[inline]
    pub fn intern(&mut self, s: &str) -> Symbol {
        self.rodeo.get_or_intern(s)
    }

    /// Get the interned string if it exists
    #[inline]
    #[must_use]
    pub fn get(&self, s: &str) -> Option<Symbol> {
        self.rodeo.get(s)
    }

    /// Resolve a symbol back to its string
    #[inline]
    #[must_use]
    pub fn resolve(&self, symbol: Symbol) -> &str {
        self.rodeo.resolve(&symbol)
    }

    /// Number of interned strings
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.rodeo.len()
    }

    /// Check if empty
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rodeo.is_empty()
    }
}

impl Default for Interner {
    fn default() -> Self {
        Self::with_common_identifiers()
    }
}

/// Common JavaScript identifiers to pre-intern
const COMMON_IDENTIFIERS: &[&str] = &[
    // Built-in objects
    "Object",
    "Array",
    "String",
    "Number",
    "Boolean",
    "Symbol",
    "Function",
    "Math",
    "Date",
    "RegExp",
    "Error",
    "JSON",
    "Promise",
    "Map",
    "Set",
    "WeakMap",
    "WeakSet",
    "Proxy",
    "Reflect",
    "ArrayBuffer",
    "DataView",
    "Int8Array",
    "Uint8Array",
    "console",
    "window",
    "document",
    "global",
    "globalThis",
    // Common methods
    "prototype",
    "constructor",
    "length",
    "name",
    "message",
    "toString",
    "valueOf",
    "hasOwnProperty",
    "isPrototypeOf",
    "propertyIsEnumerable",
    "toLocaleString",
    "call",
    "apply",
    "bind",
    "push",
    "pop",
    "shift",
    "unshift",
    "slice",
    "splice",
    "concat",
    "join",
    "reverse",
    "sort",
    "indexOf",
    "lastIndexOf",
    "every",
    "some",
    "filter",
    "map",
    "reduce",
    "reduceRight",
    "forEach",
    "find",
    "findIndex",
    "includes",
    "flat",
    "flatMap",
    "keys",
    "values",
    "entries",
    // Common property names
    "value",
    "writable",
    "enumerable",
    "configurable",
    "get",
    "set",
    "then",
    "catch",
    "finally",
    "resolve",
    "reject",
    "all",
    "race",
    "any",
    "allSettled",
    // Other common identifiers
    "undefined",
    "null",
    "NaN",
    "Infinity",
    "arguments",
    "caller",
    "callee",
    "eval",
    "parseInt",
    "parseFloat",
    "isNaN",
    "isFinite",
    "decodeURI",
    "decodeURIComponent",
    "encodeURI",
    "encodeURIComponent",
    "escape",
    "unescape",
    // Module-related
    "default",
    "exports",
    "module",
    "require",
    "__dirname",
    "__filename",
    // Common variable names
    "i",
    "j",
    "k",
    "n",
    "x",
    "y",
    "z",
    "a",
    "b",
    "c",
    "d",
    "e",
    "f",
    "g",
    "h",
    "el",
    "fn",
    "cb",
    "err",
    "res",
    "req",
    "ctx",
    "data",
    "result",
    "item",
    "items",
    "list",
    "arr",
    "obj",
    "key",
    "val",
    "prop",
    "props",
    "state",
    "event",
    "target",
    "type",
    "id",
    "index",
    "count",
    "start",
    "end",
    "left",
    "right",
    "top",
    "bottom",
    "width",
    "height",
    "size",
    "offset",
    "position",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_intern_and_resolve() {
        let mut interner = Interner::new();
        let sym1 = interner.intern("hello");
        let sym2 = interner.intern("hello");
        let sym3 = interner.intern("world");

        // Same string => same symbol
        assert_eq!(sym1, sym2);
        // Different string => different symbol
        assert_ne!(sym1, sym3);

        // Resolve back
        assert_eq!(interner.resolve(sym1), "hello");
        assert_eq!(interner.resolve(sym3), "world");
    }

    #[test]
    fn test_get_existing() {
        let mut interner = Interner::new();
        let sym = interner.intern("test");

        assert_eq!(interner.get("test"), Some(sym));
        assert_eq!(interner.get("nonexistent"), None);
    }

    #[test]
    fn test_common_identifiers() {
        let interner = Interner::with_common_identifiers();

        // Common identifiers should already be interned
        // Note: "function" is a keyword, not an identifier - use Object, console instead
        assert!(interner.get("Object").is_some());
        assert!(interner.get("console").is_some());
        assert!(interner.get("prototype").is_some());
    }
}
