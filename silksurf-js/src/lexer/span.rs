//! Source span tracking for error messages
//!
//! Spans are byte offsets into the source, enabling
//! efficient slicing and error reporting.

/// A span in the source code (byte offsets)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Span {
    /// Start byte offset (inclusive)
    pub start: u32,
    /// End byte offset (exclusive)
    pub end: u32,
}

impl Span {
    /// Create a new span
    #[inline]
    #[must_use]
    pub const fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    /// Create a zero-width span at a position
    #[inline]
    #[must_use]
    pub const fn at(pos: u32) -> Self {
        Self {
            start: pos,
            end: pos,
        }
    }

    /// Length of the span in bytes
    #[inline]
    #[must_use]
    pub const fn len(self) -> u32 {
        self.end - self.start
    }

    /// Check if span is empty
    #[inline]
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.start == self.end
    }

    /// Merge two spans (union)
    #[inline]
    #[must_use]
    pub const fn merge(self, other: Self) -> Self {
        let start = if self.start < other.start {
            self.start
        } else {
            other.start
        };
        let end = if self.end > other.end {
            self.end
        } else {
            other.end
        };
        Self { start, end }
    }

    /// Check if this span contains a byte offset
    #[inline]
    #[must_use]
    pub const fn contains(self, offset: u32) -> bool {
        offset >= self.start && offset < self.end
    }

    /// Extract the spanned text from source
    #[inline]
    #[must_use]
    pub fn text(self, source: &str) -> &str {
        &source[self.start as usize..self.end as usize]
    }
}

/// Line and column information (computed on demand)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineCol {
    /// 1-indexed line number
    pub line: u32,
    /// 1-indexed column number (bytes, not chars)
    pub col: u32,
}

impl LineCol {
    /// Compute line/column from byte offset
    ///
    /// This is O(n) where n is the offset, so cache results
    /// for error reporting rather than calling repeatedly.
    #[must_use]
    pub fn from_offset(source: &str, offset: u32) -> Self {
        let offset = offset as usize;
        let mut line = 1u32;
        let mut col = 1u32;

        for (i, c) in source.char_indices() {
            if i >= offset {
                break;
            }
            if c == '\n' {
                line += 1;
                col = 1;
            } else {
                col += 1;
            }
        }

        Self { line, col }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_span_merge() {
        let a = Span::new(5, 10);
        let b = Span::new(8, 15);
        let merged = a.merge(b);
        assert_eq!(merged.start, 5);
        assert_eq!(merged.end, 15);
    }

    #[test]
    fn test_span_text() {
        let source = "hello world";
        let span = Span::new(0, 5);
        assert_eq!(span.text(source), "hello");
    }

    #[test]
    fn test_linecol() {
        let source = "line1\nline2\nline3";
        let lc = LineCol::from_offset(source, 6); // 'l' of line2
        assert_eq!(lc.line, 2);
        assert_eq!(lc.col, 1);
    }
}
