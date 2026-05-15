// test: string concatenation with +
var s = "hello" + " " + "world";
if (s !== "hello world") { throw new Error("expected 'hello world', got " + s); }
