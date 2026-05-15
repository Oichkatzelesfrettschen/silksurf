/*
 * typography.rs -- integration tests for Unicode text-analysis helpers.
 *
 * WHY: Verify that bidi_level() and linebreak_opportunities() expose the
 * underlying crate behaviour correctly at the boundary we have adopted in
 * lib.rs.  These tests are the acceptance gate for AD-023; they must pass
 * before full render-pipeline integration can begin.
 *
 * UAX #9  -- Unicode Bidirectional Algorithm (bidi_level)
 * UAX #14 -- Unicode Line Breaking Algorithm (linebreak_opportunities)
 */

use silksurf_layout::{bidi_level, linebreak_opportunities};

// bidi_level_for_ltr_text
//
// WHY: ASCII text has no RTL characters, so the paragraph embedding level
// must be 0 (LTR) by UAX #9 rules.
#[test]
fn bidi_level_for_ltr_text() {
    assert_eq!(
        bidi_level("Hello"),
        0,
        "pure ASCII must resolve to LTR (level 0)"
    );
}

// bidi_level_for_rtl_text
//
// WHY: The string below contains only Arabic characters (\u{0645}\u{0631}\u{062D}\u{0628}\u{0627})
// which are strongly RTL.  UAX #9 must assign paragraph embedding level 1.
// The literal is written with raw \u escapes to satisfy the ASCII-only policy.
#[test]
fn bidi_level_for_rtl_text() {
    // Arabic word "marhaba" (hello) -- five strongly-RTL code points.
    let rtl = "\u{0645}\u{0631}\u{062D}\u{0628}\u{0627}";
    assert_eq!(
        bidi_level(rtl),
        1,
        "strongly-RTL text must resolve to embedding level 1"
    );
}

// linebreak_opportunities_basic
//
// WHY: "Hello world" has a legal break opportunity after "Hello " (i.e. at
// byte offset 6).  The function must return at least one break so that the
// inline-layout pass can wrap lines.
#[test]
fn linebreak_opportunities_basic() {
    let breaks = linebreak_opportunities("Hello world");
    assert!(
        !breaks.is_empty(),
        "linebreak_opportunities must return at least one break for 'Hello world'"
    );
    // Byte offset 6 is after the space following "Hello"; UAX #14 permits a
    // break there.  Mandatory end-of-text break at 11 is also expected.
    assert!(
        breaks.contains(&6),
        "expected a break opportunity at byte 6 (after 'Hello '), got {:?}",
        breaks
    );
}
