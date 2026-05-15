// test: while loop counts down
var n = 5;
var iter = 0;
while (n > 0) { n = n - 1; iter = iter + 1; }
if (iter !== 5) { throw new Error("expected 5 iterations, got " + iter); }
