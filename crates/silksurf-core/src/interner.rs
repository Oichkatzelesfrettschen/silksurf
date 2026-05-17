use std::collections::HashMap;

use crate::SmallString;

/*
 * Atom -- opaque handle into the SilkInterner string table.
 *
 * Internally a u32 index into SilkInterner::values. Sequential assignment
 * guarantees atoms from the same interner form a dense range [0..N).
 * raw() exposes the index for direct array access in the resolve table.
 */
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct Atom(u32);

impl Atom {
    /// Raw index for direct array access into a materialized resolve table.
    /// SAFETY: only valid against the interner that created this atom.
    #[inline]
    #[must_use] 
    pub fn raw(self) -> u32 {
        self.0
    }
}

pub struct SilkInterner {
    ids: HashMap<SmallString, Atom>,
    values: Vec<SmallString>,
}

impl SilkInterner {
    #[must_use] 
    pub fn new() -> Self {
        Self {
            ids: HashMap::new(),
            values: Vec::new(),
        }
    }

    pub fn intern(&mut self, value: &str) -> Atom {
        if let Some(existing) = self.ids.get(value) {
            return *existing;
        }

        let atom = Atom(self.values.len() as u32);
        let owned = SmallString::from(value);
        self.values.push(owned.clone());
        self.ids.insert(owned, atom);
        atom
    }

    pub fn resolve(&self, symbol: Atom) -> &str {
        // UNWRAP-OK: Atoms are only constructed by intern() (this module), which inserts into
        // self.values before returning, so every valid Atom maps to a present index. A panic
        // here means the caller forged an Atom or used one across interners (programmer bug).
        self.values
            .get(symbol.0 as usize)
            .map(SmallString::as_str)
            .expect("invalid Atom: symbol not found in interner")
    }

    #[must_use] 
    pub fn len(&self) -> usize {
        self.values.len()
    }

    #[must_use] 
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Direct slice access to all interned values, indexed by `Atom::raw()`.
    /// Used by `Dom::materialize_resolve_table()` to bulk-copy new atoms
    /// into the lock-free resolve table without per-atom `RwLock` acquire.
    #[must_use] 
    pub fn values_slice(&self) -> &[SmallString] {
        &self.values
    }
}

impl Default for SilkInterner {
    fn default() -> Self {
        Self::new()
    }
}

const MAX_INTERNED_LEN: usize = 24;

pub fn should_intern_identifier(value: &str) -> bool {
    let len = value.len();
    if len == 0 || len > MAX_INTERNED_LEN {
        return false;
    }
    if !value.is_ascii() {
        return false;
    }
    !value
        .as_bytes()
        .iter()
        .any(u8::is_ascii_whitespace)
}
