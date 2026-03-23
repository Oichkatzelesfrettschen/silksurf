//! Global functions: parseInt, parseFloat, isNaN, isFinite,
//! encodeURIComponent, decodeURIComponent, String, Number, Boolean.

use super::native_fn;
use crate::vm::value::{Object, Value};

pub fn install(global: &mut Object) {
    global.set_by_str("parseInt", native_fn("parseInt", parse_int));
    global.set_by_str("parseFloat", native_fn("parseFloat", parse_float));
    global.set_by_str("isNaN", native_fn("isNaN", is_nan));
    global.set_by_str("isFinite", native_fn("isFinite", is_finite));
    global.set_by_str("encodeURIComponent", native_fn("encodeURIComponent", encode_uri_component));
    global.set_by_str("decodeURIComponent", native_fn("decodeURIComponent", decode_uri_component));
    global.set_by_str("encodeURI", native_fn("encodeURI", encode_uri));
    global.set_by_str("decodeURI", native_fn("decodeURI", decode_uri));
    // String/Number/Boolean as type conversion functions
    global.set_by_str("String", native_fn("String", string_fn));
    global.set_by_str("Number", native_fn("Number", number_fn));
    global.set_by_str("Boolean", native_fn("Boolean", boolean_fn));
    // undefined is already the default register value
    global.set_by_str("undefined", Value::Undefined);
    global.set_by_str("null", Value::Null);
}

fn parse_int(args: &[Value]) -> Value {
    let s = args.first().map(|v| v.to_js_string()).unwrap_or_default();
    let text = s.as_str().unwrap_or("").trim();

    let radix = args.get(1).map(|v| v.to_number() as u32).unwrap_or(0);

    // Determine effective radix
    let (text, radix) = if radix == 0 || radix == 16 {
        if let Some(rest) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
            (rest, 16)
        } else {
            (text, if radix == 0 { 10 } else { radix })
        }
    } else {
        (text, radix)
    };

    if !(2..=36).contains(&radix) {
        return Value::Number(f64::NAN);
    }

    // Parse as many valid digits as possible
    let mut result: i64 = 0;
    let mut found_digit = false;
    let negative = text.starts_with('-');
    let digits = if negative || text.starts_with('+') {
        &text[1..]
    } else {
        text
    };

    for ch in digits.chars() {
        let digit = match ch {
            '0'..='9' => (ch as u32) - ('0' as u32),
            'a'..='z' => 10 + (ch as u32) - ('a' as u32),
            'A'..='Z' => 10 + (ch as u32) - ('A' as u32),
            _ => break,
        };
        if digit >= radix {
            break;
        }
        found_digit = true;
        result = result.wrapping_mul(radix as i64).wrapping_add(digit as i64);
    }

    if !found_digit {
        return Value::Number(f64::NAN);
    }
    if negative {
        result = -result;
    }
    Value::Number(result as f64)
}

fn parse_float(args: &[Value]) -> Value {
    let s = args.first().map(|v| v.to_js_string()).unwrap_or_default();
    let text = s.as_str().unwrap_or("").trim();

    match text.parse::<f64>() {
        Ok(n) => Value::Number(n),
        Err(_) => {
            // Try parsing prefix (e.g., "3.14abc" -> 3.14)
            let mut end = 0;
            let bytes = text.as_bytes();
            // Skip sign
            if end < bytes.len() && matches!(bytes[end], b'+' | b'-') {
                end += 1;
            }
            // Integer part
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
            // Decimal part
            if end < bytes.len() && bytes[end] == b'.' {
                end += 1;
                while end < bytes.len() && bytes[end].is_ascii_digit() {
                    end += 1;
                }
            }
            // Exponent
            if end < bytes.len() && matches!(bytes[end], b'e' | b'E') {
                end += 1;
                if end < bytes.len() && matches!(bytes[end], b'+' | b'-') {
                    end += 1;
                }
                while end < bytes.len() && bytes[end].is_ascii_digit() {
                    end += 1;
                }
            }
            if end == 0 || (end == 1 && matches!(bytes[0], b'+' | b'-')) {
                Value::Number(f64::NAN)
            } else {
                text[..end]
                    .parse::<f64>()
                    .map(Value::Number)
                    .unwrap_or(Value::Number(f64::NAN))
            }
        }
    }
}

fn is_nan(args: &[Value]) -> Value {
    let n = args.first().map_or(f64::NAN, |v| v.to_number());
    Value::Boolean(n.is_nan())
}

fn is_finite(args: &[Value]) -> Value {
    let n = args.first().map_or(f64::NAN, |v| v.to_number());
    Value::Boolean(n.is_finite())
}

fn encode_uri_component(args: &[Value]) -> Value {
    let s = args.first().map(|v| v.to_js_string()).unwrap_or_default();
    let text = s.as_str().unwrap_or("");
    let mut result = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric()
            || matches!(byte, b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')')
        {
            result.push(byte as char);
        } else {
            result.push('%');
            result.push(HEX_UPPER[(byte >> 4) as usize] as char);
            result.push(HEX_UPPER[(byte & 0xF) as usize] as char);
        }
    }
    Value::string_owned(result)
}

fn decode_uri_component(args: &[Value]) -> Value {
    let s = args.first().map(|v| v.to_js_string()).unwrap_or_default();
    let text = s.as_str().unwrap_or("");
    let mut result = Vec::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                result.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        result.push(bytes[i]);
        i += 1;
    }
    Value::string_owned(String::from_utf8_lossy(&result).into_owned())
}

// encodeURI preserves more characters than encodeURIComponent
fn encode_uri(args: &[Value]) -> Value {
    let s = args.first().map(|v| v.to_js_string()).unwrap_or_default();
    let text = s.as_str().unwrap_or("");
    let mut result = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'-' | b'_'
                    | b'.'
                    | b'!'
                    | b'~'
                    | b'*'
                    | b'\''
                    | b'('
                    | b')'
                    | b';'
                    | b'/'
                    | b'?'
                    | b':'
                    | b'@'
                    | b'&'
                    | b'='
                    | b'+'
                    | b'$'
                    | b','
                    | b'#'
            )
        {
            result.push(byte as char);
        } else {
            result.push('%');
            result.push(HEX_UPPER[(byte >> 4) as usize] as char);
            result.push(HEX_UPPER[(byte & 0xF) as usize] as char);
        }
    }
    Value::string_owned(result)
}

fn decode_uri(args: &[Value]) -> Value {
    decode_uri_component(args) // Simplified -- full impl preserves reserved chars
}

fn string_fn(args: &[Value]) -> Value {
    let s = args.first().map(|v| v.to_js_string()).unwrap_or_default();
    Value::String(s)
}

fn number_fn(args: &[Value]) -> Value {
    let n = args.first().map_or(0.0, |v| v.to_number());
    Value::Number(n)
}

fn boolean_fn(args: &[Value]) -> Value {
    let b = args.first().is_some_and(|v| v.is_truthy());
    Value::Boolean(b)
}

fn hex_val(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(10 + byte - b'a'),
        b'A'..=b'F' => Some(10 + byte - b'A'),
        _ => None,
    }
}

const HEX_UPPER: &[u8; 16] = b"0123456789ABCDEF";
