use crate::ArenaVec;
use bumpalo::Bump;

pub struct SilkArena {
    bump: Bump,
}

impl SilkArena {
    #[must_use] 
    pub fn new() -> Self {
        Self { bump: Bump::new() }
    }

    pub fn alloc<T>(&self, value: T) -> &mut T {
        self.bump.alloc(value)
    }

    pub fn alloc_str(&self, value: &str) -> &mut str {
        self.bump.alloc_str(value)
    }

    pub fn vec<T>(&self) -> ArenaVec<'_, T> {
        ArenaVec::new_in(&self.bump)
    }

    pub fn reset(&mut self) {
        self.bump.reset();
    }
}

impl Default for SilkArena {
    fn default() -> Self {
        Self::new()
    }
}
