use bumpalo::Bump;

pub struct SilkArena {
    bump: Bump,
}

impl SilkArena {
    pub fn new() -> Self {
        Self { bump: Bump::new() }
    }

    pub fn alloc<T>(&self, value: T) -> &mut T {
        self.bump.alloc(value)
    }

    pub fn alloc_str(&self, value: &str) -> &mut str {
        self.bump.alloc_str(value)
    }
}
