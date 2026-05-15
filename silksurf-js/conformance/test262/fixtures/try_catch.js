// test: try/catch catches a thrown error
var caught = false;
try {
    throw new Error("boom");
} catch (e) {
    caught = true;
}
if (!caught) { throw new Error("catch did not run"); }
