use silksurf_core::{SilkArena, SilkInterner, Span};

#[test]
fn interner_round_trip() {
    let mut interner = SilkInterner::new();
    let a = interner.intern("html");
    let b = interner.intern("html");

    assert_eq!(a, b);
    assert_eq!(interner.resolve(a), "html");
    assert_eq!(interner.len(), 1);
}

#[test]
fn arena_allocates_values() {
    let arena = SilkArena::new();
    let value = arena.alloc(42u32);
    let text = arena.alloc_str("token");

    assert_eq!(*value, 42);
    assert_eq!(text, "token");
}

#[test]
fn span_length() {
    let span = Span::new(3, 8);
    assert_eq!(span.len(), 5);
}
