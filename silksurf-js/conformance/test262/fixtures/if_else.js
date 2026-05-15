// test: if-false takes else branch
var x = 0;
if (1 > 2) { x = 1; } else { x = 2; }
if (x !== 2) { throw new Error("expected 2, got " + x); }
