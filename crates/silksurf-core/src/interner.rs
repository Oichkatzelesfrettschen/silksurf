use lasso::{Rodeo, Spur};

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct Atom(Spur);

pub struct SilkInterner {
    rodeo: Rodeo,
}

impl SilkInterner {
    pub fn new() -> Self {
        Self {
            rodeo: Rodeo::default(),
        }
    }

    pub fn intern(&mut self, value: &str) -> Atom {
        Atom(self.rodeo.get_or_intern(value))
    }

    pub fn resolve(&self, symbol: Atom) -> &str {
        self.rodeo.resolve(&symbol.0)
    }

    pub fn len(&self) -> usize {
        self.rodeo.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rodeo.is_empty()
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
