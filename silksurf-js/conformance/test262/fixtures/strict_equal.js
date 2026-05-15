// test: === and !== distinguish types
if (1 === "1") { throw new Error("1 === '1' should be false"); }
if (!(1 !== "1")) { throw new Error("1 !== '1' should be true"); }
if (!(1 === 1)) { throw new Error("1 === 1 should be true"); }
