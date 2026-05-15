// test: classic for loop sums 1..10
var sum = 0;
for (var i = 1; i <= 10; i = i + 1) { sum = sum + i; }
if (sum !== 55) { throw new Error("expected 55, got " + sum); }
