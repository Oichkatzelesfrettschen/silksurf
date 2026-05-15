// test: function with no explicit return yields undefined
//
// We use `typeof r !== "undefined"` rather than `r !== undefined` because
// the latter exposes a known VM hang on the strict-inequality path when
// one operand is the implicit undefined returned from a void function.
// Once that issue is fixed (tracked separately), this fixture can be
// rewritten to use the more direct comparison.
function noop() { var t = 1; }
var r = noop();
if (typeof r !== "undefined") { throw new Error("expected undefined, got " + typeof r); }
