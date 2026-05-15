// test: simple array literal indexing
var a = [10, 20, 30];
if (a[0] !== 10) { throw new Error("a[0] expected 10, got " + a[0]); }
if (a[2] !== 30) { throw new Error("a[2] expected 30, got " + a[2]); }
