// test: typeof for common values
if (typeof 1 !== "number") { throw new Error("typeof number"); }
if (typeof "a" !== "string") { throw new Error("typeof string"); }
if (typeof true !== "boolean") { throw new Error("typeof boolean"); }
if (typeof undefined !== "undefined") { throw new Error("typeof undefined"); }
