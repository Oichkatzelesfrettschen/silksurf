#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    #[must_use] 
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    #[must_use] 
    pub fn len(&self) -> usize {
        self.end.saturating_sub(self.start)
    }

    #[must_use] 
    pub fn is_empty(&self) -> bool {
        self.start >= self.end
    }
}
