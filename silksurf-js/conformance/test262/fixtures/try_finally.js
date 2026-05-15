// test: try/finally always runs the finally block
var ran = false;
try {
    var x = 1;
} finally {
    ran = true;
}
if (!ran) { throw new Error("finally did not run"); }
