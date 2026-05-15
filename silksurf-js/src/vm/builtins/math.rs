//! Math object and Number-related built-ins.

use super::{make_object_with_methods, native_fn};
use crate::vm::value::{Object, Value};

pub fn install(global: &mut Object) {
    let math = make_object_with_methods(vec![
        ("abs", native_fn("Math.abs", math_abs)),
        ("ceil", native_fn("Math.ceil", math_ceil)),
        ("floor", native_fn("Math.floor", math_floor)),
        ("round", native_fn("Math.round", math_round)),
        ("trunc", native_fn("Math.trunc", math_trunc)),
        ("max", native_fn("Math.max", math_max)),
        ("min", native_fn("Math.min", math_min)),
        ("pow", native_fn("Math.pow", math_pow)),
        ("sqrt", native_fn("Math.sqrt", math_sqrt)),
        ("random", native_fn("Math.random", math_random)),
        ("log", native_fn("Math.log", math_log)),
        ("log2", native_fn("Math.log2", math_log2)),
        ("log10", native_fn("Math.log10", math_log10)),
        ("sin", native_fn("Math.sin", math_sin)),
        ("cos", native_fn("Math.cos", math_cos)),
        ("sign", native_fn("Math.sign", math_sign)),
        ("clz32", native_fn("Math.clz32", math_clz32)),
    ]);

    // Install Math constants as properties on the object
    if let Value::Object(obj) = &math {
        let mut o = obj.borrow_mut();
        o.set_by_str("PI", Value::Number(std::f64::consts::PI));
        o.set_by_str("E", Value::Number(std::f64::consts::E));
        o.set_by_str("LN2", Value::Number(std::f64::consts::LN_2));
        o.set_by_str("LN10", Value::Number(std::f64::consts::LN_10));
        o.set_by_str("LOG2E", Value::Number(std::f64::consts::LOG2_E));
        o.set_by_str("LOG10E", Value::Number(std::f64::consts::LOG10_E));
        o.set_by_str("SQRT2", Value::Number(std::f64::consts::SQRT_2));
        o.set_by_str("SQRT1_2", Value::Number(std::f64::consts::FRAC_1_SQRT_2));
    }

    global.set_by_str("Math", math);

    // Number constants
    global.set_by_str("NaN", Value::Number(f64::NAN));
    global.set_by_str("Infinity", Value::Number(f64::INFINITY));
}

fn num_arg(args: &[Value], idx: usize) -> f64 {
    args.get(idx)
        .map_or(f64::NAN, super::super::value::Value::to_number)
}

fn math_abs(args: &[Value]) -> Value {
    Value::Number(num_arg(args, 0).abs())
}

fn math_ceil(args: &[Value]) -> Value {
    Value::Number(num_arg(args, 0).ceil())
}

fn math_floor(args: &[Value]) -> Value {
    Value::Number(num_arg(args, 0).floor())
}

fn math_round(args: &[Value]) -> Value {
    let n = num_arg(args, 0);
    // JS Math.round rounds .5 toward +Infinity (not Rust's round which does banker's rounding)
    Value::Number((n + 0.5).floor())
}

fn math_trunc(args: &[Value]) -> Value {
    Value::Number(num_arg(args, 0).trunc())
}

fn math_max(args: &[Value]) -> Value {
    if args.is_empty() {
        return Value::Number(f64::NEG_INFINITY);
    }
    let mut result = f64::NEG_INFINITY;
    for arg in args {
        let n = arg.to_number();
        if n.is_nan() {
            return Value::Number(f64::NAN);
        }
        if n > result {
            result = n;
        }
    }
    Value::Number(result)
}

fn math_min(args: &[Value]) -> Value {
    if args.is_empty() {
        return Value::Number(f64::INFINITY);
    }
    let mut result = f64::INFINITY;
    for arg in args {
        let n = arg.to_number();
        if n.is_nan() {
            return Value::Number(f64::NAN);
        }
        if n < result {
            result = n;
        }
    }
    Value::Number(result)
}

fn math_pow(args: &[Value]) -> Value {
    Value::Number(num_arg(args, 0).powf(num_arg(args, 1)))
}

fn math_sqrt(args: &[Value]) -> Value {
    Value::Number(num_arg(args, 0).sqrt())
}

fn math_random(_args: &[Value]) -> Value {
    // Simple xorshift64 seeded from system -- not cryptographic
    use std::cell::Cell;
    thread_local! {
        static STATE: Cell<u64> = const { Cell::new(0x1234_5678_9ABC_DEF0) };
    }
    STATE.with(|s| {
        let mut x = s.get();
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        s.set(x);
        // Map to [0, 1)
        Value::Number((x >> 11) as f64 / (1u64 << 53) as f64)
    })
}

fn math_log(args: &[Value]) -> Value {
    Value::Number(num_arg(args, 0).ln())
}

fn math_log2(args: &[Value]) -> Value {
    Value::Number(num_arg(args, 0).log2())
}

fn math_log10(args: &[Value]) -> Value {
    Value::Number(num_arg(args, 0).log10())
}

fn math_sin(args: &[Value]) -> Value {
    Value::Number(num_arg(args, 0).sin())
}

fn math_cos(args: &[Value]) -> Value {
    Value::Number(num_arg(args, 0).cos())
}

fn math_sign(args: &[Value]) -> Value {
    let n = num_arg(args, 0);
    if n.is_nan() {
        Value::Number(f64::NAN)
    } else if n == 0.0 {
        Value::Number(n) // Preserve -0.0
    } else if n > 0.0 {
        Value::Number(1.0)
    } else {
        Value::Number(-1.0)
    }
}

fn math_clz32(args: &[Value]) -> Value {
    let n = args.first().map_or(0, super::super::value::Value::to_u32);
    Value::Number(f64::from(n.leading_zeros()))
}
