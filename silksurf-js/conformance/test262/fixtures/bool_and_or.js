// test: boolean and/or short-circuit
if (!(true && true)) { throw new Error("&& true&&true"); }
if (true && false) { throw new Error("&& true&&false should be falsy"); }
if (!(false || true)) { throw new Error("|| false||true"); }
if (false || false) { throw new Error("|| false||false should be falsy"); }
