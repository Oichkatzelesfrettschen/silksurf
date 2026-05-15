// test: function call with return value
function add(a, b) { return a + b; }
var r = add(3, 4);
if (r !== 7) { throw new Error("expected 7, got " + r); }
