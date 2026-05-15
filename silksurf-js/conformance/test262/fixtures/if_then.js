// test: if-true branch executes
var x = 0;
if (1 < 2) { x = 1; }
if (x !== 1) { throw new Error("expected 1, got " + x); }
