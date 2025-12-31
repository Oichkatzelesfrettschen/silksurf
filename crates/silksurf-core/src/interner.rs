use lasso::{Rodeo, Spur};

pub struct SilkInterner {
    rodeo: Rodeo,
}

impl SilkInterner {
    pub fn new() -> Self {
        Self { rodeo: Rodeo::default() }
    }

    pub fn intern(&mut self, value: &str) -> Spur {
        self.rodeo.get_or_intern(value)
    }

    pub fn resolve(&self, symbol: Spur) -> &str {
        self.rodeo.resolve(&symbol)
    }

    pub fn len(&self) -> usize {
        self.rodeo.len()
    }
}
