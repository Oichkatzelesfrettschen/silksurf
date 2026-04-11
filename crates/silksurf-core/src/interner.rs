use std::collections::HashMap;

use crate::SmallString;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct Atom(u32);

pub struct SilkInterner {
    ids: HashMap<SmallString, Atom>,
    values: Vec<SmallString>,
}

impl SilkInterner {
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
        self.values
            .get(symbol.0 as usize)
            .map(SmallString::as_str)
            .expect("invalid Atom: symbol not found in interner")
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
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
        .any(|byte| byte.is_ascii_whitespace())
}
