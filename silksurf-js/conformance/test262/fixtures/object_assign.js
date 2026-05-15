// test: assign new property to object
var o = { a: 1 };
o.b = 5;
if (o.b !== 5) { throw new Error("o.b expected 5, got " + o.b); }
