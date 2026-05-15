# SilkSurf Synthetic Test262 Fixtures

WHY: The upstream tc39/test262 corpus is ~50,000 files (hundreds of MB of
text). Vendoring it just to measure baseline VM coverage is wasteful and
slows every checkout. This directory holds a small SYNTHETIC fixture set
exercising the JavaScript constructs the SilkSurf VM is expected to
handle in early-phase conformance work.

WHAT: Each `*.js` file is a self-contained ECMAScript test that:
  - completes normally on success, OR
  - throws an Error (or otherwise causes the VM to surface a fault)
    on failure.

The runner (silksurf-js/src/bin/test262.rs) walks this directory,
executes each script with a fresh `Vm`, and counts a script PASS if
execution returns Ok and FAIL if it throws/errors/panics.

HOW: Add new fixtures by dropping a `*.js` file in this directory. Keep
each fixture under ~20 lines and exercising a single concept; that way a
regression report points at exactly one feature.

Categories covered today:
  arithmetic, boolean logic, typeof, string concat, var/let/const,
  if/else, for loops, while loops, function calls, return values,
  closures, try/catch, simple objects, simple arrays.

This is NOT a substitute for the real test262. Once the engine handles
enough of the language to make per-file iteration tractable, replace
this directory with a vendored subset of tc39/test262.
