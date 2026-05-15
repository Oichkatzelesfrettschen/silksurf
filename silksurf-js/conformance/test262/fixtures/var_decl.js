// test: var declaration with initialiser
var a = 7;
var b = a;
if (b !== 7) { throw new Error("expected b===7, got " + b); }
