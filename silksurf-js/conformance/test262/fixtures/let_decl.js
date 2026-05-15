// test: let declaration
let a = 11;
let b = a + 1;
if (b !== 12) { throw new Error("expected 12, got " + b); }
