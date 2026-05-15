// test: closure captures outer variable
function makeAdder(x) {
    return function (y) { return x + y; };
}
var add5 = makeAdder(5);
var r = add5(10);
if (r !== 15) { throw new Error("expected 15, got " + r); }
