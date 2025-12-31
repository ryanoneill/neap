//! Source span tracking for error reporting
//!
//! Spans track the location of tokens and AST nodes in source code,
//! enabling precise error messages with source context.

/// A span representing a range in source code.
///
/// Spans use byte offsets from the start of the source file.
/// The range is half-open: [start, end).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Span {
    /// Byte offset of the start of this span (inclusive)
    pub start: usize,
    /// Byte offset of the end of this span (exclusive)
    pub end: usize,
}

impl Span {
    /// Create a new span from start and end byte offsets.
    #[must_use]
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// Create an empty span at a given position.
    #[must_use]
    pub const fn empty(pos: usize) -> Self {
        Self {
            start: pos,
            end: pos,
        }
    }

    /// Create a span covering a single byte.
    #[must_use]
    pub const fn single(pos: usize) -> Self {
        Self {
            start: pos,
            end: pos + 1,
        }
    }

    /// Returns the length of this span in bytes.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.end - self.start
    }

    /// Returns true if this span has zero length.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// Merge two spans into one that covers both.
    ///
    /// The resulting span covers from the minimum start to the maximum end.
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

    /// Extend this span to include another span.
    pub fn extend(&mut self, other: Self) {
        if other.start < self.start {
            self.start = other.start;
        }
        if other.end > self.end {
            self.end = other.end;
        }
    }

    /// Check if this span contains a byte offset.
    #[must_use]
    pub const fn contains(&self, offset: usize) -> bool {
        offset >= self.start && offset < self.end
    }

    /// Check if this span overlaps with another span.
    #[must_use]
    pub const fn overlaps(&self, other: &Self) -> bool {
        self.start < other.end && other.start < self.end
    }
}

/// A value paired with its source span.
///
/// This is used to attach location information to tokens, AST nodes,
/// and other syntax elements.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Spanned<T> {
    /// The wrapped value
    pub value: T,
    /// The source span where this value was found
    pub span: Span,
}

impl<T> Spanned<T> {
    /// Create a new spanned value.
    #[must_use]
    pub const fn new(value: T, span: Span) -> Self {
        Self { value, span }
    }

    /// Map a function over the inner value, preserving the span.
    #[must_use]
    pub fn map<U, F: FnOnce(T) -> U>(self, f: F) -> Spanned<U> {
        Spanned {
            value: f(self.value),
            span: self.span,
        }
    }

    /// Get a reference to the inner value.
    #[must_use]
    pub const fn as_ref(&self) -> Spanned<&T> {
        Spanned {
            value: &self.value,
            span: self.span,
        }
    }

    /// Unwrap the spanned value, discarding the span.
    #[must_use]
    pub fn into_inner(self) -> T {
        self.value
    }
}

impl<T: Default> Default for Spanned<T> {
    fn default() -> Self {
        Self {
            value: T::default(),
            span: Span::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn span_new() {
        let span = Span::new(10, 20);
        assert_eq!(span.start, 10);
        assert_eq!(span.end, 20);
        assert_eq!(span.len(), 10);
        assert!(!span.is_empty());
    }

    #[test]
    fn span_empty() {
        let span = Span::empty(5);
        assert_eq!(span.start, 5);
        assert_eq!(span.end, 5);
        assert_eq!(span.len(), 0);
        assert!(span.is_empty());
    }

    #[test]
    fn span_single() {
        let span = Span::single(7);
        assert_eq!(span.start, 7);
        assert_eq!(span.end, 8);
        assert_eq!(span.len(), 1);
    }

    #[test]
    fn span_merge() {
        let a = Span::new(5, 10);
        let b = Span::new(8, 15);
        let merged = a.merge(b);
        assert_eq!(merged.start, 5);
        assert_eq!(merged.end, 15);
    }

    #[test]
    fn span_merge_disjoint() {
        let a = Span::new(0, 5);
        let b = Span::new(10, 15);
        let merged = a.merge(b);
        assert_eq!(merged.start, 0);
        assert_eq!(merged.end, 15);
    }

    #[test]
    fn span_extend() {
        let mut span = Span::new(5, 10);
        span.extend(Span::new(3, 12));
        assert_eq!(span.start, 3);
        assert_eq!(span.end, 12);
    }

    #[test]
    fn span_contains() {
        let span = Span::new(5, 10);
        assert!(!span.contains(4));
        assert!(span.contains(5));
        assert!(span.contains(7));
        assert!(span.contains(9));
        assert!(!span.contains(10));
    }

    #[test]
    fn span_overlaps() {
        let a = Span::new(5, 10);
        let b = Span::new(8, 15);
        let c = Span::new(10, 15);
        let d = Span::new(0, 5);

        assert!(a.overlaps(&b));
        assert!(b.overlaps(&a));
        assert!(!a.overlaps(&c)); // Adjacent but not overlapping
        assert!(!a.overlaps(&d)); // Adjacent but not overlapping
    }

    #[test]
    fn spanned_new() {
        let spanned = Spanned::new(42, Span::new(0, 2));
        assert_eq!(spanned.value, 42);
        assert_eq!(spanned.span, Span::new(0, 2));
    }

    #[test]
    fn spanned_map() {
        let spanned = Spanned::new(42, Span::new(0, 2));
        let doubled = spanned.map(|x| x * 2);
        assert_eq!(doubled.value, 84);
        assert_eq!(doubled.span, Span::new(0, 2));
    }

    #[test]
    fn spanned_into_inner() {
        let spanned = Spanned::new("hello", Span::new(0, 5));
        assert_eq!(spanned.into_inner(), "hello");
    }
}
