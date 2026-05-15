// test: simple object literal property access
var o = { a: 1, b: 2 };
if (o.a !== 1) { throw new Error("o.a expected 1, got " + o.a); }
if (o.b !== 2) { throw new Error("o.b expected 2, got " + o.b); }
