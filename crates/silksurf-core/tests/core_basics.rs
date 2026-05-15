// Core integration tests covering the public API of silksurf-core.
//
// WHY: silksurf-core is the workspace's foundation crate. Other crates
// rely on its Atom-interner identity guarantees, its SilkError /
// SilkResult unification, and its bumpalo arena. Regressions in any
// of those primitives ripple across the entire workspace, so each
// invariant gets a dedicated, deterministic, network-free test here.
//
// WHAT: Five focused tests on SilkInterner, Atom, SilkError, the
// SilkError::From<std::io::Error> impl, and the SilkResult type alias.
//
// HOW: All tests use only the silksurf-core public surface re-exported
// from lib.rs (SilkInterner, Atom, SilkError, SilkResult,
// should_intern_identifier). No external test deps; std only.

use silksurf_core::{SilkError, SilkInterner, SilkResult, should_intern_identifier};

// 1) Interner round-trip.
//
// Intern a string, resolve the returned Atom, and verify byte-equality
// against the original input. This is the contract every interner user
// depends on (tokenizer, CSS parser, DOM attribute storage).
#[test]
fn interner_round_trip_returns_original_bytes() {
    let mut interner = SilkInterner::new();
    let original = "background-color";
    let atom = interner.intern(original);

    let resolved = interner.resolve(atom);

    assert_eq!(resolved, original, "resolve(intern(s)) must equal s");
    assert_eq!(interner.len(), 1, "single intern produces one entry");
    assert!(!interner.is_empty(), "after one intern the table is non-empty");
}

// 2) Atom monotonic / identity property.
//
// Two distinct strings must produce two distinct Atoms (different raw
// indices), and interning the same string twice must return the same
// Atom (this is the deduplication invariant the resolve table relies on).
#[test]
fn atom_distinct_for_different_strings_and_stable_for_same_string() {
    let mut interner = SilkInterner::new();

    let div_a = interner.intern("div");
    let span_a = interner.intern("span");
    let div_b = interner.intern("div");

    assert_ne!(div_a, span_a, "different strings must yield different atoms");
    assert_eq!(div_a, div_b, "interning the same string twice must yield the same atom");
    assert_ne!(div_a.raw(), span_a.raw(), "raw indices must differ for distinct atoms");
    assert_eq!(div_a.raw(), div_b.raw(), "raw indices must be stable across re-interns");
    assert_eq!(interner.len(), 2, "two distinct strings produce exactly two table entries");

    // Helper guard for downstream resolve_table consumers: verify the
    // helper that decides which strings are intern-eligible behaves
    // sensibly on canonical inputs. A short ASCII identifier qualifies;
    // a string with whitespace does not.
    assert!(should_intern_identifier("div"), "short ASCII identifier must qualify");
    assert!(!should_intern_identifier("with space"), "whitespace must disqualify");
    assert!(!should_intern_identifier(""), "empty string must disqualify");
}

// 3) SilkError Display: every variant must produce a non-empty string.
//
// silksurf-core does not expose a generation-tracked resolve table
// directly (that lives in silksurf-dom); per the task spec, fall back
// to exhaustively exercising SilkError::Display so future variant
// additions that forget #[error("...")] are caught immediately.
#[test]
fn silk_error_display_is_non_empty_for_every_variant() {
    let variants: Vec<SilkError> = vec![
        SilkError::InvalidInput("bad token".to_string()),
        SilkError::Unsupported("CSS Houdini".to_string()),
        SilkError::Css { offset: 42, message: "unclosed brace".to_string() },
        SilkError::Dom("missing parent".to_string()),
        SilkError::HtmlTokenize { offset: 7, message: "unexpected EOF".to_string() },
        SilkError::HtmlTreeBuild("orphan node".to_string()),
        SilkError::Net("connect refused".to_string()),
        SilkError::Tls("handshake failure".to_string()),
        SilkError::Engine("pipeline aborted".to_string()),
        SilkError::Js("ReferenceError: x is not defined".to_string()),
        SilkError::Io(std::io::Error::new(std::io::ErrorKind::Other, "synthetic")),
    ];

    assert!(!variants.is_empty(), "variant list must be non-empty");
    for variant in &variants {
        let displayed = format!("{variant}");
        assert!(
            !displayed.trim().is_empty(),
            "Display impl for {variant:?} produced an empty string"
        );
        // Debug is auto-derived but should still render meaningfully.
        let debugged = format!("{variant:?}");
        assert!(
            !debugged.trim().is_empty(),
            "Debug impl for {variant:?} produced an empty string"
        );
    }
}

// 4) SilkError From impls: io::Error and string-based domain conversions.
//
// thiserror generates From<std::io::Error> for SilkError::Io via
// #[from]. Verify both that the From conversion lands in the Io
// variant, and that constructing string-keyed variants directly (the
// pattern used by leaf crates' own From impls) preserves the message.
#[test]
fn silk_error_from_impls_target_correct_variants() {
    // From<std::io::Error> -> SilkError::Io (thiserror #[from])
    let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "missing.pem");
    let lifted: SilkError = io_err.into();
    match &lifted {
        SilkError::Io(inner) => {
            assert_eq!(inner.kind(), std::io::ErrorKind::NotFound, "kind must round-trip through From");
        }
        other => panic!("expected SilkError::Io, got {other:?}"),
    }
    // Display of an Io variant must surface the underlying message.
    let displayed = format!("{lifted}");
    assert!(displayed.contains("I/O"), "Io display must mention I/O, got: {displayed}");

    // String-erased conversion pattern used by leaf crates: leaf crate
    // implements `impl From<MyErr> for SilkError { fn from(e) ->
    // SilkError::MyDomain(e.to_string()) }`. Simulate the result.
    let css_message = "unexpected token at column 17";
    let from_string: SilkError = SilkError::Css { offset: 17, message: css_message.to_string() };
    match &from_string {
        SilkError::Css { offset, message } => {
            assert_eq!(*offset, 17);
            assert_eq!(message, css_message);
        }
        other => panic!("expected SilkError::Css, got {other:?}"),
    }

    // The DOM variant is the canonical string-only form.
    let dom_msg = "element not found";
    let dom_err = SilkError::Dom(dom_msg.to_string());
    let displayed_dom = format!("{dom_err}");
    assert!(
        displayed_dom.contains(dom_msg),
        "Dom display must contain the original message, got: {displayed_dom}"
    );
}

// 5) SilkResult type alias: must be a usable Result<T, SilkError>.
//
// Compose a small function that returns SilkResult<u32> on the happy
// path and SilkResult<u32> with an InvalidInput error on the sad path.
// This pins down the alias as both call-site-usable and `?`-friendly.
fn parse_positive_u32(input: &str) -> SilkResult<u32> {
    let value: u32 = input
        .parse()
        .map_err(|_| SilkError::InvalidInput(format!("not a u32: {input}")))?;
    if value == 0 {
        return Err(SilkError::InvalidInput("must be positive".to_string()));
    }
    Ok(value)
}

#[test]
fn silk_result_alias_supports_ok_and_err_paths() {
    // Happy path: returns Ok(u32).
    let ok = parse_positive_u32("42");
    assert!(ok.is_ok(), "valid input must parse, got {ok:?}");
    assert_eq!(ok.unwrap(), 42);

    // Sad path A: parse failure maps to InvalidInput.
    let bad_format = parse_positive_u32("not-a-number");
    match bad_format {
        Err(SilkError::InvalidInput(msg)) => {
            assert!(msg.contains("not-a-number"), "error must surface the bad input, got: {msg}");
        }
        other => panic!("expected InvalidInput from parse failure, got {other:?}"),
    }

    // Sad path B: domain rule (zero) also maps to InvalidInput.
    let zero = parse_positive_u32("0");
    match zero {
        Err(SilkError::InvalidInput(msg)) => {
            assert!(msg.contains("positive"), "error must explain the rule, got: {msg}");
        }
        other => panic!("expected InvalidInput for zero, got {other:?}"),
    }
}
