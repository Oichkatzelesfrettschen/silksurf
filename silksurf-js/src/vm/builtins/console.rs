//! console object (log, warn, error, info, debug)

use super::{make_object_with_methods, native_fn};
use crate::vm::value::{Object, Value};

pub fn install(global: &mut Object) {
    let console = make_object_with_methods(vec![
        ("log", native_fn("console.log", console_log)),
        ("warn", native_fn("console.warn", console_warn)),
        ("error", native_fn("console.error", console_error)),
        ("info", native_fn("console.info", console_log)),
        ("debug", native_fn("console.debug", console_log)),
    ]);
    global.set_by_str("console", console);
}

fn format_args(args: &[Value]) -> String {
    args.iter()
        .map(|arg| {
            let s = arg.to_js_string();
            s.as_str().unwrap_or("[interned]").to_string()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn console_log(args: &[Value]) -> Value {
    eprintln!("[LOG] {}", format_args(args));
    Value::Undefined
}

fn console_warn(args: &[Value]) -> Value {
    eprintln!("[WARN] {}", format_args(args));
    Value::Undefined
}

fn console_error(args: &[Value]) -> Value {
    eprintln!("[ERROR] {}", format_args(args));
    Value::Undefined
}
