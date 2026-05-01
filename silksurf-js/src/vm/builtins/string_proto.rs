//! String prototype methods.
//!
//! Dispatched when accessing properties on string values in op_get_prop.

use std::rc::Rc;

use crate::vm::builtins::array::create_array;
use crate::vm::string::JsString;
use crate::vm::value::{NativeFunction, Value};

/// Look up a string method by name. Returns a NativeFunction Value if found.
pub fn get_string_method(s: &Rc<JsString>, name: &str) -> Option<Value> {
    let text = s.as_str().unwrap_or("").to_string();
    let method: Option<Box<dyn Fn(&[Value]) -> Value>> = match name {
        "length" => return Some(Value::Number(text.chars().count() as f64)),
        "charAt" => Some(Box::new(move |args: &[Value]| {
            let idx = args.first().map_or(0, |v| v.to_number() as usize);
            text.chars()
                .nth(idx)
                .map(|c| Value::string(&c.to_string()))
                .unwrap_or_else(|| Value::string(""))
        })),
        "charCodeAt" => Some(Box::new(move |args: &[Value]| {
            let idx = args.first().map_or(0, |v| v.to_number() as usize);
            text.chars()
                .nth(idx)
                .map(|c| Value::Number(c as u32 as f64))
                .unwrap_or(Value::Number(f64::NAN))
        })),
        "indexOf" => Some(Box::new(move |args: &[Value]| {
            let search = args
                .first()
                .map(|v| {
                    let s = v.to_js_string();
                    s.as_str().unwrap_or("").to_string()
                })
                .unwrap_or_default();
            let from = args.get(1).map_or(0, |v| v.to_number().max(0.0) as usize);
            if from > text.len() {
                return Value::Number(-1.0);
            }
            text[from..]
                .find(&search)
                .map(|pos| Value::Number((pos + from) as f64))
                .unwrap_or(Value::Number(-1.0))
        })),
        "lastIndexOf" => Some(Box::new(move |args: &[Value]| {
            let search = args
                .first()
                .map(|v| {
                    let s = v.to_js_string();
                    s.as_str().unwrap_or("").to_string()
                })
                .unwrap_or_default();
            text.rfind(&search)
                .map(|pos| Value::Number(pos as f64))
                .unwrap_or(Value::Number(-1.0))
        })),
        "includes" => Some(Box::new(move |args: &[Value]| {
            let search = args
                .first()
                .map(|v| {
                    let s = v.to_js_string();
                    s.as_str().unwrap_or("").to_string()
                })
                .unwrap_or_default();
            Value::Boolean(text.contains(&search))
        })),
        "startsWith" => Some(Box::new(move |args: &[Value]| {
            let prefix = args
                .first()
                .map(|v| {
                    let s = v.to_js_string();
                    s.as_str().unwrap_or("").to_string()
                })
                .unwrap_or_default();
            Value::Boolean(text.starts_with(&prefix))
        })),
        "endsWith" => Some(Box::new(move |args: &[Value]| {
            let suffix = args
                .first()
                .map(|v| {
                    let s = v.to_js_string();
                    s.as_str().unwrap_or("").to_string()
                })
                .unwrap_or_default();
            Value::Boolean(text.ends_with(&suffix))
        })),
        "slice" => Some(Box::new(move |args: &[Value]| {
            let len = text.len() as i64;
            let start = args.first().map_or(0i64, |v| v.to_number() as i64);
            let start = if start < 0 {
                (len + start).max(0) as usize
            } else {
                (start as usize).min(text.len())
            };
            let end = args.get(1).map_or(len, |v| v.to_number() as i64);
            let end = if end < 0 {
                (len + end).max(0) as usize
            } else {
                (end as usize).min(text.len())
            };
            if start >= end {
                Value::string("")
            } else {
                Value::string(&text[start..end])
            }
        })),
        "substring" => Some(Box::new(move |args: &[Value]| {
            let len = text.len();
            let start = args
                .first()
                .map_or(0, |v| (v.to_number().max(0.0) as usize).min(len));
            let end = args
                .get(1)
                .map_or(len, |v| (v.to_number().max(0.0) as usize).min(len));
            let (start, end) = if start > end {
                (end, start)
            } else {
                (start, end)
            };
            Value::string(&text[start..end])
        })),
        "toLowerCase" | "toLocaleLowerCase" => Some(Box::new(move |_args: &[Value]| {
            Value::string_owned(text.to_lowercase())
        })),
        "toUpperCase" | "toLocaleUpperCase" => Some(Box::new(move |_args: &[Value]| {
            Value::string_owned(text.to_uppercase())
        })),
        "trim" => Some(Box::new(move |_args: &[Value]| Value::string(text.trim()))),
        "trimStart" => Some(Box::new(move |_args: &[Value]| {
            Value::string(text.trim_start())
        })),
        "trimEnd" => Some(Box::new(move |_args: &[Value]| {
            Value::string(text.trim_end())
        })),
        "split" => Some(Box::new(move |args: &[Value]| {
            let sep = args
                .first()
                .map(|v| {
                    let s = v.to_js_string();
                    s.as_str().unwrap_or("").to_string()
                })
                .unwrap_or_default();
            let limit = args.get(1).and_then(|v| {
                let n = v.to_number();
                if n.is_finite() && n >= 0.0 {
                    Some(n as usize)
                } else {
                    None
                }
            });
            let parts: Vec<Value> = if sep.is_empty() {
                // Split into individual chars
                let chars: Vec<Value> = text
                    .chars()
                    .map(|c| Value::string(&c.to_string()))
                    .collect();
                if let Some(limit) = limit {
                    chars.into_iter().take(limit).collect()
                } else {
                    chars
                }
            } else if let Some(limit) = limit {
                text.splitn(limit, &*sep)
                    .map(|part| Value::string(part))
                    .collect()
            } else {
                text.split(&*sep).map(|part| Value::string(part)).collect()
            };
            create_array(parts)
        })),
        "replace" => Some(Box::new(move |args: &[Value]| {
            let search = args
                .first()
                .map(|v| {
                    let s = v.to_js_string();
                    s.as_str().unwrap_or("").to_string()
                })
                .unwrap_or_default();
            let replacement = args
                .get(1)
                .map(|v| {
                    let s = v.to_js_string();
                    s.as_str().unwrap_or("").to_string()
                })
                .unwrap_or_default();
            // Replace first occurrence only (like JS without /g flag)
            Value::string_owned(text.replacen(&search, &replacement, 1))
        })),
        "replaceAll" => Some(Box::new(move |args: &[Value]| {
            let search = args
                .first()
                .map(|v| {
                    let s = v.to_js_string();
                    s.as_str().unwrap_or("").to_string()
                })
                .unwrap_or_default();
            let replacement = args
                .get(1)
                .map(|v| {
                    let s = v.to_js_string();
                    s.as_str().unwrap_or("").to_string()
                })
                .unwrap_or_default();
            Value::string_owned(text.replace(&search, &replacement))
        })),
        "repeat" => Some(Box::new(move |args: &[Value]| {
            let count = args.first().map_or(0, |v| {
                let n = v.to_number();
                if n.is_finite() && n >= 0.0 {
                    n as usize
                } else {
                    0
                }
            });
            if count > 1_000_000 {
                return Value::string(""); // Prevent OOM
            }
            Value::string_owned(text.repeat(count))
        })),
        "padStart" => Some(Box::new(move |args: &[Value]| {
            let target_len = args.first().map_or(0, |v| v.to_number().max(0.0) as usize);
            let pad = args
                .get(1)
                .map(|v| {
                    let s = v.to_js_string();
                    s.as_str().unwrap_or(" ").to_string()
                })
                .unwrap_or_else(|| " ".to_string());
            if text.len() >= target_len || pad.is_empty() {
                return Value::string(&text);
            }
            let needed = target_len - text.len();
            let padding: String = pad.chars().cycle().take(needed).collect();
            Value::string_owned(format!("{padding}{text}"))
        })),
        "padEnd" => Some(Box::new(move |args: &[Value]| {
            let target_len = args.first().map_or(0, |v| v.to_number().max(0.0) as usize);
            let pad = args
                .get(1)
                .map(|v| {
                    let s = v.to_js_string();
                    s.as_str().unwrap_or(" ").to_string()
                })
                .unwrap_or_else(|| " ".to_string());
            if text.len() >= target_len || pad.is_empty() {
                return Value::string(&text);
            }
            let needed = target_len - text.len();
            let padding: String = pad.chars().cycle().take(needed).collect();
            Value::string_owned(format!("{text}{padding}"))
        })),
        "at" => Some(Box::new(move |args: &[Value]| {
            let idx = args.first().map_or(0, |v| v.to_number() as i64);
            let chars: Vec<char> = text.chars().collect();
            let len = chars.len() as i64;
            let real_idx = if idx < 0 { len + idx } else { idx };
            if real_idx < 0 || real_idx >= len {
                return Value::Undefined;
            }
            Value::string_owned(chars[real_idx as usize].to_string())
        })),
        "codePointAt" => Some(Box::new(move |args: &[Value]| {
            let idx = args.first().map_or(0, |v| v.to_number() as usize);
            text.chars()
                .nth(idx)
                .map(|c| Value::Number(c as u32 as f64))
                .unwrap_or(Value::Undefined)
        })),
        "concat" => Some(Box::new(move |args: &[Value]| {
            let mut result = text.clone();
            for arg in args {
                let s = arg.to_js_string();
                result.push_str(s.as_str().unwrap_or(""));
            }
            Value::string_owned(result)
        })),
        "match" => Some(Box::new(move |_args: &[Value]| {
            // Simplified: return null (no regex engine yet)
            Value::Null
        })),
        "search" => Some(Box::new(move |_args: &[Value]| Value::Number(-1.0))),
        "localeCompare" => Some(Box::new(move |args: &[Value]| {
            let other = args
                .first()
                .map(|v| {
                    let s = v.to_js_string();
                    s.as_str().unwrap_or("").to_string()
                })
                .unwrap_or_default();
            let cmp = text.cmp(&other);
            Value::Number(match cmp {
                std::cmp::Ordering::Less => -1.0,
                std::cmp::Ordering::Equal => 0.0,
                std::cmp::Ordering::Greater => 1.0,
            })
        })),
        "toString" | "valueOf" => {
            let s_clone = Rc::clone(s);
            return Some(Value::NativeFunction(Rc::new(NativeFunction::new(
                name,
                move |_args: &[Value]| Value::String(Rc::clone(&s_clone)),
            ))));
        }
        _ => None,
    };

    method.map(|f| Value::NativeFunction(Rc::new(NativeFunction::new(name, f))))
}
