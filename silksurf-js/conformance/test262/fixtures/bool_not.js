// test: logical NOT
if (!true !== false) { throw new Error("!true !== false"); }
if (!false !== true) { throw new Error("!false !== true"); }
if (!!1 !== true) { throw new Error("!!1 !== true"); }
